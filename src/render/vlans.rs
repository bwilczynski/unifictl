//! vlans_list.tmpl, vlans_get.tmpl.

use super::Table;
use super::style;
use crate::model::{DhcpMode, VlanDetail, VlanList};

pub fn list(list: &VlanList, color: bool) -> String {
    let mut t = Table::new();
    t.row(&["ID", "NAME", "VLAN ID", "SUBNET"]);
    for v in &list.items {
        t.row(&[
            &v.id,
            &style::name(&v.name, color),
            &v.vlan_id.to_string(),
            &v.subnet,
        ]);
    }
    t.finish()
}

pub fn get(v: &VlanDetail, color: bool) -> String {
    let mut t = Table::new();
    t.row(&["FIELD", "VALUE"]);
    t.row(&["ID", &v.id]);
    t.row(&["NAME", &style::name(&v.name, color)]);
    t.row(&["VLAN ID", &v.vlan_id.to_string()]);
    t.row(&["SUBNET", &v.subnet]);
    t.row(&["GATEWAY IP", &v.gateway_ip]);
    t.row(&["BROADCAST", &v.broadcast_ip]);
    t.row(&["DHCP MODE", v.dhcp_mode.as_str()]);
    if let (DhcpMode::Server, Some(r)) = (v.dhcp_mode, &v.dhcp_range) {
        t.row(&["DHCP RANGE", &format!("{} - {}", r.start, r.end)]);
    }
    if let (DhcpMode::Relay, Some(relay)) = (v.dhcp_mode, &v.relay_server) {
        t.row(&["RELAY", relay]);
    }
    t.row(&["DNS", &v.dns_servers.join(", ")]);
    t.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    fn detail(mode: DhcpMode) -> VlanDetail {
        VlanDetail {
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
}
