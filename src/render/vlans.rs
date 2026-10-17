//! vlans_list.tmpl, vlans_get.tmpl.

use super::Table;
use super::style;
use crate::model::{DhcpMode, Vlan, VlanList};

pub fn list(list: &VlanList, wide: bool, color: bool) -> String {
    let mut t = Table::new();
    let mut header = vec!["ID", "NAME", "VLAN ID", "SUBNET"];
    if wide {
        header.extend(["GATEWAY IP", "DHCP MODE", "DHCP RANGE"]);
    }
    t.row(&header);
    for v in &list.items {
        let mut row = vec![
            style::printable(&v.id),
            style::name(&v.name, color),
            v.vlan_id.to_string(),
            style::printable(&v.subnet),
        ];
        if wide {
            row.extend([
                if v.gateway_ip.is_empty() {
                    "-".to_string()
                } else {
                    style::printable(&v.gateway_ip)
                },
                v.dhcp_mode.as_str().to_string(),
                dhcp_range(v).unwrap_or_else(|| "-".to_string()),
            ]);
        }
        t.row(&row.iter().map(String::as_str).collect::<Vec<_>>());
    }
    t.finish()
}

/// "start - end", only in DHCP server mode.
fn dhcp_range(v: &Vlan) -> Option<String> {
    match (v.dhcp_mode, &v.dhcp_range) {
        (DhcpMode::Server, Some(r)) => Some(format!(
            "{} - {}",
            style::printable(&r.start),
            style::printable(&r.end)
        )),
        _ => None,
    }
}

pub fn get(v: &Vlan, color: bool) -> String {
    let mut t = Table::new();
    t.row(&["FIELD", "VALUE"]);
    t.row(&["ID", &style::printable(&v.id)]);
    t.row(&["NAME", &style::name(&v.name, color)]);
    t.row(&["VLAN ID", &v.vlan_id.to_string()]);
    t.row(&["SUBNET", &style::printable(&v.subnet)]);
    t.row(&["GATEWAY IP", &style::printable(&v.gateway_ip)]);
    t.row(&["BROADCAST", &style::printable(&v.broadcast_ip)]);
    t.row(&["DHCP MODE", v.dhcp_mode.as_str()]);
    if let Some(r) = dhcp_range(v) {
        t.row(&["DHCP RANGE", &r]);
    }
    if let (DhcpMode::Relay, Some(relay)) = (v.dhcp_mode, &v.relay_server) {
        t.row(&["RELAY", &style::printable(relay)]);
    }
    t.row(&["DNS", &style::printable(&v.dns_servers.join(", "))]);
    t.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    fn detail(mode: DhcpMode) -> Vlan {
        Vlan {
            id: "lan-iot".into(),
            name: "LAN-IOT".into(),
            vlan_id: 20,
            subnet: "192.168.20.0/24".into(),
            gateway_ip: "192.168.20.1".into(),
            broadcast_ip: "192.168.20.255".into(),
            dhcp_mode: mode,
            dhcp_range: Some(DhcpRange {
                start: "192.168.20.6".into(),
                end: "192.168.20.254".into(),
            }),
            relay_server: Some("10.0.0.1".into()),
            dns_servers: vec!["1.1.1.1".into(), "8.8.8.8".into()],
        }
    }

    #[test]
    fn server_mode_shows_range_not_relay() {
        let out = get(&detail(DhcpMode::Server), false);
        assert!(
            out.lines().any(
                |l| l.starts_with("DHCP RANGE") && l.ends_with("192.168.20.6 - 192.168.20.254")
            )
        );
        assert!(!out.contains("RELAY"));
        assert!(out.lines().last().unwrap().ends_with("1.1.1.1, 8.8.8.8"));
    }

    #[test]
    fn relay_mode_shows_relay_not_range() {
        let out = get(&detail(DhcpMode::Relay), false);
        assert!(
            out.lines()
                .any(|l| l.starts_with("RELAY") && l.ends_with("10.0.0.1"))
        );
        assert!(!out.contains("DHCP RANGE"));
    }

    #[test]
    fn wide_list_adds_gateway_and_dhcp() {
        let l = VlanList {
            items: vec![
                detail(DhcpMode::Server),
                Vlan {
                    id: "lan-x".into(),
                    ..detail(DhcpMode::Relay)
                },
            ],
        };
        let narrow = list(&l, false, false);
        assert_eq!(
            narrow
                .lines()
                .next()
                .unwrap()
                .split_whitespace()
                .collect::<Vec<_>>(),
            ["ID", "NAME", "VLAN", "ID", "SUBNET"]
        );
        let out = list(&l, true, false);
        assert!(
            out.lines()
                .next()
                .unwrap()
                .ends_with("GATEWAY IP    DHCP MODE  DHCP RANGE"),
            "{out}"
        );
        assert!(
            out.lines()
                .nth(1)
                .unwrap()
                .ends_with("192.168.20.1  server     192.168.20.6 - 192.168.20.254"),
            "{out}"
        );
        assert!(
            out.lines().nth(2).unwrap().ends_with("relay      -"),
            "range only for server: {out}"
        );
    }
}
