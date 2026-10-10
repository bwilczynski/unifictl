//! Raw UniFi controller payloads, ported field-for-field from
//! homelab-api/internal/adapters/unifi.go. Go's encoding/json turns `null`
//! into a zero value and ignores missing keys; every non-`Option` field here
//! uses `nd` + container `default` to behave the same way.

use serde::{Deserialize, Deserializer};
use serde_json::Value;

fn nd<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

/// `{"meta": {...}, "data": [...]}` wrapper used by the /api/s/{site} endpoints.
#[derive(Debug, Deserialize)]
#[serde(bound(deserialize = "T: Deserialize<'de> + Default"))]
pub struct Envelope<T: Default> {
    #[serde(default, deserialize_with = "nd")]
    pub data: T,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Device {
    #[serde(rename = "_id", deserialize_with = "nd")]
    pub id: String,
    #[serde(deserialize_with = "nd")]
    pub mac: String,
    #[serde(deserialize_with = "nd")]
    pub name: String,
    #[serde(deserialize_with = "nd")]
    pub model: String,
    /// uap, usw, ugw, udm, udm-pro, …
    #[serde(rename = "type", deserialize_with = "nd")]
    pub kind: String,
    #[serde(deserialize_with = "nd")]
    pub state: i64,
    #[serde(deserialize_with = "nd")]
    pub ip: String,
    #[serde(deserialize_with = "nd")]
    pub version: String,
    #[serde(deserialize_with = "nd")]
    pub uptime: i64,
    #[serde(rename = "user-num_sta", deserialize_with = "nd")]
    pub user_num_sta: i64,
    #[serde(rename = "guest-num_sta", deserialize_with = "nd")]
    pub guest_num_sta: i64,
    #[serde(deserialize_with = "nd")]
    pub tx_bytes: i64,
    #[serde(deserialize_with = "nd")]
    pub rx_bytes: i64,
    #[serde(deserialize_with = "nd")]
    pub port_table: Vec<PortEntry>,
    #[serde(deserialize_with = "nd")]
    pub vap_table: Vec<Vap>,
    pub uplink: Option<Uplink>,
    pub wan1: Option<WanIface>,
    pub wan2: Option<WanIface>,
}

/// One row of a device's vap_table — one per radio per SSID.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Vap {
    /// wlanconf _id
    #[serde(deserialize_with = "nd")]
    pub id: String,
    #[serde(deserialize_with = "nd")]
    pub up: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PortEntry {
    #[serde(deserialize_with = "nd")]
    pub ifname: String,
    #[serde(deserialize_with = "nd")]
    pub port_idx: i64,
    #[serde(deserialize_with = "nd")]
    pub up: bool,
    #[serde(deserialize_with = "nd")]
    pub speed: i64,
    #[serde(deserialize_with = "nd")]
    pub port_poe: bool,
    #[serde(deserialize_with = "nd")]
    pub poe_mode: String,
    #[serde(deserialize_with = "nd")]
    pub poe_power: String,
    #[serde(deserialize_with = "nd")]
    pub tx_bytes: i64,
    #[serde(deserialize_with = "nd")]
    pub rx_bytes: i64,
    #[serde(rename = "tx_bytes-r", deserialize_with = "nd")]
    pub tx_bytes_r: f64,
    #[serde(rename = "rx_bytes-r", deserialize_with = "nd")]
    pub rx_bytes_r: f64,
    #[serde(deserialize_with = "nd")]
    pub name: String,
    #[serde(deserialize_with = "nd")]
    pub forward: String,
    pub native_networkconf_id: Option<String>,
    #[serde(deserialize_with = "nd")]
    pub tagged_vlan_mgmt: String,
    #[serde(deserialize_with = "nd")]
    pub excluded_networkconf_ids: Vec<String>,
    pub sfp_found: Option<bool>,
    pub uptime: Option<i64>,
    /// Port index of the LAG master (number) for members, `false` otherwise.
    pub aggregated_by: Value,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Uplink {
    #[serde(deserialize_with = "nd")]
    pub uplink_mac: String,
    #[serde(deserialize_with = "nd")]
    pub uplink_device_name: String,
    pub uplink_remote_port: Option<i64>,
    pub port_idx: Option<i64>,
    #[serde(deserialize_with = "nd")]
    pub speed: i64,
    #[serde(rename = "tx_bytes-r", deserialize_with = "nd")]
    pub tx_bytes_r: f64,
    #[serde(rename = "rx_bytes-r", deserialize_with = "nd")]
    pub rx_bytes_r: f64,
    #[serde(deserialize_with = "nd")]
    pub tx_bytes: i64,
    #[serde(deserialize_with = "nd")]
    pub rx_bytes: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct WanIface {
    #[serde(deserialize_with = "nd")]
    pub name: String,
    #[serde(deserialize_with = "nd")]
    pub ip: String,
    #[serde(deserialize_with = "nd")]
    pub up: bool,
    #[serde(deserialize_with = "nd")]
    pub dns: Vec<String>,
    #[serde(deserialize_with = "nd")]
    pub tx_bytes: i64,
    #[serde(deserialize_with = "nd")]
    pub rx_bytes: i64,
    #[serde(rename = "tx_bytes-r", deserialize_with = "nd")]
    pub tx_bytes_r: f64,
    #[serde(rename = "rx_bytes-r", deserialize_with = "nd")]
    pub rx_bytes_r: f64,
}

/// Active client from /stat/sta (v1 API).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Sta {
    #[serde(deserialize_with = "nd")]
    pub mac: String,
    pub hostname: Option<String>,
    pub name: Option<String>,
    #[serde(deserialize_with = "nd")]
    pub ip: String,
    #[serde(deserialize_with = "nd")]
    pub is_wired: bool,
    pub essid: Option<String>,
    pub signal: Option<i64>,
    #[serde(deserialize_with = "nd")]
    pub uptime: i64,
    /// MAC of the switch the client is wired to.
    #[serde(deserialize_with = "nd")]
    pub sw_mac: String,
    #[serde(deserialize_with = "nd")]
    pub sw_port: i64,
    #[serde(deserialize_with = "nd")]
    pub last_uplink_name: String,
    #[serde(deserialize_with = "nd")]
    pub ap_mac: String,
    #[serde(deserialize_with = "nd")]
    pub wired_rate_mbps: i64,
}

/// Client from the v2 API (active or history).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ClientV2 {
    #[serde(deserialize_with = "nd")]
    pub id: String,
    #[serde(deserialize_with = "nd")]
    pub mac: String,
    #[serde(deserialize_with = "nd")]
    pub display_name: String,
    pub name: Option<String>,
    pub hostname: Option<String>,
    #[serde(deserialize_with = "nd")]
    pub ip: String,
    #[serde(deserialize_with = "nd")]
    pub last_ip: String,
    #[serde(deserialize_with = "nd")]
    pub is_wired: bool,
    /// "online" | "offline"
    #[serde(deserialize_with = "nd")]
    pub status: String,
    #[serde(deserialize_with = "nd")]
    pub last_uplink_name: String,
    #[serde(deserialize_with = "nd")]
    pub uptime: i64,
    pub essid: Option<String>,
    pub signal: Option<i64>,
    #[serde(deserialize_with = "nd")]
    pub last_seen: i64,
    #[serde(deserialize_with = "nd")]
    pub last_uplink_mac: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct WlanConf {
    #[serde(rename = "_id", deserialize_with = "nd")]
    pub id: String,
    #[serde(deserialize_with = "nd")]
    pub name: String,
    #[serde(deserialize_with = "nd")]
    pub networkconf_id: String,
    #[serde(deserialize_with = "nd")]
    pub security: String,
    #[serde(deserialize_with = "nd")]
    pub wpa_mode: String,
    #[serde(deserialize_with = "nd")]
    pub wpa3_support: bool,
    #[serde(deserialize_with = "nd")]
    pub wpa3_transition: bool,
    #[serde(deserialize_with = "nd")]
    pub wlan_bands: Vec<String>,
    #[serde(deserialize_with = "nd")]
    pub enabled: bool,
}

/// Network/VLAN configuration. `vlan` is an integer for tagged VLANs, `""`
/// for the default untagged network and `null` for WAN entries.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct NetworkConf {
    #[serde(rename = "_id", deserialize_with = "nd")]
    pub id: String,
    #[serde(deserialize_with = "nd")]
    pub name: String,
    /// "corporate", "guest", "wan"
    #[serde(deserialize_with = "nd")]
    pub purpose: String,
    #[serde(deserialize_with = "nd")]
    pub networkgroup: String,
    pub vlan: Value,
    #[serde(deserialize_with = "nd")]
    pub vlan_enabled: bool,
    /// Gateway IP + prefix, e.g. "192.168.1.1/24".
    #[serde(deserialize_with = "nd")]
    pub ip_subnet: String,
    #[serde(deserialize_with = "nd")]
    pub dhcpd_enabled: bool,
    #[serde(deserialize_with = "nd")]
    pub dhcp_relay_enabled: bool,
    #[serde(deserialize_with = "nd")]
    pub dhcpd_start: String,
    #[serde(deserialize_with = "nd")]
    pub dhcpd_stop: String,
    #[serde(rename = "dhcpd_dns_1", deserialize_with = "nd")]
    pub dhcpd_dns_1: String,
    #[serde(rename = "dhcpd_dns_2", deserialize_with = "nd")]
    pub dhcpd_dns_2: String,
    /// "WAN" → wan1, "WAN2" → wan2.
    #[serde(deserialize_with = "nd")]
    pub wan_networkgroup: String,
    #[serde(deserialize_with = "nd")]
    pub wan_dns1: String,
    #[serde(deserialize_with = "nd")]
    pub wan_dns2: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_fields_decode_to_defaults() {
        let n: NetworkConf = serde_json::from_str(
            r#"{"_id":"x","name":"Internet 1","purpose":"wan","vlan":null,
                "vlan_enabled":null,"networkgroup":null,"wan_networkgroup":"WAN"}"#,
        )
        .unwrap();
        assert!(!n.vlan_enabled);
        assert_eq!(n.networkgroup, "");
        assert!(n.vlan.is_null());
        assert_eq!(n.wan_networkgroup, "WAN");
    }

    #[test]
    fn missing_fields_decode_to_defaults() {
        let d: Device = serde_json::from_str(r#"{"name":"x","port_table":null}"#).unwrap();
        assert_eq!(d.name, "x");
        assert!(d.port_table.is_empty());
        assert!(d.uplink.is_none());
        assert_eq!(d.state, 0);
    }

    #[test]
    fn envelope_with_null_data_is_empty() {
        let e: Envelope<Vec<Device>> =
            serde_json::from_str(r#"{"meta":{"rc":"ok"},"data":null}"#).unwrap();
        assert!(e.data.is_empty());
    }
}
