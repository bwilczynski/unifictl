//! Helpers shared by the network services, ported from
//! homelab-api/internal/network (service.go, devices_service.go,
//! clients_service.go, vlans_service.go).

use std::collections::HashMap;

use serde_json::Value;

use crate::model::{
    ClientRef, ConnectionType, DeviceRef, DeviceStatus, DeviceType, LinkSpeed, PoeMode, PortState,
};
use crate::unifi::types::{ClientV2, Device, Sta};

/// Lowercase, runs of anything but `[a-z0-9]` → `-`, trimmed of `-`.
pub(crate) fn to_kebab(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut pending_dash = false;
    for c in name.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.push(c);
        } else {
            pending_dash = true;
        }
    }
    out
}

pub(crate) fn normalize_mac(mac: &str) -> String {
    mac.to_lowercase()
}

/// First two hex chars of the MAC (colons stripped); falls back to the first
/// two chars of `fallback_id` (dashes stripped), then `"xx"`.
pub(crate) fn mac_hex_prefix(mac: &str, fallback_id: &str) -> String {
    let hex: String = mac.chars().filter(|&c| c != ':').collect();
    if hex.chars().count() >= 2 {
        return hex.chars().take(2).collect();
    }
    let id: String = fallback_id.chars().filter(|&c| c != '-').collect();
    if id.chars().count() >= 2 {
        return id.chars().take(2).collect();
    }
    "xx".to_string()
}

fn first_non_empty(candidates: [Option<&String>; 2], fallback: &str) -> String {
    candidates
        .into_iter()
        .flatten()
        .find(|s| !s.is_empty())
        .cloned()
        .unwrap_or_else(|| fallback.to_string())
}

/// Alias, then hostname, then MAC.
pub(crate) fn client_name(sta: &Sta) -> String {
    first_non_empty([sta.name.as_ref(), sta.hostname.as_ref()], &sta.mac)
}

pub(crate) fn client_name_v2(c: &ClientV2) -> String {
    first_non_empty([c.name.as_ref(), c.hostname.as_ref()], &c.mac)
}

/// Client ID for a v1 sta: `{kebab-name}-{mac-prefix}`.
pub(crate) fn client_suffix(sta: &Sta) -> String {
    format!(
        "{}-{}",
        to_kebab(&client_name(sta)),
        mac_hex_prefix(&normalize_mac(&sta.mac), "")
    )
}

/// Client ID for a v2 client: `{kebab-name}-{mac-prefix}` (falls back to the v2 id).
pub(crate) fn client_id_v2(c: &ClientV2) -> String {
    format!(
        "{}-{}",
        to_kebab(&client_name_v2(c)),
        mac_hex_prefix(&normalize_mac(&c.mac), &c.id)
    )
}

pub(crate) fn device_ref(d: &Device) -> DeviceRef {
    DeviceRef::new(to_kebab(&d.name), d.name.clone())
}

pub(crate) fn client_ref(sta: &Sta) -> ClientRef {
    ClientRef::new(client_suffix(sta), client_name(sta))
}

pub(crate) fn client_ref_v2(c: &ClientV2) -> ClientRef {
    ClientRef::new(client_id_v2(c), client_name_v2(c))
}

pub(crate) fn is_gateway_type(t: &str) -> bool {
    matches!(t, "ugw" | "udm" | "udm-pro")
}

pub(crate) fn map_device_type(t: &str) -> DeviceType {
    match t {
        "uap" => DeviceType::AccessPoint,
        "usw" => DeviceType::Switch,
        t if is_gateway_type(t) => DeviceType::Gateway,
        _ => DeviceType::Unknown,
    }
}

pub(crate) fn map_device_status(state: i64) -> DeviceStatus {
    if state == 1 {
        DeviceStatus::Connected
    } else {
        DeviceStatus::Disconnected
    }
}

pub(crate) fn map_link_speed(mbps: i64) -> Option<LinkSpeed> {
    match mbps {
        10 => Some(LinkSpeed::E),
        100 => Some(LinkSpeed::Fe),
        1000 => Some(LinkSpeed::Gbe1),
        2500 => Some(LinkSpeed::Gbe2p5),
        5000 => Some(LinkSpeed::Gbe5),
        10000 => Some(LinkSpeed::Gbe10),
        _ => None,
    }
}

