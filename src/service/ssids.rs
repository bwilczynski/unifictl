//! SSIDs: ported from homelab-api/internal/network/ssids_service.go.

use std::collections::HashMap;

use super::Service;
use super::common::*;
use crate::error::{Error, Result};
use crate::model::*;
use crate::unifi::fetch;
use crate::unifi::types::{NetworkConf, Sta, WlanConf};

impl Service {
    /// Enabled SSIDs only.
    pub fn list_ssids(&self) -> Result<SsidList> {
        fetch!(
            wlans = self.api.wlan_conf(),
            networks = self.api.network_conf(),
            clients = self.api.clients(),
        );
        let by_ssid = clients_by_ssid(&clients);
        let items = wlans
            .iter()
            .filter(|w| w.enabled)
            .map(|w| Ssid {
                id: to_kebab(&w.name),
                name: w.name.clone(),
                bands: map_bands(&w.wlan_bands),
                vlan_id: ssid_vlan_id(w, &networks),
                num_clients: by_ssid.get(w.name.as_str()).map_or(0, |v| v.len() as i64),
            })
            .collect();
        Ok(SsidList { items })
    }

    /// Any SSID, enabled or not (matches Go).
    pub fn get_ssid(&self, id: &str) -> Result<SsidDetail> {
        fetch!(
            wlans = self.api.wlan_conf(),
            networks = self.api.network_conf(),
            clients = self.api.clients(),
            devices = self.api.devices(),
        );
        let w = wlans
            .iter()
            .find(|w| to_kebab(&w.name) == id)
            .ok_or_else(|| Error::NotFound(format!("SSID not found: {id}")))?;

        let by_ssid = clients_by_ssid(&clients);
        let ssid_clients = by_ssid
            .get(w.name.as_str())
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let mut broadcasting_aps: Vec<DeviceRef> = devices
            .iter()
            .filter(|d| {
                d.kind == "uap" && d.state == 1 && d.vap_table.iter().any(|v| v.id == w.id && v.up)
            })
            .map(device_ref)
            .collect();
        broadcasting_aps.sort_by(|a, b| a.id.cmp(&b.id));

        Ok(SsidDetail {
            id: id.to_string(),
            name: w.name.clone(),
            bands: map_bands(&w.wlan_bands),
            vlan_id: ssid_vlan_id(w, &networks),
            num_clients: ssid_clients.len() as i64,
            clients: ssid_clients.iter().map(|s| client_ref(s)).collect(),
            broadcasting_aps,
            security_protocol: security_protocol(w),
        })
    }
}

/// Wireless clients grouped by ESSID, in controller order.
fn clients_by_ssid(clients: &[Sta]) -> HashMap<&str, Vec<&Sta>> {
    let mut m: HashMap<&str, Vec<&Sta>> = HashMap::new();
    for c in clients.iter().filter(|c| !c.is_wired) {
        if let Some(essid) = &c.essid {
            m.entry(essid.as_str()).or_default().push(c);
        }
    }
    m
}

/// VLAN of the SSID's network; 1 (native) when the network is unknown.
fn ssid_vlan_id(w: &WlanConf, networks: &[NetworkConf]) -> i64 {
    networks
        .iter()
        .find(|n| n.id == w.networkconf_id)
        .map_or(1, |n| extract_vlan_id(&n.vlan))
}

fn map_bands(bands: &[String]) -> Vec<WifiBand> {
    bands
        .iter()
        .filter_map(|b| match b.as_str() {
            "2g" => Some(WifiBand::Band2g),
            "5g" => Some(WifiBand::Band5g),
            "6g" => Some(WifiBand::Band6g),
            _ => None,
        })
        .collect()
}

