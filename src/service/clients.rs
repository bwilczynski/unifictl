//! Clients: ported from homelab-api/internal/network/clients_service.go.

use std::collections::HashMap;

use super::Service;
use super::common::*;
use crate::error::{Error, Result};
use crate::model::*;
use crate::unifi::fetch;
use crate::unifi::types::{ClientV2, Device, Sta};

impl Service {
    /// v2 API clients; `None` → active followed by offline history.
    pub fn list_clients(&self, status: Option<ClientStatus>) -> Result<NetworkClientList> {
        let raw = match status {
            Some(ClientStatus::Online) => self.api.active_clients()?,
            Some(ClientStatus::Offline) => self.api.offline_clients(self.history_days)?,
            None => self.api.all_clients(self.history_days)?,
        };
        Ok(NetworkClientList {
            items: raw.iter().map(client_to_list_v2).collect(),
        })
    }

    /// Looks up active (v1 sta) clients first, then offline v2 history.
    /// History is fetched up front, in parallel, rather than only on a miss.
    pub fn get_client(&self, id: &str) -> Result<NetworkClientDetail> {
        fetch!(
            devices = self.api.devices(),
            stas = self.api.clients(),
            offline = self.api.offline_clients(self.history_days),
        );
        let by_mac = mac_to_device(&devices);

        if let Some(sta) = stas.iter().find(|s| client_suffix(s) == id) {
            return Ok(client_to_detail(sta, &by_mac));
        }
        if let Some(c) = offline.iter().find(|c| client_id_v2(c) == id) {
            return Ok(client_to_detail_v2(c, &by_mac));
        }
        Err(Error::NotFound(format!("Network client not found: {id}")))
    }
}

/// Go copies the controller's status string verbatim; the v2 API only ever
/// reports "online" or "offline", so anything else is treated as offline.
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

fn upstream(by_mac: &HashMap<String, &Device>, mac: &str) -> DeviceRef {
    by_mac
        .get(&normalize_mac(mac))
        .map(|d| device_ref(d))
        .unwrap_or_default()
}

fn client_to_list_v2(c: &ClientV2) -> NetworkClient {
    let ip = if c.status == "online" {
        &c.ip
    } else {
        &c.last_ip
    };
    NetworkClient {
        id: client_id_v2(c),
        name: client_name_v2(c),
        mac: normalize_mac(&c.mac),
        ip: non_empty(ip),
        connection_type: map_connection_type(c.is_wired),
        status: map_client_status(&c.status),
    }
}

fn client_to_detail(sta: &Sta, by_mac: &HashMap<String, &Device>) -> NetworkClientDetail {
    let (id, name, mac, ip) = (
        client_suffix(sta),
        client_name(sta),
        normalize_mac(&sta.mac),
        non_empty(&sta.ip),
    );
    if sta.is_wired {
        NetworkClientDetail::Wired(WiredClientDetail {
            id,
            name,
            mac,
            ip,
            connection_type: ConnectionType::Wired,
            status: ClientStatus::Online,
            connected_to: Connection {
                device: upstream(by_mac, &sta.sw_mac),
                port: (sta.sw_port > 0).then_some(sta.sw_port),
                link_speed: if sta.wired_rate_mbps > 0 {
                    map_link_speed(sta.wired_rate_mbps)
                } else {
                    None
                },
                ..Default::default()
            },
            uptime: Some(sta.uptime),
        })
    } else {
        NetworkClientDetail::Wireless(WirelessClientDetail {
            id,
            name,
            mac,
            ip,
            connection_type: ConnectionType::Wireless,
            status: ClientStatus::Online,
            connected_to: WirelessConnection {
                device: upstream(by_mac, &sta.ap_mac),
                ssid: sta.essid.clone().unwrap_or_default(),
                signal_strength: sta.signal,
            },
            uptime: Some(sta.uptime),
        })
    }
}

