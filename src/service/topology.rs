//! Topology: ported from homelab-api/internal/network/topology_service.go.

use std::collections::HashMap;

use super::Service;
use super::common::*;
use crate::error::Result;
use crate::model::*;
use crate::unifi::types::{ClientV2, Device, Sta};

impl Service {
    /// Pass 1: device nodes + uplink edges. With `include_clients`: online
    /// clients (v1 stas) and offline clients (v2 history) with their edges.
    pub fn get_topology(&self, include_clients: bool) -> Result<NetworkTopology> {
        let devices = self.api.devices()?;
        let by_mac = mac_to_device(&devices);
        let mut nodes = Vec::new();
        let mut edges = Vec::new();

        for d in &devices {
            nodes.push(TopologyNode::Device(TopologyDeviceNode {
                kind: "device",
                id: to_kebab(&d.name),
                name: d.name.clone(),
                device_type: map_device_type(&d.kind),
                status: map_device_status(d.state),
                num_clients: (d.kind == "uap").then_some(d.user_num_sta + d.guest_num_sta),
            }));
            let Some(up) = d.uplink.as_ref().filter(|u| !u.uplink_mac.is_empty()) else {
                continue;
            };
            if let Some(upstream) = by_mac.get(&normalize_mac(&up.uplink_mac)) {
                edges.push(TopologyEdge::Wired(TopologyWiredEdge {
                    kind: "wired",
                    source: ConnectionRef::Device(device_ref(d)),
                    target: device_ref(upstream),
                    port: up.uplink_remote_port,
                    link_speed: if up.speed > 0 {
                        map_link_speed(up.speed)
                    } else {
                        None
                    },
                }));
            }
        }

        if include_clients {
            let stas = self.api.clients()?;
            let offline = self.api.offline_clients(self.history_days)?;
            for sta in &stas {
                let r = client_ref(sta);
                nodes.push(client_node(
                    r.id,
                    r.name,
                    sta.is_wired,
                    ClientStatus::Online,
                ));
                edges.extend(online_client_edge(sta, &by_mac));
            }
            for c in &offline {
                let r = client_ref_v2(c);
                nodes.push(client_node(r.id, r.name, c.is_wired, ClientStatus::Offline));
                edges.extend(offline_client_edge(c, &by_mac));
            }
        }

        Ok(NetworkTopology { nodes, edges })
    }
}

fn client_node(id: String, name: String, is_wired: bool, status: ClientStatus) -> TopologyNode {
    TopologyNode::Client(TopologyClientNode {
        kind: "client",
        id,
        name,
        connection_type: map_connection_type(is_wired),
        status,
    })
}

/// `None` when the upstream device cannot be resolved.
fn online_client_edge(sta: &Sta, by_mac: &HashMap<String, &Device>) -> Option<TopologyEdge> {
    if sta.is_wired {
        if sta.sw_mac.is_empty() {
            return None;
        }
        let upstream = by_mac.get(&normalize_mac(&sta.sw_mac))?;
        return Some(TopologyEdge::Wired(TopologyWiredEdge {
            kind: "wired",
            source: ConnectionRef::Client(client_ref(sta)),
            target: device_ref(upstream),
            port: (sta.sw_port > 0).then_some(sta.sw_port),
            link_speed: if sta.wired_rate_mbps > 0 {
                map_link_speed(sta.wired_rate_mbps)
            } else {
                None
            },
        }));
    }
    let ssid = sta.essid.clone()?;
    if sta.ap_mac.is_empty() {
        return None;
    }
    let upstream = by_mac.get(&normalize_mac(&sta.ap_mac))?;
    Some(TopologyEdge::Wireless(TopologyWirelessEdge {
        kind: "wireless",
        source: client_ref(sta),
        target: device_ref(upstream),
        ssid,
        signal_strength: sta.signal,
    }))
}

