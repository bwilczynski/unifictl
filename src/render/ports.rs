//! `ports list` and the shared port decoration (hlctl ports.go `decoratePort`).

use super::Table;
use super::format::{format_bytes_per_sec, format_link_speed, format_uptime};
use super::style;
use crate::model::{
    DevicePort, NetworkPort, NetworkPortList, PortState, TaggedScope, VlanMode, VlanRef,
};

/// Display strings for one port; absent values are `-`.
pub struct PortRow {
    pub label: String,
    pub link_speed: String,
    pub link_uptime: String,
    pub lag_info: String,
    pub sfp_present: String,
    pub poe_mode: String,
    pub poe_watts: String,
    pub vlan_mode: String,
    pub native_vlan: String,
    pub tagged_vlans: String,
    pub rx: String,
    pub tx: String,
    pub connected_to: String,
}

fn dash() -> String {
    "-".to_string()
}

fn vlan_label(v: &VlanRef) -> String {
    format!("{} ({})", v.name, v.vlan_id)
}

pub fn decorate_port(p: &DevicePort) -> PortRow {
    let (vlan_mode, native_vlan, tagged_vlans) = match &p.vlan_config {
        None => (dash(), dash(), dash()),
        Some(cfg) => {
            let tagged = match (&cfg.mode, &cfg.tagged_vlans) {
                (VlanMode::Trunk, Some(tv)) => match (tv.scope, &tv.items) {
                    (TaggedScope::All, _) => "all".to_string(),
                    (TaggedScope::Custom, Some(items)) if !items.is_empty() => {
                        items.iter().map(vlan_label).collect::<Vec<_>>().join(", ")
                    }
                    (TaggedScope::Custom, _) => dash(),
                },
                _ => dash(),
            };
            (cfg.mode.to_string(), vlan_label(&cfg.native_vlan), tagged)
        }
    };
    PortRow {
        label: p.label.clone().unwrap_or_else(dash),
        link_speed: match (p.state, p.link_speed) {
            (PortState::Up, Some(ls)) => format_link_speed(ls).to_string(),
            _ => dash(),
        },
        link_uptime: p.link_uptime.map(format_uptime).unwrap_or_else(dash),
        lag_info: p
            .lag_membership
            .as_ref()
            .map(|l| format!("{} #{}", l.role, l.id))
            .unwrap_or_else(dash),
        sfp_present: match p.sfp_module_present {
            Some(true) => "yes".to_string(),
            Some(false) => "no".to_string(),
            None => dash(),
        },
        poe_mode: p.poe_mode.map(|m| m.to_string()).unwrap_or_else(dash),
        poe_watts: p
            .poe_power_watts
            .map(|w| format!("{w:.1} W"))
            .unwrap_or_else(dash),
        vlan_mode,
        native_vlan,
        tagged_vlans,
        rx: format_bytes_per_sec(p.traffic.rx_bytes_per_sec),
        tx: format_bytes_per_sec(p.traffic.tx_bytes_per_sec),
        connected_to: p
            .connected_to
            .as_ref()
            .map(|c| c.name().to_string())
            .unwrap_or_else(dash),
    }
}

