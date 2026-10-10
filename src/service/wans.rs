//! WANs: ported from homelab-api/internal/network/wans_service.go.

use super::Service;
use super::common::*;
use crate::error::{Error, Result};
use crate::model::*;
use crate::unifi::types::{Device, NetworkConf, WanIface};

impl Service {
    pub fn list_wans(&self) -> Result<WanList> {
        let networks = self.api.network_conf()?;
        let devices = self.api.devices()?;
        let gw = find_gateway(&devices);
        let items = networks
            .iter()
            .filter(|n| n.purpose == "wan")
            .map(|n| {
                let (ip_address, status, uptime) =
                    live_fields(resolve_iface(gw, &n.wan_networkgroup), gw);
                Wan {
                    id: to_kebab(&n.name),
                    name: n.name.clone(),
                    ip_address,
                    status,
                    uptime,
                }
            })
            .collect();
        Ok(WanList { items })
    }

    pub fn get_wan(&self, id: &str) -> Result<WanDetail> {
        let networks = self.api.network_conf()?;
        let devices = self.api.devices()?;
        let gw = find_gateway(&devices);
        let n = networks
            .iter()
            .filter(|n| n.purpose == "wan")
            .find(|n| to_kebab(&n.name) == id)
            .ok_or_else(|| Error::NotFound(format!("WAN not found: {id}")))?;
        let iface = resolve_iface(gw, &n.wan_networkgroup);
        let (ip_address, status, uptime) = live_fields(iface, gw);
        Ok(WanDetail {
            id: to_kebab(&n.name),
            name: n.name.clone(),
            ip_address,
            status,
            uptime,
            dns_servers: dns_servers(iface, n),
        })
    }
}

fn find_gateway(devices: &[Device]) -> Option<&Device> {
    devices.iter().find(|d| is_gateway_type(&d.kind))
}

/// "WAN" → wan1, "WAN2" → wan2.
fn resolve_iface<'a>(gw: Option<&'a Device>, group: &str) -> Option<&'a WanIface> {
    let gw = gw?;
    match group {
        "WAN" => gw.wan1.as_ref(),
        "WAN2" => gw.wan2.as_ref(),
        _ => None,
    }
}

fn live_fields(iface: Option<&WanIface>, gw: Option<&Device>) -> (String, WanStatus, i64) {
    let ip = iface.map(|i| i.ip.clone()).unwrap_or_default();
    let status = if iface.is_some_and(|i| i.up) {
        WanStatus::Connected
    } else {
        WanStatus::Disconnected
    };
    (ip, status, gw.map_or(0, |g| g.uptime))
}

/// Configured DNS wins; otherwise what the interface reports.
fn dns_servers(iface: Option<&WanIface>, n: &NetworkConf) -> Vec<String> {
    let configured = collect_dns(&[&n.wan_dns1, &n.wan_dns2]);
    if !configured.is_empty() {
        return configured;
    }
    iface
        .map(|i| collect_dns(&i.dns.iter().map(String::as_str).collect::<Vec<_>>()))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use crate::service::testutil::{fixtures, svc};
    use crate::unifi::fixtures::FixtureApi;

    #[test]
    fn list() {
        let items = svc(fixtures()).list_wans().unwrap().items;
        assert_eq!(items.len(), 2);
        let w = &items[0];
        assert_eq!(w.id, "internet-1");
        assert_eq!(w.name, "Internet 1");
        assert_eq!(w.ip_address, "203.0.113.1");
        assert_eq!(w.status, WanStatus::Connected);
        assert!(w.uptime > 0);
    }

    #[test]
    fn list_empty() {
        assert!(
            svc(FixtureApi::default())
                .list_wans()
                .unwrap()
                .items
                .is_empty()
        );
    }

    #[test]
    fn get() {
        let d = svc(fixtures()).get_wan("internet-1").unwrap();
        assert_eq!(d.ip_address, "203.0.113.1");
        assert_eq!(d.status, WanStatus::Connected);
        assert_eq!(d.dns_servers, ["8.8.8.8", "8.8.4.4"]);
    }

    #[test]
    fn no_gateway_means_disconnected() {
        let api = FixtureApi {
            devices: vec![],
            ..fixtures()
        };
        let w = &svc(api).list_wans().unwrap().items[0];
        assert_eq!(w.status, WanStatus::Disconnected);
        assert_eq!(w.ip_address, "");
        assert_eq!(w.uptime, 0);
    }

    #[test]
    fn get_not_found() {
        let err = svc(fixtures()).get_wan("nonexistent").unwrap_err();
        assert!(matches!(err, Error::NotFound(_)));
        assert_eq!(err.to_string(), "WAN not found: nonexistent");
    }
}