/// Offline edges never carry port or signal (no live connection).
fn offline_client_edge(c: &ClientV2, by_mac: &HashMap<String, &Device>) -> Option<TopologyEdge> {
    if c.last_uplink_mac.is_empty() {
        return None;
    }
    let target = device_ref(by_mac.get(&normalize_mac(&c.last_uplink_mac))?);
    if c.is_wired {
        return Some(TopologyEdge::Wired(TopologyWiredEdge {
            kind: "wired",
            source: ConnectionRef::Client(client_ref_v2(c)),
            target,
            port: None,
            link_speed: None,
        }));
    }
    Some(TopologyEdge::Wireless(TopologyWirelessEdge {
        kind: "wireless",
        source: client_ref_v2(c),
        target,
        ssid: c.essid.clone()?,
        signal_strength: None,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::testutil::{fixtures, svc};

    fn wired<'a>(t: &'a NetworkTopology, source: &str) -> &'a TopologyWiredEdge {
        t.edges
            .iter()
            .find_map(|e| match e {
                TopologyEdge::Wired(w) if w.source.id() == source => Some(w),
                _ => None,
            })
            .unwrap_or_else(|| panic!("wired edge from {source}"))
    }

    fn wireless<'a>(t: &'a NetworkTopology, source: &str) -> &'a TopologyWirelessEdge {
        t.edges
            .iter()
            .find_map(|e| match e {
                TopologyEdge::Wireless(w) if w.source.id == source => Some(w),
                _ => None,
            })
            .unwrap_or_else(|| panic!("wireless edge from {source}"))
    }

    #[test]
    fn devices_only() {
        let t = svc(fixtures()).get_topology(false).unwrap();
        assert_eq!(t.nodes.len(), 7);
        assert_eq!(t.edges.len(), 6);
        let devices: Vec<&TopologyDeviceNode> = t
            .nodes
            .iter()
            .map(|n| match n {
                TopologyNode::Device(d) => d,
                TopologyNode::Client(c) => panic!("unexpected client {}", c.id),
            })
            .collect();
        assert_eq!(
            devices
                .iter()
                .filter(|d| d.device_type == DeviceType::Gateway)
                .count(),
            1
        );
        for d in &devices {
            match d.id.as_str() {
                "uap-01" | "uap-02" => assert_eq!(d.num_clients, Some(6), "{}", d.id),
                _ => assert_eq!(d.num_clients, None, "{}", d.id),
            }
        }
        assert!(t.edges.iter().all(|e| match e {
            TopologyEdge::Wired(w) => w.source.id() != "cgf-01",
            TopologyEdge::Wireless(_) => false,
        }));

        let uap = wired(&t, "uap-01");
        assert_eq!(uap.target.id, "us-8-60w");
        assert_eq!(uap.port, Some(5));
        assert_eq!(uap.link_speed, Some(LinkSpeed::Fe));
        assert_eq!(wired(&t, "switch-flex-mini").port, Some(6));
        let us8 = wired(&t, "us-8-60w");
        assert_eq!(us8.target.id, "cgf-01");
        assert_eq!(us8.port, Some(1));
        assert_eq!(us8.link_speed, Some(LinkSpeed::Gbe1));
    }

    #[test]
    fn with_clients() {
        let t = svc(fixtures()).get_topology(true).unwrap();
        assert_eq!(t.nodes.len(), 14, "7 devices + 5 online + 2 offline");
        assert_eq!(t.edges.len(), 13, "6 device + 5 online + 2 offline");

        let nas = wired(&t, "nas-1-68");
        assert_eq!(nas.target.id, "us-8-60w");
        assert_eq!(nas.port, Some(3));
        assert_eq!(nas.link_speed, Some(LinkSpeed::Gbe1));

        let ec = wired(&t, "ec-ee-ee-00-00-04-ec");
        assert_eq!(ec.target.id, "uap-01");
        assert_eq!(ec.port, Some(6));
        assert_eq!(ec.link_speed, Some(LinkSpeed::Fe));

        let mb = wireless(&t, "macbook-pro-3c");
        assert_eq!(mb.target.id, "uap-01");
        assert_eq!(mb.ssid, "homelab");
        assert_eq!(mb.signal_strength, Some(-69));

        let kindle = wireless(&t, "kindle-paperwhite-e0");
        assert_eq!(kindle.ssid, "homelab");
        assert_eq!(kindle.signal_strength, None);

        let host = wired(&t, "host-02-aa");
        assert_eq!(host.target.id, "switch-flex-mini");
        assert_eq!(host.port, None);

        for n in &t.nodes {
            if let TopologyNode::Client(c) = n {
                let want = match c.id.as_str() {
                    "kindle-paperwhite-e0" | "host-02-aa" => ClientStatus::Offline,
                    _ => ClientStatus::Online,
                };
                assert_eq!(c.status, want, "{}", c.id);
            }
        }
    }

    #[test]
    fn node_and_edge_json_shape() {
        let t = svc(fixtures()).get_topology(true).unwrap();
        let v = serde_json::to_value(&t).unwrap();
        let first = &v["nodes"][0];
        assert_eq!(first["kind"], "device");
        assert!(first.get("type").is_some());
        let edge = v["edges"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["kind"] == "wireless")
            .unwrap();
        assert_eq!(edge["source"]["kind"], "client");
        assert_eq!(edge["target"]["kind"], "device");
    }
}
