//! Devices: ported from homelab-api/internal/network/devices_service.go.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use super::Service;
use super::common::*;
use crate::error::{Error, Result};
use crate::model::*;
use crate::unifi::fetch;
use crate::unifi::types::{Device, NetworkConf, PortEntry, Sta};

/// `(switch MAC, port index)`.
pub(crate) type PortKey = (String, i64);

/// Lookups shared by device detail and the flat port listing.
pub(crate) struct PortIndex<'a> {
    pub(crate) by_mac: HashMap<String, &'a Device>,
    /// Both directions of every uplink: (upstream MAC, remote port) → device,
    /// and (device MAC, its local uplink port) → upstream device.
    pub(crate) sw_port_to_device: HashMap<PortKey, &'a Device>,
    pub(crate) sw_port_to_client: HashMap<PortKey, &'a Sta>,
    pub(crate) ap_mac_to_clients: HashMap<String, Vec<&'a Sta>>,
}

impl<'a> PortIndex<'a> {
    pub(crate) fn new(devices: &'a [Device], clients: &'a [Sta]) -> Self {
        let by_mac = mac_to_device(devices);

        let mut sw_port_to_device = HashMap::new();
        for d in devices {
            let Some(up) = d.uplink.as_ref().filter(|u| !u.uplink_mac.is_empty()) else {
                continue;
            };
            let upstream_mac = normalize_mac(&up.uplink_mac);
            if let Some(remote) = up.uplink_remote_port {
                sw_port_to_device.insert((upstream_mac.clone(), remote), d);
            }
            if let (Some(local), Some(upstream)) = (up.port_idx, by_mac.get(&upstream_mac)) {
                sw_port_to_device.insert((normalize_mac(&d.mac), local), *upstream);
            }
        }

        let mut sw_port_to_client = HashMap::new();
        let mut ap_mac_to_clients: HashMap<String, Vec<&Sta>> = HashMap::new();
        for c in clients {
            if c.is_wired && !c.sw_mac.is_empty() && c.sw_port != 0 {
                sw_port_to_client.insert((normalize_mac(&c.sw_mac), c.sw_port), c);
            }
            if !c.is_wired && !c.ap_mac.is_empty() {
                ap_mac_to_clients
                    .entry(normalize_mac(&c.ap_mac))
                    .or_default()
                    .push(c);
            }
        }

        Self {
            by_mac,
            sw_port_to_device,
            sw_port_to_client,
            ap_mac_to_clients,
        }
    }
}

impl Service {
    pub fn list_devices(&self) -> Result<NetworkDeviceList> {
        let devices = self.api.devices()?;
        let by_mac = mac_to_device(&devices);
        let items = devices.iter().map(|d| network_device(d, &by_mac)).collect();
        Ok(NetworkDeviceList { items })
    }

    pub fn get_device(&self, id: &str) -> Result<NetworkDeviceDetail> {
        fetch!(
            devices = self.api.devices(),
            clients = self.api.clients(),
            confs = self.api.network_conf(),
        );
        let idx = PortIndex::new(&devices, &clients);
        let d = devices
            .iter()
            .find(|d| to_kebab(&d.name) == id)
            .ok_or_else(|| Error::NotFound(format!("Network device not found: {id}")))?;
        Ok(build_device_detail(d, &idx, &confs))
    }
}

fn build_device_detail(d: &Device, idx: &PortIndex, confs: &[NetworkConf]) -> NetworkDeviceDetail {
    match d.kind.as_str() {
        "usw" => NetworkDeviceDetail::Switch(SwitchDetail {
            base: network_device(d, &idx.by_mac),
            ports: build_device_ports(d, &d.port_table, idx, confs),
        }),
        "uap" => NetworkDeviceDetail::AccessPoint(build_ap_detail(d, idx)),
        t if is_gateway_type(t) => NetworkDeviceDetail::Gateway(GatewayDetail {
            base: network_device(d, &idx.by_mac),
            ports: build_gateway_ports(d, idx, confs),
            wans: gateway_wan_refs(confs),
        }),
        _ => NetworkDeviceDetail::Unknown(network_device(d, &idx.by_mac)),
    }
}

