//! Raw UniFi controller payloads, ported field-for-field from
//! homelab-api/internal/adapters/unifi.go. Go's encoding/json turns `null`
//! into a zero value and ignores missing keys; every non-`Option` field here
//! uses `nd` + container `default` to behave the same way.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};
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

/// One entry of `/api/self/sites`: `name` is the short ID used in URLs and
/// config, `desc` the name shown in the UI.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Site {
    #[serde(deserialize_with = "nd")]
    pub name: String,
    #[serde(deserialize_with = "nd")]
    pub desc: String,
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
    /// Experience score 0–100; -1 (or absent) when the device has none.
    pub satisfaction: Option<i64>,
    #[serde(deserialize_with = "nd")]
    pub port_table: Vec<PortEntry>,
    #[serde(deserialize_with = "nd")]
    pub vap_table: Vec<Vap>,
    pub uplink: Option<Uplink>,
    pub wan1: Option<WanIface>,
    pub wan2: Option<WanIface>,
    /// Gateways only, keyed by `wan_networkgroup` ("WAN", "WAN2").
    #[serde(deserialize_with = "nd")]
    pub uptime_stats: BTreeMap<String, WanUptimeStats>,
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
    /// The port's maximum: "FE", "GE", "2.5GE"/"2P5GE", "10GE", "SFP+", …
    #[serde(deserialize_with = "nd")]
    pub media: String,
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
    /// This device's uplink port maximum, Mbps.
    pub max_speed: Option<i64>,
    pub full_duplex: Option<bool>,
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

/// One WAN's entry in a gateway's `uptime_stats`. `uptime`, `availability`
/// and `latency_average` are present only while the WAN is up, `downtime`
/// only while it is down.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct WanUptimeStats {
    /// Seconds since this WAN came up.
    pub uptime: Option<i64>,
    /// Seconds since this WAN went down.
    pub downtime: Option<i64>,
    /// Percent over `time_period`.
    pub availability: Option<f64>,
    /// Milliseconds, averaged over `time_period`.
    pub latency_average: Option<f64>,
    /// Seconds.
    pub time_period: Option<i64>,
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
    /// 0–100, wireless clients only.
    pub wifi_experience_score: Option<f64>,
    /// Wi-Fi PHY rate, AP → client, kbps.
    pub tx_rate: Option<f64>,
    pub wired_rate_mbps: Option<i64>,
    /// Network-side counters: `tx` is what the network sent to the client.
    #[serde(rename = "rx_bytes-r")]
    pub rx_bytes_r: Option<f64>,
    #[serde(rename = "tx_bytes-r")]
    pub tx_bytes_r: Option<f64>,
    /// Upstream switch port (active), and the last one (history).
    pub sw_port: Option<i64>,
    pub last_uplink_remote_port: Option<i64>,
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

/// One page of `POST /v2/api/site/{site}/system-log/all`, newest first.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SystemLogPage {
    #[serde(deserialize_with = "nd")]
    pub data: Vec<SystemLogEntry>,
    #[serde(deserialize_with = "nd")]
    pub total_page_count: i64,
}

/// A system log entry. `message_raw` and `title_raw` are templates whose
/// `{KEY}` placeholders name entries of `parameters`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SystemLogEntry {
    #[serde(deserialize_with = "nd")]
    pub id: String,
    /// Epoch milliseconds.
    #[serde(deserialize_with = "nd")]
    pub timestamp: i64,
    /// LOW, MEDIUM, HIGH, VERY_HIGH, WARNING, INFO
    #[serde(deserialize_with = "nd")]
    pub severity: String,
    #[serde(deserialize_with = "nd")]
    pub category: String,
    #[serde(deserialize_with = "nd")]
    pub subcategory: String,
    #[serde(deserialize_with = "nd")]
    pub event: String,
    #[serde(deserialize_with = "nd")]
    pub title_raw: String,
    #[serde(deserialize_with = "nd")]
    pub message_raw: String,
    #[serde(deserialize_with = "nd")]
    pub parameters: BTreeMap<String, LogParam>,
}

/// A template parameter, kept as raw JSON: the controller sends `id` and
/// `name` as strings or numbers, and sometimes a whole parameter as `null`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(transparent)]
pub struct LogParam(Value);

impl LogParam {
    /// Usually a MAC for devices and clients.
    pub fn id(&self) -> Option<String> {
        scalar(self.0.get("id"))
    }

