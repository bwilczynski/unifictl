//! `devices list` / `devices get` (hlctl devices_*.tmpl + device_base.tmpl).

use super::Table;
use super::format::{format_bytes, format_bytes_per_sec, format_uptime};
use super::ports::decorate_port;
use super::style;
use crate::model::{DevicePort, NetworkDevice, NetworkDeviceDetail, NetworkDeviceList, PortState};

/// Lean by default; `wide` adds identity and link columns after them.
pub fn list(list: &NetworkDeviceList, wide: bool, color: bool) -> String {
    let mut t = Table::new();
    let mut header = vec!["ID", "NAME", "IP", "TYPE", "STATUS", "EXPERIENCE"];
    if wide {
        header.extend(["MAC", "MODEL", "FIRMWARE", "UPTIME", "UPLINK", "DOWN", "UP"]);
    }
    t.row(&header);
    for d in &list.items {
        let mut row = vec![
            d.id.clone(),
            style::name(&d.name, color),
            d.ip.clone(),
            d.device_type.as_str().to_string(),
            style::device_status(d.status, color),
            style::experience(d.experience, color),
        ];
        if wide {
            row.extend([
                d.mac.clone(),
                d.model.clone(),
                d.firmware_version.clone(),
                format_uptime(d.uptime),
                d.uplink.as_ref().map_or_else(
                    || "-".to_string(),
                    |u| style::uplink_speed(u.link_speed, u.max_link_speed, u.full_duplex, color),
                ),
                style::down(&format_bytes_per_sec(d.traffic.rx_bytes_per_sec), color),
                style::up(&format_bytes_per_sec(d.traffic.tx_bytes_per_sec), color),
            ]);
        }
        t.row(&row.iter().map(String::as_str).collect::<Vec<_>>());
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
                    &style::signal(Some(c.signal_strength), color),
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

fn base(t: &mut Table, b: &NetworkDevice, color: bool) {
    t.row(&["FIELD", "VALUE"]);
    t.row(&["ID", &b.id]);
    t.row(&["NAME", &style::name(&b.name, color)]);
    t.row(&["MAC", &b.mac]);
    t.row(&["IP", &b.ip]);
    t.row(&["TYPE", b.device_type.as_str()]);
    t.row(&["STATUS", &style::device_status(b.status, color)]);
    if let Some(e) = b.experience {
        t.row(&["EXPERIENCE", &style::experience(Some(e), color)]);
    }
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
        &style::down(
            &traffic(b.traffic.rx_bytes_per_sec, b.traffic.rx_bytes_total),
            color,
        ),
    ]);
    t.row(&[
        "TRAFFIC TX",
        &style::up(
            &traffic(b.traffic.tx_bytes_per_sec, b.traffic.tx_bytes_total),
            color,
        ),
    ]);
    if let Some(ul) = &b.uplink {
        let mut s = ul.device.name.clone();
        if let Some(port) = ul.port {
            s.push_str(&format!(" (port {port}"));
            if ul.link_speed.is_some() {
                s.push_str(&format!(
                    ", {}",
                    style::uplink_speed(ul.link_speed, ul.max_link_speed, ul.full_duplex, color)
                ));
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
            &style::down(&r.rx, color),
            &style::up(&r.tx, color),
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
    fn default_list_is_lean_with_experience() {
        let out = list(&svc().list_devices().unwrap(), false, false);
        assert_eq!(
            out,
            "\
ID                NAME              IP            TYPE         STATUS     EXPERIENCE
us-8              US 8              192.168.1.44  switch       connected  72%
uap-02            UAP-02            192.168.1.10  accessPoint  connected  45%
switch-flex-mini  Switch Flex Mini  192.168.1.12  switch       connected  -
uap-01            UAP-01            192.168.1.22  accessPoint  connected  98%
usw-flex-2-5g-8   USW Flex 2.5G 8   192.168.1.40  switch       connected  -
us-8-60w          US 8 60W          192.168.1.21  switch       connected  93%
cgf-01            CGF-01            192.168.1.10  gateway      connected  -
"
        );
    }

    #[test]
    fn empty_list_is_header_only() {
        let out = list(&NetworkDeviceList { items: vec![] }, false, false);
        assert_eq!(out.lines().count(), 1);
    }

    /// Cells of a table line (columns are separated by 2+ spaces).
    fn cells(line: &str) -> Vec<&str> {
        line.split("  ")
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .collect()
    }

    #[test]
    fn wide_list_adds_columns_after_default() {
        let out = list(&svc().list_devices().unwrap(), true, false);
        assert_eq!(
            cells(out.lines().next().unwrap()),
            [
                "ID",
                "NAME",
                "IP",
                "TYPE",
                "STATUS",
                "EXPERIENCE",
                "MAC",
                "MODEL",
                "FIRMWARE",
                "UPTIME",
                "UPLINK",
                "DOWN",
                "UP"
            ]
        );
        let row = |id: &str| cells(out.lines().find(|l| l.starts_with(id)).unwrap());
        let ap = row("uap-01 ");
        assert_eq!(ap[6], "aa:bb:cc:dd:04:01");
        assert_eq!(ap[7], "U7LT");
        assert_eq!(ap[10], "100 Mbps");
        assert_eq!(ap[11], "20.1 KB/s", "DOWN is the uplink's rx");
        assert_eq!(ap[12], "70.5 KB/s", "UP is the uplink's tx");
        assert_eq!(row("cgf-01")[10], "-", "gateway has no uplink");
    }

    #[test]
    fn wide_list_grades_uplinks() {
        let out = list(&svc().list_devices().unwrap(), true, true);
        let line = |id: &str| out.lines().find(|l| l.starts_with(id)).unwrap().to_string();
        assert!(
            line("uap-01").contains("\x1b[33m100 Mbps\x1b[0m"),
            "below max"
        );
        assert!(
            line("usw-flex-2-5g-8").contains("\x1b[32m2.5 GbE\x1b[0m"),
            "limited by upstream port"
        );
        assert!(
            line("switch-flex-mini").contains("\x1b[31m1 GbE\x1b[0m"),
            "half duplex"
        );
        assert!(line("us-8 ").contains("\x1b[33m72%\x1b[0m"));
        assert!(line("uap-02").contains("\x1b[31m45%\x1b[0m"));
    }

    #[test]
    fn detail_shows_experience_and_graded_uplink() {
        let out = get(&svc().get_device("uap-01").unwrap(), false, true);
        assert!(
            out.lines()
                .any(|l| l.starts_with("EXPERIENCE") && l.ends_with("\x1b[32m98%\x1b[0m"))
        );
        let uplink = out.lines().find(|l| l.starts_with("UPLINK")).unwrap();
        assert!(
            uplink.ends_with("(port 5, \x1b[33m100 Mbps\x1b[0m)"),
            "{uplink}"
        );
        let gw = get(&svc().get_device("cgf-01").unwrap(), false, false);
        assert!(!gw.contains("EXPERIENCE"), "no score, no row");
    }

    #[test]
    fn access_point_client_signal_is_graded() {
        let out = get(&svc().get_device("uap-01").unwrap(), false, true);
        assert!(out.contains("\x1b[33m-69 dBm\x1b[0m"), "{out}");
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

    #[test]
    fn traffic_rows_and_port_traffic_are_coloured() {
        let out = get(&svc().get_device("us-8-60w").unwrap(), false, true);
        let rx = out.lines().find(|l| l.starts_with("TRAFFIC RX")).unwrap();
        let tx = out.lines().find(|l| l.starts_with("TRAFFIC TX")).unwrap();
        assert!(rx.contains("\x1b[36m"), "{rx}");
        assert!(tx.contains("\x1b[35m"), "{tx}");
        let ports =
            &out[out.find("--- PORTS ---").unwrap()..out.find("--- VLAN CONFIG ---").unwrap()];
        assert!(
            ports
                .lines()
                .skip(2)
                .all(|l| l.is_empty() || (l.contains("\x1b[36m") && l.contains("\x1b[35m")))
        );
    }
}