/// The fields every device view shares. The gateway has no upstream device.
fn network_device(d: &Device, by_mac: &HashMap<String, &Device>) -> NetworkDevice {
    let device_type = map_device_type(&d.kind);
    let status = map_device_status(d.state);
    let uplink = if device_type == DeviceType::Gateway {
        None
    } else {
        device_uplink(d, by_mac)
    };
    NetworkDevice {
        id: to_kebab(&d.name),
        name: d.name.clone(),
        mac: normalize_mac(&d.mac),
        ip: d.ip.clone(),
        device_type,
        status,
        model: d.model.clone(),
        firmware_version: d.version.clone(),
        uptime: d.uptime,
        // A disconnected device's last score is stale.
        experience: d
            .satisfaction
            .filter(|s| *s >= 0 && status == DeviceStatus::Connected),
        traffic: device_traffic(d),
        uplink,
    }
}

fn build_ap_detail(d: &Device, idx: &PortIndex) -> AccessPointDetail {
    let stas = idx
        .ap_mac_to_clients
        .get(&normalize_mac(&d.mac))
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let connected_clients: Vec<ApClient> = stas
        .iter()
        .map(|sta| ApClient {
            client: client_ref(sta),
            ssid: sta.essid.clone().unwrap_or_default(),
            signal_strength: sta.signal.unwrap_or(0),
        })
        .collect();
    AccessPointDetail {
        base: network_device(d, &idx.by_mac),
        num_clients: connected_clients.len() as i64,
        connected_clients,
    }
}

/// LAN ports of a gateway: every port whose ifname is not wan1/wan2.
pub(crate) fn build_gateway_ports(
    d: &Device,
    idx: &PortIndex,
    confs: &[NetworkConf],
) -> Vec<DevicePort> {
    let wan: HashSet<&str> = [&d.wan1, &d.wan2]
        .into_iter()
        .flatten()
        .filter(|w| !w.name.is_empty())
        .map(|w| w.name.as_str())
        .collect();
    let lan: Vec<PortEntry> = d
        .port_table
        .iter()
        .filter(|p| !wan.contains(p.ifname.as_str()))
        .cloned()
        .collect();
    build_device_ports(d, &lan, idx, confs)
}

fn gateway_wan_refs(confs: &[NetworkConf]) -> Vec<WanRef> {
    confs
        .iter()
        .filter(|n| n.purpose == "wan")
        .map(|n| WanRef {
            id: to_kebab(&n.name),
            name: n.name.clone(),
        })
        .collect()
}

pub(crate) fn build_device_ports(
    d: &Device,
    entries: &[PortEntry],
    idx: &PortIndex,
    confs: &[NetworkConf],
) -> Vec<DevicePort> {
    let conf_by_id: HashMap<&str, &NetworkConf> =
        confs.iter().map(|c| (c.id.as_str(), c)).collect();
    let default_net_id = find_default_net_id(confs);
    // Pass 1: ports that LAG members point at are masters.
    let masters: HashSet<i64> = entries
        .iter()
        .filter_map(|p| p.aggregated_by.as_f64())
        .map(|v| v as i64)
        .collect();
    let switch_mac = normalize_mac(&d.mac);

    entries
        .iter()
        .map(|p| DevicePort {
            number: p.port_idx,
            label: port_label(p),
            state: map_port_state(p.up),
            link_speed: if p.up && p.speed > 0 {
                map_link_speed(p.speed)
            } else {
                None
            },
            link_uptime: link_uptime(p),
            sfp_module_present: p.sfp_found,
            poe_mode: p.port_poe.then(|| map_poe_mode(&p.poe_mode)),
            poe_power_watts: if p.port_poe && !p.poe_power.is_empty() {
                p.poe_power.parse().ok()
            } else {
                None
            },
            lag_membership: lag_membership(p, &masters),
            vlan_config: build_vlan_config(p, &conf_by_id, confs, default_net_id),
            traffic: Traffic {
                rx_bytes_total: p.rx_bytes,
                tx_bytes_total: p.tx_bytes,
                rx_bytes_per_sec: p.rx_bytes_r as i64,
                tx_bytes_per_sec: p.tx_bytes_r as i64,
            },
            connected_to: resolve_connected_to(&switch_mac, p.port_idx, idx),
        })
        .collect()
}

fn conf_to_vlan_ref(c: &NetworkConf) -> VlanRef {
    VlanRef {
        id: to_kebab(&c.name),
        name: c.name.clone(),
        vlan_id: extract_vlan_id(&c.vlan),
    }
}

