//! VLANs: ported from homelab-api/internal/network/vlans_service.go.

use std::net::Ipv4Addr;

use super::Service;
use super::common::*;
use crate::error::{Error, Result};
use crate::model::*;
use crate::unifi::types::NetworkConf;

impl Service {
    pub fn list_vlans(&self) -> Result<VlanList> {
        let items = self
            .api
            .network_conf()?
            .iter()
            .filter(|n| is_lan_network(n))
            .map(build_vlan)
            .collect();
        Ok(VlanList { items })
    }

    pub fn get_vlan(&self, id: &str) -> Result<Vlan> {
        let networks = self.api.network_conf()?;
        let n = networks
            .iter()
            .filter(|n| is_lan_network(n))
            .find(|n| to_kebab(&n.name) == id)
            .ok_or_else(|| Error::NotFound(format!("VLAN not found: {id}")))?;
        Ok(build_vlan(n))
    }
}

fn is_lan_network(n: &NetworkConf) -> bool {
    n.purpose == "corporate" || n.purpose == "guest"
}

fn build_vlan(n: &NetworkConf) -> Vlan {
    let (subnet, gateway_ip, broadcast_ip) = parse_ip_subnet(&n.ip_subnet);
    let dhcp_mode = if n.dhcpd_enabled {
        DhcpMode::Server
    } else if n.dhcp_relay_enabled {
        DhcpMode::Relay
    } else {
        DhcpMode::Disabled
    };
    Vlan {
        id: to_kebab(&n.name),
        name: n.name.clone(),
        vlan_id: extract_vlan_id(&n.vlan),
        subnet,
        gateway_ip,
        broadcast_ip,
        dhcp_mode,
        dhcp_range: (dhcp_mode == DhcpMode::Server).then(|| DhcpRange {
            start: n.dhcpd_start.clone(),
            end: n.dhcpd_stop.clone(),
        }),
        relay_server: (dhcp_mode == DhcpMode::Relay).then(|| n.dhcpd_start.clone()),
        dns_servers: collect_dns(&[&n.dhcpd_dns_1, &n.dhcpd_dns_2]),
    }
}

/// "192.168.20.1/24" → ("192.168.20.0/24", "192.168.20.1", "192.168.20.255").
/// Unparseable (including IPv6) → (input, "", ""), like Go's error branch.
fn parse_ip_subnet(s: &str) -> (String, String, String) {
    if s.is_empty() {
        return Default::default();
    }
    let parsed = s.split_once('/').and_then(|(ip, len)| {
        Some((
            ip.parse::<Ipv4Addr>().ok()?,
            len.parse::<u32>().ok().filter(|l| *l <= 32)?,
        ))
    });
    let Some((ip, len)) = parsed else {
        return (s.to_string(), String::new(), String::new());
    };
    let mask = if len == 0 { 0 } else { u32::MAX << (32 - len) };
    let network = u32::from(ip) & mask;
    let broadcast = network | !mask;
    (
        format!("{}/{len}", Ipv4Addr::from(network)),
        ip.to_string(),
        Ipv4Addr::from(broadcast).to_string(),
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::error::Error;
    use crate::service::testutil::{fixtures, svc};
    use crate::unifi::fixtures::FixtureApi;

    #[test]
    fn list_excludes_wan() {
        let items = svc(fixtures()).list_vlans().unwrap().items;
        assert_eq!(items.len(), 4);
        let by_id = |id: &str| items.iter().find(|v| v.id == id).unwrap();
        assert_eq!(by_id("lan-mgmt").vlan_id, 1);
        assert_eq!(by_id("lan-mgmt").subnet, "192.168.1.0/24");
        assert_eq!(by_id("lan-iot").vlan_id, 20);
        assert_eq!(by_id("lan-iot").subnet, "192.168.20.0/24");
        assert_eq!(by_id("lan-srv").vlan_id, 100);
    }

    #[test]
    fn list_empty() {
        assert!(
            svc(FixtureApi::default())
                .list_vlans()
                .unwrap()
                .items
                .is_empty()
        );
    }

    #[test]
    fn get_server_dhcp() {
        let d = svc(fixtures()).get_vlan("lan-iot").unwrap();
        assert_eq!(d.vlan_id, 20);
        assert_eq!(d.subnet, "192.168.20.0/24");
        assert_eq!(d.gateway_ip, "192.168.20.1");
        assert_eq!(d.broadcast_ip, "192.168.20.255");
        assert_eq!(d.dhcp_mode, DhcpMode::Server);
        assert_eq!(
            d.dhcp_range,
            Some(DhcpRange {
                start: "192.168.20.6".into(),
                end: "192.168.20.254".into()
            })
        );
        assert_eq!(d.dns_servers, ["192.168.20.1"]);
    }

    #[test]
    fn get_relay_dhcp() {
        let api = FixtureApi {
            networks: vec![NetworkConf {
                id: "bb30".into(),
                name: "LAN-RELAY".into(),
                purpose: "corporate".into(),
                vlan: json!(30),
                vlan_enabled: true,
                ip_subnet: "192.168.30.1/24".into(),
                dhcp_relay_enabled: true,
                dhcpd_start: "192.168.30.200".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let d = svc(api).get_vlan("lan-relay").unwrap();
        assert_eq!(d.dhcp_mode, DhcpMode::Relay);
        assert_eq!(d.relay_server.as_deref(), Some("192.168.30.200"));
        assert_eq!(d.dhcp_range, None);
    }

    #[test]
    fn get_dns_variants() {
        let s = svc(fixtures());
        assert_eq!(
            s.get_vlan("lan-int").unwrap().dns_servers,
            ["192.168.100.5", "192.168.10.1"]
        );
        assert!(s.get_vlan("lan-srv").unwrap().dns_servers.is_empty());
        let mgmt = s.get_vlan("lan-mgmt").unwrap();
        assert_eq!(mgmt.vlan_id, 1);
        assert_eq!(mgmt.gateway_ip, "192.168.1.1");
        assert_eq!(mgmt.broadcast_ip, "192.168.1.255");
    }

    #[test]
    fn get_not_found() {
        let err = svc(fixtures()).get_vlan("nonexistent").unwrap_err();
        assert!(matches!(err, Error::NotFound(_)));
        assert_eq!(err.to_string(), "VLAN not found: nonexistent");
    }

    #[test]
    fn parse_ip_subnet_cases() {
        assert_eq!(
            parse_ip_subnet("10.0.5.1/22"),
            ("10.0.4.0/22".into(), "10.0.5.1".into(), "10.0.7.255".into())
        );
        assert_eq!(
            parse_ip_subnet(""),
            (String::new(), String::new(), String::new())
        );
        assert_eq!(
            parse_ip_subnet("garbage"),
            ("garbage".into(), String::new(), String::new())
        );
    }

    #[test]
    fn list_item_equals_get() {
        let s = svc(fixtures());
        for v in s.list_vlans().unwrap().items {
            assert_eq!(s.get_vlan(&v.id).unwrap(), v, "{}", v.id);
        }
    }
}