    /// The display value; `None` when missing, `null` or empty.
    pub fn name(&self) -> Option<String> {
        scalar(self.0.get("name"))
    }
}

fn scalar(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Request body for the system log. Omitted times mean the whole retained
/// log; empty filters are left out.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp_from: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp_to: Option<i64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub severities: Vec<&'static str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub categories: Vec<&'static str>,
    pub page_size: i64,
    pub page_number: i64,
}

/// `/v2/api/site/{site}/traffic`: traffic per client and application over a
/// window, and the same per application across the site.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct TrafficReport {
    #[serde(deserialize_with = "nd")]
    pub client_usage_by_app: Vec<ClientUsage>,
    #[serde(deserialize_with = "nd")]
    pub total_usage_by_app: Vec<AppUsage>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ClientUsage {
    #[serde(deserialize_with = "nd")]
    pub client: TrafficClient,
    #[serde(deserialize_with = "nd")]
    pub usage_by_app: Vec<AppUsage>,
}

/// The client a traffic entry belongs to; `name` may be empty.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct TrafficClient {
    #[serde(deserialize_with = "nd")]
    pub mac: String,
    pub name: Option<String>,
    pub hostname: Option<String>,
    #[serde(deserialize_with = "nd")]
    pub is_wired: bool,
}

/// Traffic of one DPI application. `bytes_received` is download from the
/// client's point of view. `activity_seconds` is set per client,
/// `client_count` in the site totals.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct AppUsage {
    #[serde(deserialize_with = "nd")]
    pub category: i64,
    #[serde(deserialize_with = "nd")]
    pub application: i64,
    #[serde(deserialize_with = "nd")]
    pub bytes_received: i64,
    #[serde(deserialize_with = "nd")]
    pub bytes_transmitted: i64,
    #[serde(deserialize_with = "nd")]
    pub total_bytes: i64,
    #[serde(deserialize_with = "nd")]
    pub activity_seconds: i64,
    #[serde(deserialize_with = "nd")]
    pub client_count: i64,
}

/// One page of an Integration API list.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct DpiPage {
    #[serde(rename = "totalCount", deserialize_with = "nd")]
    pub total_count: i64,
    #[serde(deserialize_with = "nd")]
    pub data: Vec<DpiEntry>,
}

/// A DPI category (`id` = traffic `category`) or application
/// (`id` = `category << 16 | application`).
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct DpiEntry {
    #[serde(deserialize_with = "nd")]
    pub id: i64,
    #[serde(deserialize_with = "nd")]
    pub name: String,
}

/// `/v2/api/site/{site}/aggregated-dashboard?widgets=wan_history`: the web
/// dashboard's WAN history. Other widgets, sent by controllers that ignore
/// `widgets`, are skipped.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct WanHistory {
    pub dashboard_meta: Option<DashboardMeta>,
    pub wan_history: Option<WanHistoryWidget>,
}

/// The window the controller actually covered, epoch milliseconds. It starts
/// at or after the requested one (aligned to a bucket, or as far back as
/// history goes).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct DashboardMeta {
    #[serde(deserialize_with = "nd")]
    pub start_timestamp: i64,
    #[serde(deserialize_with = "nd")]
    pub end_timestamp: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct WanHistoryWidget {
    #[serde(deserialize_with = "nd")]
    pub wan_history_details: Vec<WanHistoryDetail>,
}

/// One WAN's history. A WAN that has never been up has no entry.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct WanHistoryDetail {
    /// `WAN`, `WAN2`: the network's `wan_networkgroup`.
    #[serde(deserialize_with = "nd")]
    pub network_group: String,
    /// Availability in percent over the window.
    pub uptime: Option<f64>,
    #[serde(deserialize_with = "nd")]
    pub downtime_history: Vec<WanDowntime>,
}