fn security_protocol(w: &WlanConf) -> SecurityProtocol {
    if w.security == "open" {
        return SecurityProtocol::Open;
    }
    match w.wpa_mode.as_str() {
        "wpa2" if w.wpa3_transition => SecurityProtocol::Wpa2Wpa3,
        "wpa3" => SecurityProtocol::Wpa3,
        _ => SecurityProtocol::Wpa2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use crate::service::testutil::{fixtures, svc};
    use crate::unifi::fixtures::FixtureApi;
    use crate::unifi::types::Sta;

    fn sta(mac: &str, name: Option<&str>, essid: &str) -> Sta {
        Sta {
            mac: mac.into(),
            name: name.map(Into::into),
            essid: Some(essid.into()),
            ..Default::default()
        }
    }

    #[test]
    fn list_counts_clients_and_resolves_vlans() {
        let api = FixtureApi {
            clients: vec![
                sta("aa:bb:cc:dd:ee:01", None, "home-wifi-iot"),
                sta("aa:bb:cc:dd:ee:02", None, "home-wifi-iot"),
                sta("aa:bb:cc:dd:ee:03", None, "home-wifi"),
            ],
            ..fixtures()
        };
        let items = svc(api).list_ssids().unwrap().items;
        assert_eq!(items.len(), 2);
        let iot = items.iter().find(|s| s.id == "home-wifi-iot").unwrap();
        assert_eq!(iot.vlan_id, 20);
        assert_eq!(iot.num_clients, 2);
        assert_eq!(iot.bands, [WifiBand::Band2g, WifiBand::Band5g]);
        let home = items.iter().find(|s| s.id == "home-wifi").unwrap();
        assert_eq!(home.vlan_id, 10);
        assert_eq!(home.num_clients, 1);
    }

    #[test]
    fn list_empty_and_missing_networkconf() {
        assert!(
            svc(FixtureApi::default())
                .list_ssids()
                .unwrap()
                .items
                .is_empty()
        );
        let api = FixtureApi {
            wlans: vec![WlanConf {
                id: "cc02".into(),
                name: "orphan-ssid".into(),
                networkconf_id: "missing-id".into(),
                enabled: true,
                wlan_bands: vec!["5g".into()],
                ..Default::default()
            }],
            ..Default::default()
        };
        assert_eq!(svc(api).list_ssids().unwrap().items[0].vlan_id, 1);
    }

    #[test]
    fn get_detail() {
        let api = FixtureApi {
            clients: vec![
                sta("aa:bb:cc:dd:ee:01", Some("Device A"), "home-wifi-iot"),
                sta("aa:bb:cc:dd:ee:02", Some("Device B"), "home-wifi-iot"),
            ],
            ..fixtures()
        };
        let d = svc(api).get_ssid("home-wifi-iot").unwrap();
        assert_eq!(d.id, "home-wifi-iot");
        assert_eq!(d.vlan_id, 20);
        assert_eq!(d.num_clients, 2);
        assert_eq!(d.clients.len(), 2);
        assert_eq!(d.broadcasting_aps.len(), 2);
        assert!(d.broadcasting_aps.iter().all(|ap| ap.kind == "device"));
        let ids: Vec<&str> = d.broadcasting_aps.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["uap-01", "uap-02"], "sorted by id");
    }

    #[test]
    fn get_disabled_ssid_still_resolves() {
        let api = FixtureApi {
            wlans: vec![WlanConf {
                id: "cc01".into(),
                name: "hidden-net".into(),
                networkconf_id: "bb20".into(),
                enabled: false,
                wlan_bands: vec!["2g".into()],
                ..Default::default()
            }],
            ..Default::default()
        };
        let d = svc(api).get_ssid("hidden-net").unwrap();
        let v = serde_json::to_value(&d).unwrap();
        assert_eq!(v["broadcastingAps"], serde_json::Value::Null);
    }

    #[test]
    fn get_not_found() {
        let err = svc(fixtures()).get_ssid("nonexistent").unwrap_err();
        assert!(matches!(err, Error::NotFound(_)));
        assert_eq!(err.to_string(), "SSID not found: nonexistent");
    }

    #[test]
    fn security_protocol_mapping() {
        let w = |security: &str, wpa_mode: &str, transition: bool| WlanConf {
            security: security.into(),
            wpa_mode: wpa_mode.into(),
            wpa3_transition: transition,
            ..Default::default()
        };
        assert_eq!(
            security_protocol(&w("open", "wpa2", false)),
            SecurityProtocol::Open
        );
        assert_eq!(
            security_protocol(&w("wpapsk", "wpa2", false)),
            SecurityProtocol::Wpa2
        );
        assert_eq!(
            security_protocol(&w("wpapsk", "wpa2", true)),
            SecurityProtocol::Wpa2Wpa3
        );
        assert_eq!(
            security_protocol(&w("wpapsk", "wpa3", false)),
            SecurityProtocol::Wpa3
        );
        assert_eq!(
            security_protocol(&w("wpapsk", "", false)),
            SecurityProtocol::Wpa2
        );
    }
}
