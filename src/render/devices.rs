//! `devices list` / `devices get` (hlctl devices_*.tmpl + device_base.tmpl).

use super::Table;
use super::format::{format_bytes, format_bytes_per_sec, format_link_speed, format_uptime};
use super::ports::decorate_port;
use super::style;
use crate::model::{DeviceBase, DevicePort, NetworkDeviceDetail, NetworkDeviceList, PortState};

pub fn list(list: &NetworkDeviceList, color: bool) -> String {
    let mut t = Table::new();
    t.row(&["ID", "NAME", "MAC", "IP", "TYPE", "STATUS"]);
    for d in &list.items {
        t.row(&[
            &d.id,
            &style::name(&d.name, color),
            &d.mac,
            &d.ip,
            d.device_type.as_str(),
            &style::device_status(d.status, color),
        ]);
    }
    t.finish()
}

/// Switch/gateway port tables show only `up` ports unless `all_ports`;
/// JSON output is unaffected (handled by the caller).
pub fn get(detail: &NetworkDeviceDetail, all_ports: bool, color: bool) -> String {
    let mut t = Table::new();
    match detail {
        NetworkDeviceDetail::Switch(sw) => {
            base(&mut t, &sw.base, color);
            port_sections(&mut t, &sw.ports, all_ports, color);
        }
        NetworkDeviceDetail::Gateway(gw) => {
            base(&mut t, &gw.base, color);
            port_sections(&mut t, &gw.ports, all_ports, color);
            section(&mut t, "--- WANS ---");
            t.row(&["ID", "NAME"]);
            for w in &gw.wans {
                t.row(&[&w.id, &style::name(&w.name, color)]);
            }
        }
        NetworkDeviceDetail::AccessPoint(ap) => {
            base(&mut t, &ap.base, color);
            section(&mut t, "--- CLIENTS ---");
            t.row(&["CLIENT", "SSID", "SIGNAL"]);
            for c in &ap.connected_clients {
                t.row(&[
                    &c.client.name,
                    &c.ssid,
                    &format!("{} dBm", c.signal_strength),
                ]);
            }
        }
        NetworkDeviceDetail::Unknown(b) => base(&mut t, b, color),
    }
    t.finish()
}

/// `{{ flush }}` + blank line + section title.
fn section(t: &mut Table, title: &str) {
    t.flush();
    t.blank();
    t.line(title);
}

fn base(t: &mut Table, b: &DeviceBase, color: bool) {
    t.row(&["FIELD", "VALUE"]);
    t.row(&["ID", &b.id]);
    t.row(&["NAME", &style::name(&b.name, color)]);
    t.row(&["MAC", &b.mac]);
    t.row(&["IP", &b.ip]);
    t.row(&["TYPE", b.device_type.as_str()]);
    t.row(&["STATUS", &style::device_status(b.status, color)]);
    t.row(&["MODEL", &b.model]);
    t.row(&["FIRMWARE", &b.firmware_version]);
    t.row(&["UPTIME", &format_uptime(b.uptime)]);
    let traffic = |rate: i64, total: i64| {
        format!(
            "{} ({} total)",
            format_bytes_per_sec(rate),
            format_bytes(total)
        )
    };
    t.row(&[
        "TRAFFIC RX",
        &traffic(b.traffic.rx_bytes_per_sec, b.traffic.rx_bytes_total),
    ]);
    t.row(&[
        "TRAFFIC TX",
        &traffic(b.traffic.tx_bytes_per_sec, b.traffic.tx_bytes_total),
    ]);
    if let Some(ul) = &b.uplink {
        let mut s = ul.device.name.clone();
        if let Some(port) = ul.port {
            s.push_str(&format!(" (port {port}"));
            if let Some(ls) = ul.link_speed {
                s.push_str(&format!(", {}", format_link_speed(ls)));
            }
            s.push(')');
        }
        t.row(&["UPLINK", &s]);
    }
}

fn port_sections(t: &mut Table, ports: &[DevicePort], all_ports: bool, color: bool) {
    let shown: Vec<&DevicePort> = ports
        .iter()
        .filter(|p| all_ports || p.state == PortState::Up)
        .collect();
    section(t, "--- PORTS ---");
    t.row(&[
        "PORT",
        "LABEL",
        "STATE",
        "SPEED",
        "UPTIME",
        "SFP",
        "LAG",
        "POE",
        "POE WATTS",
        "RX",
        "TX",
        "CONNECTED TO",
    ]);
    for p in &shown {
        let r = decorate_port(p);
        t.row(&[
            &p.number.to_string(),
            &r.label,
            &style::port_state(p.state, color),
            &r.link_speed,
            &r.link_uptime,
            &r.sfp_present,
            &r.lag_info,
            &r.poe_mode,
            &r.poe_watts,
            &r.rx,
            &r.tx,
            &r.connected_to,
        ]);
    }
    section(t, "--- VLAN CONFIG ---");
    t.row(&["PORT", "MODE", "NATIVE VLAN", "TAGGED VLANS"]);
    for p in &shown {
        let r = decorate_port(p);
        t.row(&[
            &p.number.to_string(),
            &r.vlan_mode,
            &r.native_vlan,
            &r.tagged_vlans,
        ]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::Service;
    use crate::unifi::fixtures::FixtureApi;

    fn svc() -> Service {
        Service::new(
            Box::new(FixtureApi::load(&FixtureApi::default_dir()).unwrap()),
            30,
        )
    }

    #[test]
    fn list_has_header_and_rows() {
        let out = list(&svc().list_devices().unwrap(), false);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 8);
        assert!(lines[0].starts_with("ID"));
        assert!(lines[7].starts_with("cgf-01"));
        assert!(lines[7].contains("gateway") && lines[7].contains("connected"));
    }

    #[test]
    fn empty_list_is_header_only() {
        let out = list(&NetworkDeviceList { items: vec![] }, false);
        assert_eq!(out.lines().count(), 1);
    }

    #[test]
    fn switch_sections_and_active_port_filter() {
        let detail = svc().get_device("us-8-60w").unwrap();
        let out = get(&detail, false, false);
        assert!(out.starts_with("FIELD"));
        assert!(out.contains("\n\n--- PORTS ---\nPORT"));
        assert!(out.contains("\n\n--- VLAN CONFIG ---\nPORT"));
        assert!(
            out.lines()
                .any(|l| l.starts_with("UPLINK") && l.contains("CGF-01"))
        );
        let section = |name: &str| -> usize {
            let start = out.find(name).unwrap();
            out[start..]
                .lines()
                .skip(2)
                .take_while(|l| !l.is_empty())
                .count()
        };
        assert_eq!(section("--- PORTS ---"), 7, "only up ports by default");
        let all = get(&detail, true, false);
        let start = all.find("--- PORTS ---").unwrap();
        assert_eq!(
            all[start..]
                .lines()
                .skip(2)
                .take_while(|l| !l.is_empty())
                .count(),
            12
        );
    }

    #[test]
    fn gateway_has_wans_section_and_no_uplink() {
        let out = get(&svc().get_device("cgf-01").unwrap(), true, false);
        assert!(out.contains("\n\n--- WANS ---\nID"));
        assert!(out.lines().any(|l| l.starts_with("internet-1")));
        assert!(!out.contains("UPLINK"));
    }

    #[test]
    fn access_point_has_clients_section() {
        let out = get(&svc().get_device("uap-01").unwrap(), false, false);
        assert!(out.contains("--- CLIENTS ---\nCLIENT"));
        assert!(
            out.lines()
                .any(|l| l.starts_with("MacBook Pro") && l.ends_with("-69 dBm"))
        );
    }
}