/// One outage: start in epoch milliseconds and its length.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct WanDowntime {
    #[serde(deserialize_with = "nd")]
    pub timestamp: i64,
    #[serde(deserialize_with = "nd")]
    pub downtime_in_seconds: i64,
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

    #[test]
    fn traffic_report_tolerates_nulls_and_missing_keys() {
        let r: TrafficReport = serde_json::from_str(
            r#"{"client_usage_by_app": [{"client": {"mac": "aa", "name": null, "is_wired": null},
                "usage_by_app": null}], "total_usage_by_app": null}"#,
        )
        .unwrap();
        assert_eq!(r.client_usage_by_app.len(), 1);
        assert_eq!(r.client_usage_by_app[0].client.name, None);
        assert!(!r.client_usage_by_app[0].client.is_wired);
        assert!(r.client_usage_by_app[0].usage_by_app.is_empty());
        assert!(r.total_usage_by_app.is_empty());
        let p: DpiPage =
            serde_json::from_str(r#"{"totalCount": 2, "data": [{"id": 1, "name": "X"}]}"#).unwrap();
        assert_eq!(p.total_count, 2);
        assert_eq!(
            p.data,
            vec![DpiEntry {
                id: 1,
                name: "X".into()
            }]
        );
    }

    #[test]
    fn satisfaction_null_or_missing_is_none() {
        let d: Device = serde_json::from_str(r#"{"satisfaction": null}"#).unwrap();
        assert_eq!(d.satisfaction, None);
        let d: Device = serde_json::from_str("{}").unwrap();
        assert_eq!(d.satisfaction, None);
        let d: Device = serde_json::from_str(r#"{"satisfaction": 93}"#).unwrap();
        assert_eq!(d.satisfaction, Some(93));
    }

    #[test]
    fn client_v2_tolerates_floats_and_nulls() {
        let c: ClientV2 = serde_json::from_str(
            r#"{"tx_rate": 866.7, "rx_bytes-r": 12.5, "tx_bytes-r": null,
                "wifi_experience_score": null, "sw_port": null}"#,
        )
        .unwrap();
        assert_eq!(c.tx_rate, Some(866.7));
        assert_eq!(c.rx_bytes_r, Some(12.5));
        assert_eq!(c.tx_bytes_r, None);
        assert_eq!(c.wifi_experience_score, None);
        let c: ClientV2 = serde_json::from_str(r#"{"wifi_experience_score": 91.0}"#).unwrap();
        assert_eq!(c.wifi_experience_score, Some(91.0));
    }

    #[test]
    fn wan_history_decodes_and_tolerates_nulls() {
        let h: WanHistory = serde_json::from_str(
            r#"{"dashboard_meta": {"start_timestamp": 1000, "end_timestamp": 2000, "widgets": ["wan_history"]},
                "wan_history": {"wan_history_details": [
                  {"network_group": "WAN", "isp_name": "Example ISP", "isp_asn": 64500, "uptime": 99.95,
                   "downtime_history": [{"id": "x", "timestamp": 1500, "downtime_in_seconds": 95, "wan_network_group": "WAN"}],
                   "health_history": [{"timestamp": 1000, "wan_downtime": false}]},
                  {"network_group": "WAN2", "uptime": null, "downtime_history": null}
                ]}}"#,
        )
        .unwrap();
        let meta = h.dashboard_meta.unwrap();
        assert_eq!((meta.start_timestamp, meta.end_timestamp), (1000, 2000));
        let d = &h.wan_history.as_ref().unwrap().wan_history_details;
        assert_eq!(d[0].network_group, "WAN");
        assert_eq!(d[0].uptime, Some(99.95));
        assert_eq!(d[0].downtime_history[0].timestamp, 1500);
        assert_eq!(d[0].downtime_history[0].downtime_in_seconds, 95);
        assert_eq!(d[1].uptime, None);
        assert!(d[1].downtime_history.is_empty());
    }

    #[test]
    fn wan_history_ignores_other_widgets() {
        let h: WanHistory = serde_json::from_str(
            r#"{"radio_activity": {"x": [1, 2]}, "wan": {"y": 1},
                "dashboard_meta": {"start_timestamp": 1, "end_timestamp": 2},
                "wan_history": {"wan_history_details": [{"network_group": "WAN", "uptime": 100}]}}"#,
        )
        .unwrap();
        assert_eq!(
            h.wan_history.unwrap().wan_history_details[0].uptime,
            Some(100.0)
        );
    }

    #[test]
    fn wan_history_missing_parts_are_none() {
        let h: WanHistory = serde_json::from_str("{}").unwrap();
        assert!(h.dashboard_meta.is_none());
        assert!(h.wan_history.is_none());
        let h: WanHistory =
            serde_json::from_str(r#"{"dashboard_meta": null, "wan_history": null}"#).unwrap();
        assert!(h.dashboard_meta.is_none() && h.wan_history.is_none());
    }
}
