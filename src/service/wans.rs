//! WANs: ported from homelab-api/internal/network/wans_service.go.

use super::Service;
use super::common::*;
use crate::error::{Error, Result};
use crate::model::*;
use crate::unifi::fetch;
use crate::unifi::types::{Device, NetworkConf, WanIface};

impl Service {
    pub fn list_wans(&self) -> Result<WanList> {
        fetch!(
            networks = self.api.network_conf(),
            devices = self.api.devices(),
        );
        let gw = find_gateway(&devices);
        let items = networks
            .iter()
            .filter(|n| n.purpose == "wan")
            .map(|n| build_wan(n, gw))
            .collect();
        Ok(WanList { items })
    }

    pub fn get_wan(&self, id: &str) -> Result<Wan> {
        fetch!(
            networks = self.api.network_conf(),
            devices = self.api.devices(),
        );
        let gw = find_gateway(&devices);
        let n = networks
            .iter()
            .filter(|n| n.purpose == "wan")
            .find(|n| to_kebab(&n.name) == id)
            .ok_or_else(|| Error::NotFound(format!("WAN not found: {id}")))?;
        Ok(build_wan(n, gw))
    }
}

fn build_wan(n: &NetworkConf, gw: Option<&Device>) -> Wan {
    let iface = resolve_iface(gw, &n.wan_networkgroup);
    let stats = gw.and_then(|g| g.uptime_stats.get(&n.wan_networkgroup));
    let status = if iface.is_some_and(|i| i.up) {
        WanStatus::Connected
    } else {
        WanStatus::Disconnected
    };
    Wan {
        id: to_kebab(&n.name),
        name: n.name.clone(),
        ip_address: iface.map(|i| i.ip.clone()).unwrap_or_default(),
        status,
        uptime: stats.and_then(|s| s.uptime),
        downtime: stats.and_then(|s| s.downtime),
        availability: stats.and_then(|s| s.availability),
        availability_period: stats.and_then(|s| s.time_period),
        latency: stats
            .and_then(|s| s.latency_average)
            .map(|ms| ms.round() as i64),
        dns_servers: dns_servers(iface, n),
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
    }

    #[test]
    fn uptime_is_the_wans_own_not_the_gateways() {
        let items = svc(fixtures()).list_wans().unwrap().items;
        let (up, down) = (&items[0], &items[1]);
        assert_eq!(up.uptime, Some(4253), "gateway uptime is 5627");
        assert_eq!(up.downtime, None);
        assert_eq!(up.availability, Some(100.0));
        assert_eq!(up.availability_period, Some(4270));
        assert_eq!(up.latency, Some(13));
        assert_eq!(down.status, WanStatus::Disconnected);
        assert_eq!(down.uptime, None);
        assert_eq!(down.downtime, Some(5577));
        assert_eq!(down.availability, None);
        assert_eq!(down.latency, None);
    }

    #[test]
    fn gateway_without_uptime_stats_reports_nothing() {
        let mut api = fixtures();
        for d in &mut api.devices {
            d.uptime_stats.clear();
        }
        let w = &svc(api).list_wans().unwrap().items[0];
        assert_eq!(w.status, WanStatus::Connected);
        assert_eq!(w.uptime, None, "never the gateway's uptime");
        assert_eq!(w.availability, None);
        assert_eq!(w.latency, None);
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
        assert_eq!(w.uptime, None);
    }

    #[test]
    fn get_not_found() {
        let err = svc(fixtures()).get_wan("nonexistent").unwrap_err();
        assert!(matches!(err, Error::NotFound(_)));
        assert_eq!(err.to_string(), "WAN not found: nonexistent");
    }

    #[test]
    fn list_item_equals_get() {
        let s = svc(fixtures());
        for w in s.list_wans().unwrap().items {
            assert_eq!(s.get_wan(&w.id).unwrap(), w, "{}", w.id);
        }
    }
}