pub(crate) fn map_connection_type(is_wired: bool) -> ConnectionType {
    if is_wired {
        ConnectionType::Wired
    } else {
        ConnectionType::Wireless
    }
}

pub(crate) fn map_port_state(up: bool) -> PortState {
    if up { PortState::Up } else { PortState::Down }
}

pub(crate) fn map_poe_mode(mode: &str) -> PoeMode {
    match mode {
        "auto" => PoeMode::Auto,
        "passive24v" => PoeMode::Passive24v,
        "passthrough" => PoeMode::Passthrough,
        _ => PoeMode::Off,
    }
}

/// Numeric `vlan` → that number; `""`, `null` or anything else → 1 (untagged).
pub(crate) fn extract_vlan_id(v: &Value) -> i64 {
    v.as_f64().map(|f| f as i64).unwrap_or(1)
}

pub(crate) fn collect_dns(dns: &[&str]) -> Vec<String> {
    dns.iter()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

pub(crate) fn mac_to_device(devices: &[Device]) -> HashMap<String, &Device> {
    devices.iter().map(|d| (normalize_mac(&d.mac), d)).collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn to_kebab_matches_go() {
        for (input, want) in [
            ("USG 3P", "usg-3p"),
            ("AP Living Room", "ap-living-room"),
            ("Switch Flex Mini", "switch-flex-mini"),
            ("US 8 60W", "us-8-60w"),
            ("UAP-01", "uap-01"),
            ("Sonos One SL (L)", "sonos-one-sl-l"),
            ("  --Edge--  ", "edge"),
            ("ec:ee:ee:00:00:04", "ec-ee-ee-00-00-04"),
        ] {
            assert_eq!(to_kebab(input), want, "to_kebab({input:?})");
        }
    }

    #[test]
    fn map_device_type_matches_go() {
        for (input, want) in [
            ("uap", DeviceType::AccessPoint),
            ("usw", DeviceType::Switch),
            ("ugw", DeviceType::Gateway),
            ("udm", DeviceType::Gateway),
            ("udm-pro", DeviceType::Gateway),
            ("other", DeviceType::Unknown),
            ("", DeviceType::Unknown),
        ] {
            assert_eq!(map_device_type(input), want, "map_device_type({input:?})");
        }
    }

    #[test]
    fn mac_hex_prefix_fallbacks() {
        assert_eq!(mac_hex_prefix("3c:22:fb:aa:bb:01", ""), "3c");
        assert_eq!(mac_hex_prefix("", "e1-23"), "e1");
        assert_eq!(mac_hex_prefix("", ""), "xx");
    }

    #[test]
    fn client_name_prefers_alias_then_hostname_then_mac() {
        let mut sta = Sta {
            mac: "ec:ee:ee:00:00:04".into(),
            ..Default::default()
        };
        assert_eq!(client_name(&sta), "ec:ee:ee:00:00:04");
        assert_eq!(client_suffix(&sta), "ec-ee-ee-00-00-04-ec");
        sta.hostname = Some("nas-1".into());
        assert_eq!(client_name(&sta), "nas-1");
        sta.name = Some(String::new());
        assert_eq!(client_name(&sta), "nas-1");
        sta.name = Some("NAS".into());
        assert_eq!(client_name(&sta), "NAS");
    }

    #[test]
    fn extract_vlan_id_defaults_to_one() {
        assert_eq!(extract_vlan_id(&json!(20)), 20);
        assert_eq!(extract_vlan_id(&json!("")), 1);
        assert_eq!(extract_vlan_id(&json!(null)), 1);
        assert_eq!(extract_vlan_id(&json!("20")), 1);
    }

    #[test]
    fn link_speed_mapping() {
        assert_eq!(map_link_speed(2500), Some(LinkSpeed::Gbe2p5));
        assert_eq!(map_link_speed(25000), None);
    }
}
