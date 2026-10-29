//! Helpers shared by the network services, ported from
//! homelab-api/internal/network (service.go, devices_service.go,
//! clients_service.go, vlans_service.go).

use std::collections::HashMap;

use serde_json::Value;

use crate::model::{
    ClientRef, ConnectionType, DeviceRef, DeviceStatus, DeviceType, LinkSpeed, PoeMode, PortState,
};
use crate::unifi::types::{ClientV2, Device, Sta, TrafficClient};

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

/// Last four hex chars of the MAC (colons stripped); falls back to the last
/// four chars of `fallback_id` (dashes stripped), then `"xxxx"`. The end of
/// the MAC rather than the start: the first byte is shared by every device
/// from one vendor, so same-named devices collided.
pub(crate) fn mac_hex_suffix(mac: &str, fallback_id: &str) -> String {
    let last4 = |s: String| {
        let chars: Vec<char> = s.chars().collect();
        (chars.len() >= 4).then(|| chars[chars.len() - 4..].iter().collect::<String>())
    };
    last4(mac.chars().filter(|&c| c != ':').collect())
        .or_else(|| last4(fallback_id.chars().filter(|&c| c != '-').collect()))
        .unwrap_or_else(|| "xxxx".to_string())
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

/// Client ID for a v1 sta: `{kebab-name}-{mac-suffix}`.
pub(crate) fn client_suffix(sta: &Sta) -> String {
    format!(
        "{}-{}",
        to_kebab(&client_name(sta)),
        mac_hex_suffix(&normalize_mac(&sta.mac), "")
    )
}

/// Client ID for a v2 client: `{kebab-name}-{mac-suffix}` (falls back to the v2 id).
pub(crate) fn client_id_v2(c: &ClientV2) -> String {
    format!(
        "{}-{}",
        to_kebab(&client_name_v2(c)),
        mac_hex_suffix(&normalize_mac(&c.mac), &c.id)
    )
}

/// Alias, then hostname, then MAC, for a client in a traffic report.
pub(crate) fn client_name_traffic(c: &TrafficClient) -> String {
    first_non_empty([c.name.as_ref(), c.hostname.as_ref()], &c.mac)
}

/// Client ID for a traffic report client, matching `clients list`.
pub(crate) fn client_id_traffic(c: &TrafficClient) -> String {
    format!(
        "{}-{}",
        to_kebab(&client_name_traffic(c)),
        mac_hex_suffix(&normalize_mac(&c.mac), "")
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

/// A port's `media` → its maximum speed; unknown media → `None`.
pub(crate) fn map_port_media(media: &str) -> Option<LinkSpeed> {
    match media {
        "FE" => Some(LinkSpeed::Fe),
        "GE" | "SFP" => Some(LinkSpeed::Gbe1),
        "2.5GE" | "2P5GE" => Some(LinkSpeed::Gbe2p5),
        "5GE" => Some(LinkSpeed::Gbe5),
        "10GE" | "SFP+" => Some(LinkSpeed::Gbe10),
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
    fn normalize_mac_only_lowercases() {
        // Shared by logs, whose parameter ids are not always MACs.
        assert_eq!(normalize_mac("68:EE:EE:00:00:02"), "68:ee:ee:00:00:02");
        assert_eq!(normalize_mac("admin-1"), "admin-1");
    }

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
    fn mac_hex_suffix_fallbacks() {
        assert_eq!(mac_hex_suffix("3c:22:fb:aa:bb:01", ""), "bb01");
        assert_eq!(mac_hex_suffix("", "e1-23-45"), "2345");
        assert_eq!(mac_hex_suffix("", "e1"), "xxxx");
        assert_eq!(mac_hex_suffix("", ""), "xxxx");
    }

    #[test]
    fn same_name_same_vendor_clients_get_distinct_ids() {
        let sta = |mac: &str| Sta {
            name: Some("Mac Mini".into()),
            mac: mac.into(),
            ..Default::default()
        };
        let a = client_suffix(&sta("f0:18:98:b0:53:53"));
        let b = client_suffix(&sta("f0:18:98:ee:68:68"));
        assert_eq!((a.as_str(), b.as_str()), ("mac-mini-5353", "mac-mini-6868"));
    }

    #[test]
    fn client_name_prefers_alias_then_hostname_then_mac() {
        let mut sta = Sta {
            mac: "ec:ee:ee:00:00:04".into(),
            ..Default::default()
        };
        assert_eq!(client_name(&sta), "ec:ee:ee:00:00:04");
        assert_eq!(client_suffix(&sta), "ec-ee-ee-00-00-04-0004");
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
