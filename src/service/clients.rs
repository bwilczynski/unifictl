//! Clients: ported from homelab-api/internal/network/clients_service.go.

use super::Service;
use super::common::*;
use crate::error::{Error, Result};
use crate::model::*;
use crate::unifi::types::ClientV2;

/// Query filters for `clients list`; `None` means "do not filter".
#[derive(Debug, Clone, Default)]
pub struct ClientFilter {
    pub status: Option<ClientStatus>,
    pub connection: Option<ConnectionType>,
    /// The upstream device: access point, switch or gateway.
    pub device_id: Option<String>,
    /// As `ssids list` shows it; only wireless clients match.
    pub ssid_id: Option<String>,
    /// This machine's MACs, any case or separator; only those clients match.
    pub self_macs: Option<Vec<String>>,
}

impl ClientFilter {
    fn matches(&self, c: &NetworkClient) -> bool {
        if self.self_macs.as_ref().is_some_and(|macs| {
            !macs
                .iter()
                .any(|m| normalize_mac(&m.replace('-', ":")) == c.mac)
        }) {
            return false;
        }
        if self.connection.is_some_and(|t| c.connection_type != t) {
            return false;
        }
        let device = match &c.connected_to {
            ClientConnection::Wired(w) => &w.device,
            ClientConnection::Wireless(w) => &w.device,
        };
        if self.device_id.as_deref().is_some_and(|id| device.id != id) {
            return false;
        }
        !self
            .ssid_id
            .as_deref()
            .is_some_and(|id| match &c.connected_to {
                ClientConnection::Wireless(w) => to_kebab(&w.ssid) != id,
                ClientConnection::Wired(_) => true,
            })
    }
}

impl Service {
    /// v2 API clients; no status → active followed by offline history.
    pub fn list_clients(&self, filter: &ClientFilter) -> Result<NetworkClientList> {
        let raw = match filter.status {
            Some(ClientStatus::Online) => self.api.active_clients()?,
            Some(ClientStatus::Offline) => self.api.offline_clients(self.history_days)?,
            None => self.api.all_clients(self.history_days)?,
        };
        let mut items: Vec<_> = raw
            .iter()
            .map(network_client)
            .filter(|c| filter.matches(c))
            .collect();
        if filter.self_macs.is_some() {
            // Docked with Wi-Fi still on: the wired connection first.
            items.sort_by_key(|c| c.connection_type != ConnectionType::Wired);
        }
        Ok(NetworkClientList { items })
    }

    /// The `clients list` item with this ID (active first, then history).
    pub fn get_client(&self, id: &str) -> Result<NetworkClient> {
        self.api
            .all_clients(self.history_days)?
            .iter()
            .find(|c| client_id_v2(c) == id)
            .map(network_client)
            .ok_or_else(|| Error::NotFound(format!("Network client not found: {id}")))
    }
}

/// The v2 API only ever reports "online" or "offline"; anything else is
/// treated as offline.
fn map_client_status(status: &str) -> ClientStatus {
    if status == "online" {
        ClientStatus::Online
    } else {
        ClientStatus::Offline
    }
}

fn non_empty(s: &str) -> Option<String> {
    (!s.is_empty()).then(|| s.to_string())
}

/// The upstream device by name (its ID is the kebab-cased name), or the
/// zero device when the controller does not say.
fn upstream_ref(name: &str) -> DeviceRef {
    if name.is_empty() {
        DeviceRef::default()
    } else {
        DeviceRef::new(to_kebab(name), name.to_string())
    }
}

/// Counters are network-side (`tx` is what the network sent the client), so
/// they are swapped into the client's view.
fn client_rates(c: &ClientV2) -> Option<Rates> {
    if c.rx_bytes_r.is_none() && c.tx_bytes_r.is_none() {
        return None;
    }
    Some(Rates {
        rx_bytes_per_sec: c.tx_bytes_r.unwrap_or(0.0) as i64,
        tx_bytes_per_sec: c.rx_bytes_r.unwrap_or(0.0) as i64,
    })
}