/// `ports_list.tmpl`.
pub fn list_table(list: &NetworkPortList, wide: bool, color: bool) -> String {
    let mut t = Table::new();
    if wide {
        t.row(&[
            "DEVICE",
            "PORT",
            "LABEL",
            "STATE",
            "LINK SPEED",
            "MODE",
            "NATIVE VLAN",
            "TAGGED VLANS",
            "POE",
            "LAG",
            "SFP",
            "RX/S",
            "TX/S",
            "CONNECTED TO",
        ]);
    } else {
        t.row(&[
            "DEVICE",
            "PORT",
            "LABEL",
            "STATE",
            "MODE",
            "NATIVE VLAN",
            "TAGGED VLANS",
            "CONNECTED TO",
        ]);
    }
    // hlctl sorts table rows (not JSON) by device name, then port number.
    let mut items: Vec<&NetworkPort> = list.items.iter().collect();
    items.sort_by(|a, b| {
        a.device
            .name
            .cmp(&b.device.name)
            .then(a.port.number.cmp(&b.port.number))
    });
    for np in items {
        let p = &np.port;
        let r = decorate_port(p);
        let number = p.number.to_string();
        let state = style::port_state(p.state, color);
        if wide {
            t.row(&[
                &np.device.name,
                &number,
                &r.label,
                &state,
                &r.link_speed,
                &r.vlan_mode,
                &r.native_vlan,
                &r.tagged_vlans,
                &r.poe_mode,
                &r.lag_info,
                &r.sfp_present,
                &r.rx,
                &r.tx,
                &r.connected_to,
            ]);
        } else {
            t.row(&[
                &np.device.name,
                &number,
                &r.label,
                &state,
                &r.vlan_mode,
                &r.native_vlan,
                &r.tagged_vlans,
                &r.connected_to,
            ]);
        }
    }
    t.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    fn port() -> DevicePort {
        DevicePort {
            number: 6,
            label: None,
            state: PortState::Up,
            link_speed: Some(LinkSpeed::Gbe2p5),
            link_uptime: Some(90),
            sfp_module_present: Some(false),
            poe_mode: Some(PoeMode::Auto),
            poe_power_watts: Some(6.1),
            lag_membership: Some(LagMembership {
                id: 11,
                role: LagRole::Member,
            }),
            vlan_config: Some(VlanConfig {
                mode: VlanMode::Trunk,
                native_vlan: VlanRef {
                    id: "lan-mgmt".into(),
                    name: "LAN-MGMT".into(),
                    vlan_id: 1,
                },
                tagged_vlans: Some(TaggedVlans {
                    items: Some(vec![
                        VlanRef {
                            id: "lan-int".into(),
                            name: "LAN-INT".into(),
                            vlan_id: 10,
                        },
                        VlanRef {
                            id: "lan-iot".into(),
                            name: "LAN-IOT".into(),
                            vlan_id: 20,
                        },
                    ]),
                    scope: TaggedScope::Custom,
                }),
            }),
            traffic: Traffic {
                rx_bytes_per_sec: 2048,
                tx_bytes_per_sec: 0,
                ..Default::default()
            },
            connected_to: Some(ConnectionRef::Device(DeviceRef::new(
                "switch-flex".into(),
                "Switch-Flex".into(),
            ))),
        }
    }

    #[test]
    fn decorates_every_field() {
        let r = decorate_port(&port());
        assert_eq!(r.label, "-");
        assert_eq!(r.link_speed, "2.5 GbE");
        assert_eq!(r.link_uptime, "1m 30s");
        assert_eq!(r.lag_info, "member #11");
        assert_eq!(r.sfp_present, "no");
        assert_eq!(r.poe_mode, "auto");
        assert_eq!(r.poe_watts, "6.1 W");
        assert_eq!(r.vlan_mode, "trunk");
        assert_eq!(r.native_vlan, "LAN-MGMT (1)");
        assert_eq!(r.tagged_vlans, "LAN-INT (10), LAN-IOT (20)");
        assert_eq!(r.rx, "2.0 KB/s");
        assert_eq!(r.tx, "0 B/s");
        assert_eq!(r.connected_to, "Switch-Flex");
    }

    #[test]
    fn decorates_absent_fields_with_dashes() {
        let p = DevicePort {
            state: PortState::Down,
            label: Some("Uplink".into()),
            link_speed: Some(LinkSpeed::Gbe1),
            link_uptime: None,
            sfp_module_present: None,
            poe_mode: None,
            poe_power_watts: None,
            lag_membership: None,
            vlan_config: None,
            connected_to: None,
            ..port()
        };
        let r = decorate_port(&p);
        assert_eq!(r.label, "Uplink");
        assert_eq!(r.link_speed, "-", "down ports show no speed");
        for v in [
            &r.link_uptime,
            &r.sfp_present,
            &r.lag_info,
            &r.poe_mode,
            &r.poe_watts,
            &r.vlan_mode,
        ] {
            assert_eq!(v, "-");
        }
        assert_eq!(
            (
                r.native_vlan.as_str(),
                r.tagged_vlans.as_str(),
                r.connected_to.as_str()
            ),
            ("-", "-", "-")
        );
    }

    #[test]
    fn trunk_all_and_custom_empty() {
        let mut p = port();
        p.vlan_config.as_mut().unwrap().tagged_vlans = Some(TaggedVlans {
            items: None,
            scope: TaggedScope::All,
        });
        assert_eq!(decorate_port(&p).tagged_vlans, "all");
        p.vlan_config.as_mut().unwrap().tagged_vlans = Some(TaggedVlans {
            items: Some(vec![]),
            scope: TaggedScope::Custom,
        });
        assert_eq!(decorate_port(&p).tagged_vlans, "-");
    }

    #[test]
    fn list_rows_sorted_by_device_name_then_port() {
        let np = |dev: &str, number: i64| NetworkPort {
            port: DevicePort { number, ..port() },
            device: DeviceRef::new(dev.to_lowercase(), dev.into()),
        };
        let list = NetworkPortList {
            items: vec![
                np("US 8", 2),
                np("CGF-01", 9),
                np("US 8", 1),
                np("CGF-01", 3),
            ],
        };
        let order: Vec<String> = list_table(&list, false, false)
            .lines()
            .skip(1)
            .map(|l| {
                l.split("  ")
                    .filter(|c| !c.is_empty())
                    .take(2)
                    .collect::<Vec<_>>()
                    .join("/")
            })
            .collect();
        assert_eq!(order, ["CGF-01/3", "CGF-01/9", "US 8/1", "US 8/2"]);
    }

    #[test]
    fn list_narrow_and_wide_headers() {
        let list = NetworkPortList {
            items: vec![NetworkPort {
                port: port(),
                device: DeviceRef::new("core".into(), "Switch-Core".into()),
            }],
        };
        let narrow = list_table(&list, false, false);
        assert!(narrow.starts_with("DEVICE"));
        assert!(narrow.lines().next().unwrap().ends_with("CONNECTED TO"));
        assert!(!narrow.contains("RX/S"));
        let wide = list_table(&list, true, false);
        assert!(wide.lines().next().unwrap().contains("LINK SPEED"));
        assert!(wide.contains("2.0 KB/s"));
    }
}