/// `None` when the port is disabled or its native VLAN cannot be resolved.
fn build_vlan_config(
    p: &PortEntry,
    conf_by_id: &HashMap<&str, &NetworkConf>,
    confs: &[NetworkConf],
    default_net_id: Option<&str>,
) -> Option<VlanConfig> {
    let mode = match p.forward.as_str() {
        "native" => VlanMode::Access,
        "all" | "customize" => VlanMode::Trunk,
        _ => return None,
    };
    let native = resolve_native_conf(
        p.native_networkconf_id.as_deref(),
        default_net_id,
        conf_by_id,
    )?;
    let tagged_vlans = match mode {
        VlanMode::Access => None,
        VlanMode::Trunk if p.forward == "customize" && !p.excluded_networkconf_ids.is_empty() => {
            // trunk-custom: all corporate VLANs minus excluded and minus native.
            let excluded: HashSet<&str> = p
                .excluded_networkconf_ids
                .iter()
                .map(String::as_str)
                .collect();
            let mut items: Vec<VlanRef> = confs
                .iter()
                .filter(|c| {
                    c.purpose == "corporate"
                        && !excluded.contains(c.id.as_str())
                        && c.id != native.id
                })
                .map(conf_to_vlan_ref)
                .collect();
            items.sort_by_key(|v| v.vlan_id);
            Some(TaggedVlans {
                items: Some(items),
                scope: TaggedScope::Custom,
            })
        }
        VlanMode::Trunk => Some(TaggedVlans {
            items: None,
            scope: TaggedScope::All,
        }),
    };
    Some(VlanConfig {
        mode,
        native_vlan: conf_to_vlan_ref(native),
        tagged_vlans,
    })
}

/// The port's native network, falling back to the default untagged network.
fn resolve_native_conf<'a>(
    native_id: Option<&str>,
    default_net_id: Option<&str>,
    conf_by_id: &HashMap<&str, &'a NetworkConf>,
) -> Option<&'a NetworkConf> {
    let id = native_id.filter(|s| !s.is_empty()).or(default_net_id)?;
    conf_by_id.get(id).copied()
}

/// `_id` of the default (untagged, corporate) network.
fn find_default_net_id(confs: &[NetworkConf]) -> Option<&str> {
    confs
        .iter()
        .find(|c| c.purpose == "corporate" && !c.vlan_enabled)
        .map(|c| c.id.as_str())
}

fn port_label(p: &PortEntry) -> Option<String> {
    (p.name != format!("Port {}", p.port_idx)).then(|| p.name.clone())
}

fn link_uptime(p: &PortEntry) -> Option<i64> {
    if p.up { p.uptime } else { None }
}

fn lag_membership(p: &PortEntry, masters: &HashSet<i64>) -> Option<LagMembership> {
    match &p.aggregated_by {
        Value::Number(n) => n.as_f64().map(|v| LagMembership {
            id: v as i64,
            role: LagRole::Member,
        }),
        Value::Bool(false) if masters.contains(&p.port_idx) => Some(LagMembership {
            id: p.port_idx,
            role: LagRole::Master,
        }),
        _ => None,
    }
}

fn resolve_connected_to(switch_mac: &str, port_idx: i64, idx: &PortIndex) -> Option<ConnectionRef> {
    let key = (switch_mac.to_string(), port_idx);
    if let Some(dev) = idx.sw_port_to_device.get(&key) {
        return Some(ConnectionRef::Device(device_ref(dev)));
    }
    idx.sw_port_to_client
        .get(&key)
        .map(|sta| ConnectionRef::Client(client_ref(sta)))
}

fn device_traffic(d: &Device) -> Traffic {
    if is_gateway_type(&d.kind) {
        return d
            .wan1
            .as_ref()
            .map(|w| Traffic {
                rx_bytes_total: w.rx_bytes,
                tx_bytes_total: w.tx_bytes,
                rx_bytes_per_sec: w.rx_bytes_r as i64,
                tx_bytes_per_sec: w.tx_bytes_r as i64,
            })
            .unwrap_or_default();
    }
    let (rx, tx) = d
        .uplink
        .as_ref()
        .map(|u| (u.rx_bytes_r, u.tx_bytes_r))
        .unwrap_or((0.0, 0.0));
    Traffic {
        rx_bytes_total: d.rx_bytes,
        tx_bytes_total: d.tx_bytes,
        rx_bytes_per_sec: rx as i64,
        tx_bytes_per_sec: tx as i64,
    }
}