/// Live fields (experience, signal, rates, link speed, uptime) only while
/// online: history keeps stale values.
pub(super) fn network_client(c: &ClientV2) -> NetworkClient {
    let online = c.status == "online";
    let device = upstream_ref(&c.last_uplink_name);
    let connected_to = if c.is_wired {
        ClientConnection::Wired(Connection {
            device,
            port: c.sw_port.or(c.last_uplink_remote_port).filter(|p| *p > 0),
            link_speed: c
                .wired_rate_mbps
                .filter(|_| online)
                .and_then(map_link_speed),
            ..Default::default()
        })
    } else {
        ClientConnection::Wireless(WirelessConnection {
            device,
            ssid: c.essid.clone().unwrap_or_default(),
            signal_strength: c.signal.filter(|_| online),
            rate_mbps: c
                .tx_rate
                .filter(|r| online && *r > 0.0)
                .map(|kbps| (kbps / 1000.0) as i64),
        })
    };
    NetworkClient {
        id: client_id_v2(c),
        name: client_name_v2(c),
        mac: normalize_mac(&c.mac),
        ip: non_empty(if online { &c.ip } else { &c.last_ip }),
        connection_type: map_connection_type(c.is_wired),
        status: map_client_status(&c.status),
        connected_to,
        experience: c
            .wifi_experience_score
            .filter(|s| online && *s >= 0.0)
            .map(|s| s.round() as i64),
        traffic: if online { client_rates(c) } else { None },
        uptime: online.then_some(c.uptime),
        last_seen: (c.last_seen > 0).then_some(c.last_seen),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use crate::service::testutil::{fixtures, svc};
    use crate::unifi::fixtures::FixtureApi;

    #[test]
    fn list_all_online_offline() {
        let s = svc(fixtures());
        assert_eq!(
            s.list_clients(&ClientFilter::default())
                .unwrap()
                .items
                .len(),
            5
        );
        let status = |status| ClientFilter {
            status: Some(status),
            ..Default::default()
        };
        let online = s.list_clients(&status(ClientStatus::Online)).unwrap().items;
        assert_eq!(online.len(), 3);
        assert!(online.iter().all(|c| c.status == ClientStatus::Online));
        let offline = s
            .list_clients(&status(ClientStatus::Offline))
            .unwrap()
            .items;
        assert_eq!(offline.len(), 2);
        assert!(offline.iter().all(|c| c.status == ClientStatus::Offline));
    }

    #[test]
    fn list_ids_and_fields() {
        let items = svc(fixtures())
            .list_clients(&ClientFilter::default())
            .unwrap()
            .items;
        let by_id = |id: &str| {
            items
                .iter()
                .find(|c| c.id == id)
                .unwrap_or_else(|| panic!("{id}"))
        };
        let mb = by_id("macbook-pro-0001");
        assert_eq!(mb.name, "MacBook Pro");
        assert_eq!(mb.connection_type, ConnectionType::Wireless);
        assert_eq!(mb.ip.as_deref(), Some("192.168.10.67"));
        assert_eq!(by_id("nas-1-0002").connection_type, ConnectionType::Wired);
        let kindle = by_id("kindle-paperwhite-bb04");
        assert_eq!(kindle.status, ClientStatus::Offline);
        assert_eq!(kindle.ip.as_deref(), Some("192.168.10.37"));
        assert_eq!(by_id("host-02-bb05").connection_type, ConnectionType::Wired);
    }

    #[test]
    fn list_empty() {
        assert!(
            svc(FixtureApi::default())
                .list_clients(&ClientFilter::default())
                .unwrap()
                .items
                .is_empty()
        );
    }

    #[test]
    fn get_not_found() {
        let err = svc(fixtures()).get_client("nobody-00").unwrap_err();
        assert!(matches!(err, Error::NotFound(_)));
        assert_eq!(err.to_string(), "Network client not found: nobody-00");
    }
    fn wired(c: &NetworkClient) -> &Connection {
        match &c.connected_to {
            ClientConnection::Wired(w) => w,
            ClientConnection::Wireless(_) => panic!("expected wired: {}", c.id),
        }
    }

    fn wireless(c: &NetworkClient) -> &WirelessConnection {
        match &c.connected_to {
            ClientConnection::Wireless(w) => w,
            ClientConnection::Wired(_) => panic!("expected wireless: {}", c.id),
        }
    }

    #[test]
    fn get_wireless() {
        let c = svc(fixtures()).get_client("macbook-pro-0001").unwrap();
        let w = wireless(&c);
        assert_eq!(w.device, DeviceRef::new("uap-01".into(), "UAP-01".into()));
        assert_eq!(w.ssid, "homelab");
        assert_eq!(w.signal_strength, Some(-69));
        assert_eq!(w.rate_mbps, Some(866));
        assert_eq!(c.experience, Some(92));
        assert_eq!(
            c.traffic,
            Some(Rates {
                rx_bytes_per_sec: 524288,
                tx_bytes_per_sec: 10240
            }),
            "client download is the network-side tx"
        );
        assert_eq!(c.uptime, Some(27075));
        assert_eq!(c.last_seen, Some(1791712800));
        assert_eq!(c.status, ClientStatus::Online);
    }

    #[test]
    fn get_wired() {
        let c = svc(fixtures()).get_client("nas-1-0002").unwrap();
        let w = wired(&c);
        assert_eq!(w.device.id, "us-8-60w");
        assert_eq!(w.port, Some(3));
        assert_eq!(w.link_speed, Some(LinkSpeed::Gbe1));
        assert_eq!(w.max_link_speed, None);
        assert_eq!(c.experience, None, "wired clients have no score");
        assert_eq!(c.uptime, Some(1024199));
    }

    #[test]
    fn offline_client_has_no_live_fields() {
        let c = svc(fixtures()).get_client("host-02-bb05").unwrap();
        assert_eq!(c.status, ClientStatus::Offline);
        let w = wired(&c);
        assert_eq!(
            w.device,
            DeviceRef::new("switch-flex-mini".into(), "Switch Flex Mini".into())
        );
        assert_eq!(w.port, Some(4), "last known port is reported");
        assert_eq!(w.link_speed, None, "stale history speed is not live");
        assert_eq!((c.traffic, c.uptime, c.experience), (None, None, None));
        assert_eq!(c.last_seen, Some(1778829796));
        assert_eq!(c.ip.as_deref(), Some("192.168.10.42"));

        let k = svc(fixtures())
            .get_client("kindle-paperwhite-bb04")
            .unwrap();
        assert_eq!(wireless(&k).ssid, "homelab");
        assert_eq!(wireless(&k).signal_strength, None);
        assert_eq!(wireless(&k).rate_mbps, None);
    }

    #[test]
    fn get_equals_list_item() {
        let s = svc(fixtures());
        for c in s.list_clients(&ClientFilter::default()).unwrap().items {
            assert_eq!(s.get_client(&c.id).unwrap(), c, "{}", c.id);
        }
    }

    #[test]
    fn unknown_upstream_serializes_zero_device() {
        let mut api = fixtures();
        for c in &mut api.active {
            c.last_uplink_name.clear();
        }
        let c = svc(api).get_client("nas-1-0002").unwrap();
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(
            v["connectedTo"]["device"],
            serde_json::json!({"kind": "", "id": "", "name": ""})
        );
    }

    #[test]
    fn json_shape_is_additive() {
        let c = svc(fixtures()).get_client("macbook-pro-0001").unwrap();
        let v = serde_json::to_value(&c).unwrap();
        for key in [
            "id",
            "name",
            "mac",
            "ip",
            "connectionType",
            "status",
            "connectedTo",
            "uptime",
        ] {
            assert!(v.get(key).is_some(), "existing key {key} kept");
        }
        assert_eq!(v["connectedTo"]["signalStrength"], -69);
        assert_eq!(v["connectedTo"]["rateMbps"], 866);
        assert_eq!(v["experience"], 92);
        assert_eq!(v["traffic"]["rxBytesPerSec"], 524288);
        let off =
            serde_json::to_value(svc(fixtures()).get_client("host-02-bb05").unwrap()).unwrap();
        assert!(off.get("traffic").is_none() && off.get("uptime").is_none());
    }

    fn self_filter(macs: &[&str]) -> ClientFilter {
        ClientFilter {
            self_macs: Some(macs.iter().map(|m| m.to_string()).collect()),
            ..Default::default()
        }
    }

    fn ids(s: &Service, f: &ClientFilter) -> Vec<String> {
        s.list_clients(f)
            .unwrap()
            .items
            .into_iter()
            .map(|c| c.id)
            .collect()
    }

    #[test]
    fn self_lists_wired_before_wireless() {
        // MacBook Pro (wireless) comes first in the fixtures, nas-1 (wired) second.
        let s = svc(fixtures());
        assert_eq!(
            ids(
                &s,
                &self_filter(&["3c:ee:ee:00:00:01", "68:ee:ee:00:00:02"])
            ),
            ["nas-1-0002", "macbook-pro-0001"]
        );
    }

    #[test]
    fn self_matches_any_mac_spelling() {
        let s = svc(fixtures());
        assert_eq!(
            ids(&s, &self_filter(&["68-EE-EE-00-00-02"])),
            ["nas-1-0002"]
        );
    }

    #[test]
    fn self_with_no_match_is_empty() {
        let s = svc(fixtures());
        assert!(ids(&s, &self_filter(&["02:00:00:00:00:99"])).is_empty());
        assert!(ids(&s, &self_filter(&[])).is_empty());
    }

    #[test]
    fn self_does_not_match_a_shared_suffix() {
        // Same last four hex digits as nas-1 (68:ee:ee:00:00:02).
        assert!(ids(&svc(fixtures()), &self_filter(&["02:00:00:00:00:02"])).is_empty());
    }
}
