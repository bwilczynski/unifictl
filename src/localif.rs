//! This machine's network interfaces, for `clients list --self`.

#[cfg(target_os = "linux")]
use std::path::Path;

use crate::error::{Error, Result};

/// One local network interface as the OS reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interface {
    pub name: String,
    /// As read, any case; empty when there is no link-layer address.
    pub mac: String,
    /// The link is live, not just administratively enabled.
    pub up: bool,
}

/// Virtual and loopback interfaces: never a client of the controller.
const SKIP_PREFIXES: &[&str] = &[
    "docker",
    "br-",
    "veth",
    "tailscale",
    "wg",
    "tun",
    "tap",
    "utun",
    "awdl",
    "llw",
    "bridge",
    "gif",
    "stf",
    "anpi",
    "ap",
];

fn skipped(name: &str) -> bool {
    matches!(name, "lo" | "lo0") || SKIP_PREFIXES.iter().any(|p| name.starts_with(p))
}

/// Six hex bytes, not all zero.
fn is_hw_mac(mac: &str) -> bool {
    let bytes: Vec<&str> = mac.split([':', '-']).collect();
    bytes.len() == 6
        && bytes
            .iter()
            .all(|b| b.len() == 2 && b.chars().all(|c| c.is_ascii_hexdigit()))
        && bytes.iter().any(|b| *b != "00")
}

/// The MACs of the interfaces that could be a controller client: up, physical,
/// with a real hardware address. Returned as read; callers normalise.
pub fn usable_macs(ifaces: &[Interface]) -> Vec<String> {
    ifaces
        .iter()
        .filter(|i| i.up && !skipped(&i.name) && is_hw_mac(&i.mac))
        .map(|i| i.mac.clone())
        .collect()
}

fn read_error(cause: impl std::fmt::Display) -> Error {
    Error::Other(format!("cannot read local network interfaces: {cause}"))
}

/// This machine's interfaces.
pub fn read() -> Result<Vec<Interface>> {
    #[cfg(target_os = "linux")]
    return read_sysfs(Path::new("/sys/class/net"));
    #[cfg(target_os = "macos")]
    return macos::read();
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    Err(read_error("unsupported platform"))
}

/// Some drivers never report `up`; for those, carrier present means live.
#[cfg_attr(not(any(test, target_os = "linux")), allow(dead_code))]
fn linux_up(operstate: &str, carrier: Option<&str>) -> bool {
    operstate == "up" || (operstate == "unknown" && carrier == Some("1"))
}

/// `IFM_AVALID` and `IFM_ACTIVE` from macOS `<net/if_media.h>`.
const IFM_AVALID: i32 = 0x1;
const IFM_ACTIVE: i32 = 0x2;

/// macOS keeps RUNNING set on an unplugged Ethernet port, so the media status
/// (what `ifconfig` shows as `status: active`) decides when the interface
/// reports one; otherwise the flags do.
#[cfg_attr(not(any(test, target_os = "macos")), allow(dead_code))]
fn macos_up(up: bool, running: bool, media_status: Option<i32>) -> bool {
    match media_status {
        Some(s) if s & IFM_AVALID != 0 => up && s & IFM_ACTIVE != 0,
        _ => up && running,
    }
}