/// Offline client: no port, link speed, signal or uptime (no live connection).
fn client_to_detail_v2(c: &ClientV2, by_mac: &HashMap<String, &Device>) -> NetworkClientDetail {
    let (id, name, mac, ip) = (
        client_id_v2(c),
        client_name_v2(c),
        normalize_mac(&c.mac),
        non_empty(&c.last_ip),
    );
    let device = upstream(by_mac, &c.last_uplink_mac);
    if c.is_wired {
        NetworkClientDetail::Wired(WiredClientDetail {
            id,
            name,
            mac,
            ip,
            connection_type: ConnectionType::Wired,
            status: ClientStatus::Offline,
            connected_to: Connection {
                device,
                port: None,
                link_speed: None,
                ..Default::default()
            },
            uptime: None,
        })
    } else {
        NetworkClientDetail::Wireless(WirelessClientDetail {
            id,
            name,
            mac,
            ip,
            connection_type: ConnectionType::Wireless,
            status: ClientStatus::Offline,
            connected_to: WirelessConnection {
                device,
                ssid: c.essid.clone().unwrap_or_default(),
                signal_strength: None,
            },
            uptime: None,
        })
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
        assert_eq!(s.list_clients(None).unwrap().items.len(), 5);
        let online = s.list_clients(Some(ClientStatus::Online)).unwrap().items;
        assert_eq!(online.len(), 3);
        assert!(online.iter().all(|c| c.status == ClientStatus::Online));
        let offline = s.list_clients(Some(ClientStatus::Offline)).unwrap().items;
        assert_eq!(offline.len(), 2);
        assert!(offline.iter().all(|c| c.status == ClientStatus::Offline));
    }

    #[test]
    fn list_ids_and_fields() {
        let items = svc(fixtures()).list_clients(None).unwrap().items;
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
                .list_clients(None)
                .unwrap()
                .items
                .is_empty()
        );
    }

    #[test]
    fn get_wireless() {
        let NetworkClientDetail::Wireless(c) =
            svc(fixtures()).get_client("macbook-pro-0001").unwrap()
        else {
            panic!("expected wireless")
        };
        assert_eq!(c.connected_to.ssid, "homelab");
        assert_eq!(c.connected_to.signal_strength, Some(-69));
        assert_eq!(c.connected_to.device.id, "uap-01");
        assert_eq!(c.uptime, Some(27075));
        assert_eq!(c.status, ClientStatus::Online);
    }

    #[test]
    fn get_wired() {
        let NetworkClientDetail::Wired(c) = svc(fixtures()).get_client("nas-1-0002").unwrap()
        else {
            panic!("expected wired")
        };
        assert_eq!(c.connected_to.device.id, "us-8-60w");
        assert_eq!(c.connected_to.port, Some(3));
        assert_eq!(c.connected_to.link_speed, Some(LinkSpeed::Gbe1));
        assert_eq!(c.uptime, Some(1024199));
    }

    #[test]
    fn get_offline_wired() {
        let api = FixtureApi {
            clients: vec![],
            ..fixtures()
        };
        let NetworkClientDetail::Wired(c) = svc(api).get_client("host-02-bb05").unwrap() else {
            panic!("expected wired")
        };
        assert_eq!(c.status, ClientStatus::Offline);
        assert_eq!(
            c.connected_to.device,
            DeviceRef::new("switch-flex-mini".into(), "Switch Flex Mini".into())
        );
        assert_eq!(c.connected_to.port, None);
        assert_eq!(c.connected_to.link_speed, None);
        assert_eq!(c.uptime, None);
    }

    #[test]
    fn get_offline_wireless() {
        let api = FixtureApi {
            clients: vec![],
            ..fixtures()
        };
        let NetworkClientDetail::Wireless(c) =
            svc(api).get_client("kindle-paperwhite-bb04").unwrap()
        else {
            panic!("expected wireless")
        };
        assert_eq!(c.status, ClientStatus::Offline);
        assert_eq!(c.connected_to.device.id, "uap-01");
        assert_eq!(c.connected_to.ssid, "homelab");
        assert_eq!(c.connected_to.signal_strength, None);
        assert_eq!(c.ip.as_deref(), Some("192.168.10.37"));
        assert_eq!(c.uptime, None);
    }

    #[test]
    fn get_not_found() {
        let err = svc(fixtures()).get_client("nobody-00").unwrap_err();
        assert!(matches!(err, Error::NotFound(_)));
        assert_eq!(err.to_string(), "Network client not found: nobody-00");
    }

    #[test]
    fn unresolved_upstream_serializes_go_zero_device() {
        let api = FixtureApi {
            devices: vec![],
            ..fixtures()
        };
        let detail = svc(api).get_client("nas-1-0002").unwrap();
        let v = serde_json::to_value(&detail).unwrap();
        assert_eq!(
            v["connectedTo"]["device"],
            serde_json::json!({"kind": "", "id": "", "name": ""})
        );
    }
}