fn device_uplink(d: &Device, by_mac: &HashMap<String, &Device>) -> Option<Connection> {
    let up = d.uplink.as_ref().filter(|u| !u.uplink_mac.is_empty())?;
    let upstream = by_mac.get(&normalize_mac(&up.uplink_mac))?;
    let remote_max = up
        .uplink_remote_port
        .and_then(|port| upstream.port_table.iter().find(|p| p.port_idx == port))
        .and_then(|p| map_port_media(&p.media));
    let own_max = up.max_speed.and_then(map_link_speed);
    Some(Connection {
        device: device_ref(upstream),
        port: up.uplink_remote_port,
        link_speed: if up.speed > 0 {
            map_link_speed(up.speed)
        } else {
            None
        },
        max_link_speed: effective_max(own_max, remote_max),
        full_duplex: up.full_duplex,
    })
}

/// The slower of the two ports' maxima, or whichever one is known.
fn effective_max(a: Option<LinkSpeed>, b: Option<LinkSpeed>) -> Option<LinkSpeed> {
    match (a, b) {
        (Some(a), Some(b)) => Some(if a.mbps() <= b.mbps() { a } else { b }),
        (a, b) => a.or(b),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::error::Error;
    use crate::service::testutil::{fixtures, svc};
    use crate::unifi::fixtures::FixtureApi;

    fn confs() -> Vec<NetworkConf> {
        let mk = |id: &str, name: &str, vlan_enabled: bool, vlan: serde_json::Value| NetworkConf {
            id: id.into(),
            name: name.into(),
            purpose: "corporate".into(),
            vlan_enabled,
            vlan,
            ..Default::default()
        };
        vec![
            mk("id-mgmt", "LAN-MGMT", false, json!("")),
            mk("id-iot", "LAN-IOT", true, json!(20)),
            mk("id-int", "LAN-INT", true, json!(10)),
            mk("id-srv", "LAN-SRV", true, json!(100)),
            NetworkConf {
                id: "id-wan".into(),
                name: "WAN".into(),
                purpose: "wan".into(),
                ..Default::default()
            },
        ]
    }

    fn vlan_config(p: PortEntry) -> Option<VlanConfig> {
        let confs = confs();
        let by_id: HashMap<&str, &NetworkConf> = confs.iter().map(|c| (c.id.as_str(), c)).collect();
        build_vlan_config(&p, &by_id, &confs, Some("id-mgmt"))
    }

    fn switch_ports(id: &str) -> Vec<DevicePort> {
        match svc(fixtures()).get_device(id).unwrap() {
            NetworkDeviceDetail::Switch(sw) => sw.ports,
            other => panic!("expected switch, got {other:?}"),
        }
    }

    fn port(ports: &[DevicePort], number: i64) -> &DevicePort {
        ports
            .iter()
            .find(|p| p.number == number)
            .unwrap_or_else(|| panic!("port {number}"))
    }

    // ---- list ----

    #[test]
    fn list_devices() {
        let list = svc(fixtures()).list_devices().unwrap();
        assert_eq!(list.items.len(), 7);
        let gw = &list.items[6];
        assert_eq!(gw.id, "cgf-01");
        assert_eq!(gw.device_type, DeviceType::Gateway);
        assert_eq!(gw.status, DeviceStatus::Connected);
        assert_eq!(list.items[5].id, "us-8-60w");
        assert_eq!(list.items[5].device_type, DeviceType::Switch);
        assert_eq!(list.items[3].id, "uap-01");
        assert_eq!(list.items[3].device_type, DeviceType::AccessPoint);
    }

    #[test]
    fn list_devices_empty() {
        let list = svc(FixtureApi::default()).list_devices().unwrap();
        assert_eq!(serde_json::to_value(&list).unwrap(), json!({"items": []}));
    }

    #[test]
    fn list_devices_propagates_controller_errors() {
        let api = FixtureApi {
            fail: true,
            ..Default::default()
        };
        assert!(matches!(svc(api).list_devices(), Err(Error::Unifi(_))));
    }

    // ---- detail ----

    #[test]
    fn get_device_gateway() {
        let NetworkDeviceDetail::Gateway(gw) = svc(fixtures()).get_device("cgf-01").unwrap() else {
            panic!("expected gateway")
        };
        assert_eq!(gw.base.id, "cgf-01");
        assert_eq!(gw.base.model, "UDMA6A8");
        assert_eq!(gw.base.firmware_version, "5.1.19.33549");
        assert_eq!(gw.base.uptime, 5627);
        assert!(gw.base.uplink.is_none());
        assert_eq!(gw.base.traffic.tx_bytes_total, 37143304);
        assert_eq!(gw.base.traffic.rx_bytes_per_sec, 946);
    }

    #[test]
    fn get_device_gateway_ports_and_wans() {
        let NetworkDeviceDetail::Gateway(gw) = svc(fixtures()).get_device("cgf-01").unwrap() else {
            panic!("expected gateway")
        };
        assert_eq!(gw.ports.len(), 5, "7 ports minus WAN eth4/eth6");
        assert!(port(&gw.ports, 4).poe_mode.is_some());
        assert!(port(&gw.ports, 1).poe_mode.is_none());
        assert!(gw.ports.iter().all(|p| p.number != 5 && p.number != 7));
        assert_eq!(
            gw.wans,
            vec![
                WanRef {
                    id: "internet-1".into(),
                    name: "Internet 1".into()
                },
                WanRef {
                    id: "internet-2".into(),
                    name: "Internet 2".into()
                },
            ]
        );
    }

    #[test]
    fn get_device_unknown() {
        let api = FixtureApi {
            devices: vec![Device {
                id: "x".into(),
                mac: "BB:BB:BB:BB:BB:01".into(),
                name: "Weird Box".into(),
                model: "WB1".into(),
                kind: "weird".into(),
                state: 1,
                ..Default::default()
            }],
            ..Default::default()
        };
        let NetworkDeviceDetail::Unknown(u) = svc(api).get_device("weird-box").unwrap() else {
            panic!("expected unknown")
        };
        assert_eq!(u.id, "weird-box");
        assert_eq!(u.model, "WB1");
        assert_eq!(u.mac, "bb:bb:bb:bb:bb:01");
    }

    #[test]
    fn get_device_not_found() {
        let err = svc(fixtures()).get_device("nonexistent").unwrap_err();
        assert!(matches!(err, Error::NotFound(_)));
        assert_eq!(err.to_string(), "Network device not found: nonexistent");
        // The old prefixed form is not accepted.
        assert!(matches!(
            svc(fixtures()).get_device("unifi.us-8"),
            Err(Error::NotFound(_))
        ));
    }

    #[test]
    fn get_device_switch() {
        let NetworkDeviceDetail::Switch(sw) = svc(fixtures()).get_device("us-8-60w").unwrap()
        else {
            panic!("expected switch")
        };
        assert_eq!(sw.base.model, "US8P60");
        assert_eq!(sw.ports.len(), 12);
        let p1 = &sw.ports[0];
        assert_eq!(p1.number, 1);
        assert_eq!(p1.state, PortState::Up);
        assert_eq!(p1.link_speed, Some(LinkSpeed::Gbe1));
        assert!(p1.poe_mode.is_none() && p1.poe_power_watts.is_none());
        assert_eq!(p1.traffic.tx_bytes_total, 107288538);
        let p4 = &sw.ports[3];
        assert_eq!(p4.state, PortState::Down);
        assert!(p4.link_speed.is_none());
        let p5 = &sw.ports[4];
        assert_eq!(p5.poe_mode, Some(PoeMode::Auto));
        assert_eq!(p5.poe_power_watts, Some(2.88));
        assert_eq!(sw.base.traffic.tx_bytes_total, 60801611);
        assert_eq!(sw.base.uplink.as_ref().unwrap().device.id, "cgf-01");
    }

    #[test]
    fn switch_port_connected_to_device_and_client() {
        let ports = switch_ports("us-8-60w");
        assert_eq!(
            port(&ports, 5).connected_to,
            Some(ConnectionRef::Device(DeviceRef::new(
                "uap-01".into(),
                "UAP-01".into()
            )))
        );
        assert_eq!(
            port(&ports, 3).connected_to,
            Some(ConnectionRef::Client(ClientRef::new(
                "nas-1-0002".into(),
                "nas-1".into()
            )))
        );
    }

    #[test]
    fn switch_uplink_port_points_at_upstream_device() {
        let ports = switch_ports("us-8");
        assert_eq!(
            port(&ports, 1).connected_to,
            Some(ConnectionRef::Device(DeviceRef::new(
                "us-8-60w".into(),
                "US 8 60W".into()
            )))
        );
    }

    #[test]
    fn switch_port_vlan_configs() {
        let ports = switch_ports("us-8-60w");
        let p2 = port(&ports, 2).vlan_config.clone().unwrap();
        assert_eq!(p2.mode, VlanMode::Access);
        assert_eq!(p2.native_vlan.id, "lan-int");
        assert!(p2.tagged_vlans.is_none());

        let p1 = port(&ports, 1).vlan_config.clone().unwrap();
        assert_eq!(p1.mode, VlanMode::Trunk);
        assert_eq!(p1.native_vlan.id, "lan-mgmt");
        assert_eq!(p1.tagged_vlans.as_ref().unwrap().scope, TaggedScope::All);

        let p7 = port(&ports, 7).vlan_config.clone().unwrap();
        assert_eq!(p7.native_vlan.id, "lan-mgmt");
        assert_eq!(p7.tagged_vlans.as_ref().unwrap().scope, TaggedScope::All);

        let p6 = port(&ports, 6).vlan_config.clone().unwrap();
        let tagged = p6.tagged_vlans.unwrap();
        assert_eq!(tagged.scope, TaggedScope::Custom);
        let items = tagged.items.unwrap();
        assert_eq!(
            items.iter().map(|v| v.id.as_str()).collect::<Vec<_>>(),
            ["lan-int"]
        );
    }

    #[test]
    fn switch_port_labels_sfp_uptime_lag() {
        let ports = switch_ports("us-8-60w");
        assert_eq!(port(&ports, 11).label.as_deref(), Some("LAG Master"));
        assert!(port(&ports, 1).label.is_none());
        assert_eq!(port(&ports, 9).sfp_module_present, Some(true));
        assert_eq!(port(&ports, 10).sfp_module_present, Some(false));
        assert_eq!(port(&ports, 1).sfp_module_present, None);
        assert_eq!(port(&ports, 1).link_uptime, Some(3600));
        assert_eq!(
            port(&ports, 11).lag_membership,
            Some(LagMembership {
                id: 11,
                role: LagRole::Master
            })
        );
        assert_eq!(
            port(&ports, 12).lag_membership,
            Some(LagMembership {
                id: 11,
                role: LagRole::Member
            })
        );
        assert_eq!(port(&ports, 1).lag_membership, None);
    }

    #[test]
    fn get_device_access_point() {
        let NetworkDeviceDetail::AccessPoint(ap) = svc(fixtures()).get_device("uap-01").unwrap()
        else {
            panic!("expected access point")
        };
        assert_eq!(ap.base.model, "U7LT");
        assert_eq!(ap.connected_clients.len(), 3);
        assert_eq!(ap.num_clients, 3);
        let mb = ap
            .connected_clients
            .iter()
            .find(|c| c.client.name == "MacBook Pro")
            .unwrap();
        assert_eq!(mb.client.id, "macbook-pro-0001");
        assert_eq!(mb.ssid, "homelab");
        assert_eq!(mb.signal_strength, -69);
        let uplink = ap.base.uplink.unwrap();
        assert_eq!(uplink.device.id, "us-8-60w");
        assert_eq!(uplink.port, Some(5));
    }

    // ---- unit helpers (ported from service_test.go) ----

    #[test]
    fn vlan_config_access() {
        let cfg = vlan_config(PortEntry {
            port_idx: 2,
            forward: "native".into(),
            native_networkconf_id: Some("id-int".into()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(cfg.mode, VlanMode::Access);
        assert_eq!(
            cfg.native_vlan,
            VlanRef {
                id: "lan-int".into(),
                name: "LAN-INT".into(),
                vlan_id: 10
            }
        );
        assert!(cfg.tagged_vlans.is_none());
    }

    #[test]
    fn vlan_config_trunk_all_falls_back_to_default_native() {
        let cfg = vlan_config(PortEntry {
            port_idx: 1,
            forward: "all".into(),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(cfg.mode, VlanMode::Trunk);
        assert_eq!(cfg.native_vlan.id, "lan-mgmt");
        assert_eq!(cfg.native_vlan.vlan_id, 1);
        assert_eq!(
            cfg.tagged_vlans,
            Some(TaggedVlans {
                items: None,
                scope: TaggedScope::All
            })
        );
    }

    #[test]
    fn vlan_config_trunk_custom_excludes_native_and_excluded() {
        let cfg = vlan_config(PortEntry {
            port_idx: 6,
            forward: "customize".into(),
            native_networkconf_id: Some("id-mgmt".into()),
            excluded_networkconf_ids: vec!["id-iot".into(), "id-srv".into()],
            ..Default::default()
        })
        .unwrap();
        let tagged = cfg.tagged_vlans.unwrap();
        assert_eq!(tagged.scope, TaggedScope::Custom);
        assert_eq!(
            tagged
                .items
                .unwrap()
                .iter()
                .map(|v| v.id.as_str())
                .collect::<Vec<_>>(),
            ["lan-int"]
        );
    }

    #[test]
    fn vlan_config_customize_without_exclusions_is_trunk_all() {
        let cfg = vlan_config(PortEntry {
            forward: "customize".into(),
            native_networkconf_id: Some("id-mgmt".into()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(cfg.tagged_vlans.unwrap().scope, TaggedScope::All);
    }

    #[test]
    fn vlan_config_custom_sorts_items_by_vlan_id() {
        let cfg = vlan_config(PortEntry {
            forward: "customize".into(),
            native_networkconf_id: Some("id-mgmt".into()),
            excluded_networkconf_ids: vec!["nothing".into()],
            ..Default::default()
        })
        .unwrap();
        let ids: Vec<i64> = cfg
            .tagged_vlans
            .unwrap()
            .items
            .unwrap()
            .iter()
            .map(|v| v.vlan_id)
            .collect();
        assert_eq!(ids, [10, 20, 100]);
    }

    #[test]
    fn vlan_config_disabled_or_unresolvable_is_none() {
        assert!(
            vlan_config(PortEntry {
                forward: "disable".into(),
                ..Default::default()
            })
            .is_none()
        );
        assert!(
            vlan_config(PortEntry {
                forward: "native".into(),
                native_networkconf_id: Some("nonexistent".into()),
                ..Default::default()
            })
            .is_none()
        );
    }

    #[test]
    fn find_default_net_id_picks_untagged_corporate() {
        let confs = vec![
            NetworkConf {
                id: "wan1".into(),
                purpose: "wan".into(),
                ..Default::default()
            },
            NetworkConf {
                id: "tagged".into(),
                purpose: "corporate".into(),
                vlan_enabled: true,
                ..Default::default()
            },
            NetworkConf {
                id: "default".into(),
                purpose: "corporate".into(),
                ..Default::default()
            },
        ];
        assert_eq!(find_default_net_id(&confs), Some("default"));
        assert_eq!(find_default_net_id(&[]), None);
    }

    #[test]
    fn port_label_hides_default_name() {
        assert_eq!(
            port_label(&PortEntry {
                port_idx: 3,
                name: "Port 3".into(),
                ..Default::default()
            }),
            None
        );
        assert_eq!(
            port_label(&PortEntry {
                port_idx: 11,
                name: "LAG Master".into(),
                ..Default::default()
            }),
            Some("LAG Master".into())
        );
    }

    #[test]
    fn link_uptime_only_when_up() {
        assert_eq!(
            link_uptime(&PortEntry {
                up: true,
                uptime: Some(3600),
                ..Default::default()
            }),
            Some(3600)
        );
        assert_eq!(
            link_uptime(&PortEntry {
                up: true,
                ..Default::default()
            }),
            None
        );
        assert_eq!(
            link_uptime(&PortEntry {
                up: false,
                uptime: Some(999),
                ..Default::default()
            }),
            None
        );
    }

    #[test]
    fn lag_membership_roles() {
        let masters = HashSet::from([11]);
        let master = PortEntry {
            port_idx: 11,
            aggregated_by: json!(false),
            ..Default::default()
        };
        assert_eq!(
            lag_membership(&master, &masters),
            Some(LagMembership {
                id: 11,
                role: LagRole::Master
            })
        );
        let member = PortEntry {
            port_idx: 12,
            aggregated_by: json!(11),
            ..Default::default()
        };
        assert_eq!(
            lag_membership(&member, &masters),
            Some(LagMembership {
                id: 11,
                role: LagRole::Member
            })
        );
        let none = PortEntry {
            port_idx: 1,
            aggregated_by: json!(false),
            ..Default::default()
        };
        assert_eq!(lag_membership(&none, &masters), None);
    }

    #[test]
    fn list_devices_carries_experience_and_uplink() {
        let items = svc(fixtures()).list_devices().unwrap().items;
        let by_id = |id: &str| items.iter().find(|d| d.id == id).unwrap();
        assert_eq!(by_id("uap-01").experience, Some(98));
        assert_eq!(by_id("us-8").experience, Some(72));
        assert_eq!(by_id("switch-flex-mini").experience, None, "-1 means none");
        assert_eq!(by_id("uap-01").model, "U7LT");
        assert!(
            by_id("cgf-01").uplink.is_none(),
            "gateway has no upstream device"
        );
        assert_eq!(
            by_id("cgf-01").traffic.rx_bytes_per_sec,
            946,
            "gateway rates are WAN rates"
        );

        let ap = by_id("uap-01").uplink.as_ref().unwrap();
        assert_eq!(ap.device.id, "us-8-60w");
        assert_eq!(ap.port, Some(5));
        assert_eq!(ap.link_speed, Some(LinkSpeed::Fe));
        assert_eq!(ap.max_link_speed, Some(LinkSpeed::Gbe1));
        assert_eq!(ap.full_duplex, Some(true));

        let flex = by_id("usw-flex-2-5g-8").uplink.as_ref().unwrap();
        assert_eq!(flex.link_speed, Some(LinkSpeed::Gbe2p5));
        assert_eq!(
            flex.max_link_speed,
            Some(LinkSpeed::Gbe2p5),
            "own port 10G, upstream port 2.5G"
        );
        assert_eq!(
            by_id("switch-flex-mini")
                .uplink
                .as_ref()
                .unwrap()
                .full_duplex,
            Some(false)
        );
    }

    #[test]
    fn list_item_is_the_detail_base() {
        let s = svc(fixtures());
        let items = s.list_devices().unwrap().items;
        for d in &items {
            let base = match s.get_device(&d.id).unwrap() {
                NetworkDeviceDetail::Switch(x) => x.base,
                NetworkDeviceDetail::Gateway(x) => x.base,
                NetworkDeviceDetail::AccessPoint(x) => x.base,
                NetworkDeviceDetail::Unknown(x) => x,
            };
            assert_eq!(&base, d, "{}", d.id);
        }
    }

    #[test]
    fn effective_max_unknown_media_uses_own_max() {
        let mut api = fixtures();
        for d in &mut api.devices {
            for p in &mut d.port_table {
                p.media = "SFP28".into();
            }
        }
        let items = svc(api).list_devices().unwrap().items;
        let flex = items.iter().find(|d| d.id == "usw-flex-2-5g-8").unwrap();
        assert_eq!(
            flex.uplink.as_ref().unwrap().max_link_speed,
            Some(LinkSpeed::Gbe10)
        );
    }

    #[test]
    fn port_media_maps_to_speed() {
        assert_eq!(map_port_media("FE"), Some(LinkSpeed::Fe));
        assert_eq!(map_port_media("GE"), Some(LinkSpeed::Gbe1));
        assert_eq!(map_port_media("2.5GE"), Some(LinkSpeed::Gbe2p5));
        assert_eq!(map_port_media("2P5GE"), Some(LinkSpeed::Gbe2p5));
        assert_eq!(map_port_media("10GE"), Some(LinkSpeed::Gbe10));
        assert_eq!(map_port_media("SFP+"), Some(LinkSpeed::Gbe10));
        assert_eq!(map_port_media("SFP28"), None);
        assert_eq!(map_port_media(""), None);
    }

    #[test]
    fn disconnected_device_has_no_experience() {
        let mut api = fixtures();
        api.devices
            .iter_mut()
            .find(|d| d.name == "UAP-01")
            .unwrap()
            .state = 0;
        let items = svc(api).list_devices().unwrap().items;
        let ap = items.iter().find(|d| d.id == "uap-01").unwrap();
        assert_eq!(ap.status, DeviceStatus::Disconnected);
        assert_eq!(ap.experience, None, "a stale score is not live");
    }
}