/// Interfaces under a sysfs `class/net` directory, sorted by name. Only an
/// unreadable `root` is an error; missing per-interface files mean "no
/// address" or "not up" (reading `carrier` of a down link fails with EINVAL).
#[cfg(target_os = "linux")]
pub fn read_sysfs(root: &Path) -> Result<Vec<Interface>> {
    let attr = |dir: &Path, f: &str| {
        std::fs::read_to_string(dir.join(f))
            .ok()
            .map(|s| s.trim().to_string())
    };
    let mut out = Vec::new();
    for entry in std::fs::read_dir(root).map_err(read_error)? {
        let dir = entry.map_err(read_error)?.path();
        let Some(name) = dir.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let operstate = attr(&dir, "operstate").unwrap_or_else(|| "unknown".into());
        out.push(Interface {
            name: name.to_string(),
            mac: attr(&dir, "address").unwrap_or_default(),
            up: linux_up(&operstate, attr(&dir, "carrier").as_deref()),
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

#[cfg(target_os = "macos")]
mod macos {
    use super::{Interface, macos_up, read_error};
    use crate::error::Result;

    /// `struct ifmediareq` from `<net/if.h>`, declared under `#pragma pack(4)`.
    #[repr(C, packed(4))]
    struct IfMediaReq {
        ifm_name: [libc::c_char; libc::IFNAMSIZ],
        ifm_current: libc::c_int,
        ifm_mask: libc::c_int,
        ifm_status: libc::c_int,
        ifm_active: libc::c_int,
        ifm_count: libc::c_int,
        ifm_ulist: *mut libc::c_int,
    }

    // SIOCGIFMEDIA encodes the struct size (44 bytes on 64-bit); a layout
    // mismatch makes every ioctl fail and silently drops the media check.
    const _: () = assert!(
        std::mem::size_of::<IfMediaReq>() as libc::c_ulong == (libc::SIOCGIFMEDIA >> 16) & 0x1fff
    );
    const _: () = assert!(std::mem::size_of::<IfMediaReq>() == 44);

    /// The interface's media status, or `None` when it has none to report.
    fn media_status(sock: libc::c_int, name: &std::ffi::CStr) -> Option<i32> {
        if sock < 0 {
            return None;
        }
        let bytes = name.to_bytes();
        if bytes.len() >= libc::IFNAMSIZ {
            return None;
        }
        // SAFETY: all-zero is a valid ifmediareq (no media list requested).
        let mut req: IfMediaReq = unsafe { std::mem::zeroed() };
        for (dst, src) in req.ifm_name.iter_mut().zip(bytes) {
            *dst = *src as libc::c_char;
        }
        // SAFETY: `req` is a properly sized, NUL-terminated ifmediareq.
        let rc = unsafe { libc::ioctl(sock, libc::SIOCGIFMEDIA, &mut req) };
        (rc == 0).then_some(req.ifm_status)
    }

    /// `AF_LINK` entries of `getifaddrs`, one per interface.
    pub fn read() -> Result<Vec<Interface>> {
        let mut head: *mut libc::ifaddrs = std::ptr::null_mut();
        // SAFETY: getifaddrs fills `head` with a list we free below.
        if unsafe { libc::getifaddrs(&mut head) } != 0 {
            return Err(read_error(std::io::Error::last_os_error()));
        }
        // SAFETY: a plain datagram socket, only used for SIOCGIFMEDIA and
        // closed below; a failure just means no media status.
        let sock = unsafe { libc::socket(libc::AF_INET, libc::SOCK_DGRAM, 0) };
        let mut out = Vec::new();
        let mut cur = head;
        while !cur.is_null() {
            // SAFETY: `cur` is a node of the list getifaddrs returned.
            let ifa = unsafe { &*cur };
            cur = ifa.ifa_next;
            // SAFETY: a non-null ifa_addr points to a sockaddr.
            if ifa.ifa_addr.is_null()
                || i32::from(unsafe { (*ifa.ifa_addr).sa_family }) != libc::AF_LINK
            {
                continue;
            }
            // SAFETY: AF_LINK addresses are sockaddr_dl; the link-layer
            // address follows the name inside sdl_data, within sdl_len.
            // SAFETY: ifa_name is a NUL-terminated C string.
            let cname = unsafe { std::ffi::CStr::from_ptr(ifa.ifa_name) };
            let name = cname.to_string_lossy().into_owned();
            let mac = unsafe {
                let sdl = ifa.ifa_addr as *const libc::sockaddr_dl;
                let data = std::ptr::addr_of!((*sdl).sdl_data) as *const u8;
                if (*sdl).sdl_alen == 6 {
                    let bytes = std::slice::from_raw_parts(data.add((*sdl).sdl_nlen as usize), 6);
                    bytes
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect::<Vec<_>>()
                        .join(":")
                } else {
                    String::new()
                }
            };
            let flags = ifa.ifa_flags as libc::c_int;
            out.push(Interface {
                name,
                mac,
                up: macos_up(
                    flags & libc::IFF_UP != 0,
                    flags & libc::IFF_RUNNING != 0,
                    media_status(sock, cname),
                ),
            });
        }
        if sock >= 0 {
            // SAFETY: `sock` is the socket opened above.
            unsafe { libc::close(sock) };
        }
        // SAFETY: `head` came from a successful getifaddrs.
        unsafe { libc::freeifaddrs(head) };
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn i(name: &str, mac: &str, up: bool) -> Interface {
        Interface {
            name: name.into(),
            mac: mac.into(),
            up,
        }
    }

    #[test]
    fn usable_macs_keeps_up_physical_interfaces() {
        let ifaces = [
            i("enp0s31f6", "68:ee:ee:00:00:02", true),
            i("wlan0", "3c:ee:ee:00:00:01", true),
            i("lo", "00:00:00:00:00:00", true),
            i("docker0", "02:42:ac:00:00:01", true),
            i("br-1a2b", "02:42:ac:00:00:02", true),
            i("veth9", "02:42:ac:00:00:03", true),
            i("tailscale0", "", true),
            i("wg0", "", true),
            i("tun0", "", true),
            i("utun3", "", true),
            i("awdl0", "aa:ee:ee:00:00:09", true),
            i("eth1", "68:ee:ee:00:00:07", false),
            i("eth2", "00:00:00:00:00:00", true),
            i("eth3", "68:ee:ee:00:00", true),
        ];
        assert_eq!(
            usable_macs(&ifaces),
            ["68:ee:ee:00:00:02", "3c:ee:ee:00:00:01"]
        );
    }

    #[test]
    fn linux_up_rules() {
        assert!(linux_up("up", None));
        assert!(linux_up("unknown", Some("1")));
        assert!(!linux_up("unknown", Some("0")));
        assert!(!linux_up("unknown", None));
        assert!(!linux_up("down", Some("1")));
        assert!(!linux_up("dormant", Some("1")));
    }

    #[test]
    fn macos_up_prefers_media_status() {
        const AVALID: i32 = 0x1;
        const ACTIVE: i32 = 0x2;
        // Unplugged built-in Ethernet: UP and RUNNING, but media inactive.
        assert!(!macos_up(true, true, Some(AVALID)));
        assert!(macos_up(true, true, Some(AVALID | ACTIVE)));
        assert!(!macos_up(false, true, Some(AVALID | ACTIVE)));
        // No media status (ioctl unsupported or not valid): fall back to flags.
        assert!(macos_up(true, true, None));
        assert!(macos_up(true, true, Some(0)));
        assert!(!macos_up(true, false, None));
    }

    #[cfg(target_os = "linux")]
    fn iface_dir(root: &Path, name: &str, files: &[(&str, &str)]) {
        let dir = root.join(name);
        std::fs::create_dir(&dir).unwrap();
        for (f, v) in files {
            std::fs::write(dir.join(f), v).unwrap();
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn read_sysfs_reads_address_and_state() {
        let root = tempfile::tempdir().unwrap();
        iface_dir(
            root.path(),
            "wlan0",
            &[
                ("address", "3c:ee:ee:00:00:01\n"),
                ("operstate", "unknown\n"),
                ("carrier", "1\n"),
            ],
        );
        iface_dir(
            root.path(),
            "eth0",
            &[("address", "68:ee:ee:00:00:02\n"), ("operstate", "up\n")],
        );
        assert_eq!(
            read_sysfs(root.path()).unwrap(),
            [
                i("eth0", "68:ee:ee:00:00:02", true),
                i("wlan0", "3c:ee:ee:00:00:01", true)
            ]
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn read_sysfs_skips_unreadable_carrier() {
        let root = tempfile::tempdir().unwrap();
        iface_dir(
            root.path(),
            "eth0",
            &[("address", "68:ee:ee:00:00:02\n"), ("operstate", "down\n")],
        );
        assert_eq!(
            read_sysfs(root.path()).unwrap(),
            [i("eth0", "68:ee:ee:00:00:02", false)]
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn read_sysfs_missing_root_is_error() {
        let err = read_sysfs(Path::new("/nonexistent/sys/class/net")).unwrap_err();
        assert!(
            err.to_string()
                .starts_with("cannot read local network interfaces: "),
            "{err}"
        );
    }
}
