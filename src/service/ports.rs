//! Flat port listing: ported from homelab-api/internal/network/ports_service.go.

use super::Service;
use super::common::*;
use super::devices::{PortIndex, build_device_ports, build_gateway_ports};
use crate::error::Result;
use crate::model::*;
use crate::unifi::fetch;

/// Query filters for `ports list`; `None` means "do not filter".
#[derive(Debug, Clone, Default)]
pub struct PortFilter {
    pub device_id: Option<String>,
    pub state: Option<PortState>,
    pub mode: Option<VlanMode>,
    pub vlan_id: Option<i64>,
}

impl PortFilter {
    fn matches(&self, p: &NetworkPort) -> bool {
        if self
            .device_id
            .as_deref()
            .is_some_and(|id| p.device.id != id)
        {
            return false;
        }
        if self.state.is_some_and(|s| p.port.state != s) {
            return false;
        }
        if self
            .mode
            .is_some_and(|mode| p.port.vlan_config.as_ref().is_none_or(|c| c.mode != mode))
        {
            return false;
        }
        !self
            .vlan_id
            .is_some_and(|vlan_id| !carries_vlan(p.port.vlan_config.as_ref(), vlan_id))
    }
}

/// Native match, or trunk-all, or a tagged-custom item match.
fn carries_vlan(cfg: Option<&VlanConfig>, vlan_id: i64) -> bool {
    let Some(cfg) = cfg else { return false };
    if cfg.native_vlan.vlan_id == vlan_id {
        return true;
    }
    match &cfg.tagged_vlans {
        Some(TaggedVlans {
            scope: TaggedScope::All,
            ..
        }) => true,
        Some(TaggedVlans {
            items: Some(items), ..
        }) => items.iter().any(|v| v.vlan_id == vlan_id),
        _ => false,
    }
}

impl Service {
    /// Every switch port plus gateway LAN ports, in controller order.
    pub fn list_ports(&self, filter: &PortFilter) -> Result<NetworkPortList> {
        fetch!(
            devices = self.api.devices(),
            clients = self.api.clients(),
            confs = self.api.network_conf(),
        );
        let idx = PortIndex::new(&devices, &clients);

        let mut items = Vec::new();
        for d in devices
            .iter()
            .filter(|d| d.kind == "usw" || is_gateway_type(&d.kind))
        {
            let device = device_ref(d);
            let ports = if is_gateway_type(&d.kind) {
                build_gateway_ports(d, &idx, &confs)
            } else {
                build_device_ports(d, &d.port_table, &idx, &confs)
            };
            items.extend(
                ports
                    .into_iter()
                    .map(|port| NetworkPort {
                        port,
                        device: device.clone(),
                    })
                    .filter(|p| filter.matches(p)),
            );
        }
        Ok(NetworkPortList { items })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::testutil::{fixtures, svc};

    fn list(filter: PortFilter) -> Vec<NetworkPort> {
        svc(fixtures()).list_ports(&filter).unwrap().items
    }

    fn dev(id: &str) -> PortFilter {
        PortFilter {
            device_id: Some(id.into()),
            ..Default::default()
        }
    }

    #[test]
    fn no_filter_lists_switch_and_gateway_lan_ports() {
        let items = list(PortFilter::default());
        assert_eq!(items.len(), 40, "35 switch + 5 gateway LAN");
        assert!(
            items
                .iter()
                .all(|p| !p.device.id.is_empty() && p.device.kind == "device")
        );
        assert_eq!(
            items.iter().filter(|p| p.device.id == "us-8-60w").count(),
            12
        );
    }

    #[test]
    fn device_filter() {
        let items = list(dev("us-8-60w"));
        assert_eq!(items.len(), 12);
        assert!(items.iter().all(|p| p.device.id == "us-8-60w"));
        assert_eq!(list(dev("cgf-01")).len(), 5);
    }

    #[test]
    fn state_filter() {
        let items = list(PortFilter {
            state: Some(PortState::Up),
            ..dev("us-8-60w")
        });
        assert_eq!(items.len(), 7);
        assert!(items.iter().all(|p| p.port.state == PortState::Up));
    }

    #[test]
    fn mode_filters() {
        let access = list(PortFilter {
            mode: Some(VlanMode::Access),
            ..dev("us-8-60w")
        });
        assert_eq!(access.len(), 3);
        let trunk = list(PortFilter {
            mode: Some(VlanMode::Trunk),
            ..dev("us-8-60w")
        });
        assert_eq!(trunk.len(), 9);
        let all_trunk = list(PortFilter {
            mode: Some(VlanMode::Trunk),
            ..Default::default()
        });
        assert!(all_trunk.iter().all(|p| p.port.vlan_config.is_some()));
    }

    #[test]
    fn vlan_filters() {
        let native = list(PortFilter {
            mode: Some(VlanMode::Access),
            vlan_id: Some(10),
            ..dev("us-8-60w")
        });
        assert_eq!(native.len(), 3);
        let trunk_all = list(PortFilter {
            mode: Some(VlanMode::Trunk),
            vlan_id: Some(100),
            ..dev("us-8-60w")
        });
        assert_eq!(
            trunk_all.len(),
            8,
            "trunk-all matches any VLAN; port 6 excludes 100"
        );
        let custom = list(PortFilter {
            mode: Some(VlanMode::Trunk),
            state: Some(PortState::Up),
            vlan_id: Some(10),
            ..dev("us-8-60w")
        });
        assert_eq!(custom.len(), 7);
        assert!(custom.iter().any(|p| p.port.number == 6));
    }

    #[test]
    fn uplink_port_connected_to_upstream() {
        let items = list(dev("us-8"));
        let p1 = items.iter().find(|p| p.port.number == 1).unwrap();
        assert_eq!(p1.port.connected_to.as_ref().unwrap().id(), "us-8-60w");
    }

    #[test]
    fn gateway_wan_ports_excluded() {
        let gw: Vec<i64> = list(dev("cgf-01")).iter().map(|p| p.port.number).collect();
        assert!(!gw.contains(&5) && !gw.contains(&7), "{gw:?}");
    }

    #[test]
    fn json_flattens_port_with_device() {
        let items = list(dev("cgf-01"));
        let v = serde_json::to_value(&items[0]).unwrap();
        assert_eq!(v["device"]["id"], "cgf-01");
        assert!(v.get("number").is_some());
    }
}
