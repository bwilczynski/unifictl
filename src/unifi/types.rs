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

/// A number however the controller sends it: an integer, a float or a
/// numeric string. Anything else (`"auto"`, `"VHT80"`, `null`) is `None`, so
/// one odd radio field never fails the whole `stat/device` response.
fn lenient_f64<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    Ok(match Option::<Value>::deserialize(d)? {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => s.trim().parse().ok(),
        _ => None,
    }
    .filter(|f: &f64| f.is_finite()))
}

/// `lenient_f64`, rounded to the nearest integer.
fn lenient_i64<'de, D: Deserializer<'de>>(d: D) -> Result<Option<i64>, D::Error> {
    Ok(lenient_f64(d)?.map(|f| f.round() as i64))
}

/// `true` only for a JSON `true`.
fn lenient_bool<'de, D: Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    Ok(matches!(
        Option::<Value>::deserialize(d)?,
        Some(Value::Bool(true))
    ))
}

/// `lenient_string`, empty when absent.
fn lenient_text<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(lenient_string(d)?.unwrap_or_default())
}

/// The elements of an array that parse as `T`; anything that is not an array
/// is empty, so one malformed radio never fails the whole device list.
fn lenient_vec<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    Ok(match Option::<Value>::deserialize(d)? {
        Some(Value::Array(items)) => items
            .into_iter()
            .filter_map(|v| T::deserialize(v).ok())
            .collect(),
        _ => Vec::new(),
    })
}

/// A non-empty string or number as text; anything else is `None`.
fn lenient_string<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Ok(scalar(Option::<Value>::deserialize(d)?.as_ref()))
}

/// `{"meta": {...}, "data": [...]}` wrapper used by the /api/s/{site} endpoints.
/// Read `data` through [`Envelope::into_data`], which rejects API errors.
#[derive(Debug, Deserialize)]
#[serde(bound(deserialize = "T: Deserialize<'de> + Default"))]
pub struct Envelope<T: Default> {
    #[serde(default, deserialize_with = "nd")]
    meta: Meta,
    #[serde(default, deserialize_with = "nd")]
    data: T,
}

/// The controller's verdict on a request: `rc` is `ok` or `error`, and an
/// error carries a message such as `api.err.NoPermission`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Meta {
    #[serde(deserialize_with = "nd")]
    rc: String,
    #[serde(deserialize_with = "lenient_string")]
    msg: Option<String>,
}

impl<T: Default> Envelope<T> {
    /// `data`, or the controller's message when it reported an error (an
    /// HTTP 200 can still carry one).
    pub fn into_data(self) -> Result<T, String> {
        if self.meta.rc == "error" {
            return Err(self.meta.msg.unwrap_or_else(|| "unknown error".into()));
        }
        Ok(self.data)
    }
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

/// `stat/sysinfo`: the controller's name and versions.
/// `console_display_version` is the UniFi OS version, empty on a legacy
/// controller. Its `update_available` is the Network app's and is not read.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct SysInfo {
    #[serde(deserialize_with = "nd")]
    pub name: String,
    #[serde(deserialize_with = "nd")]
    pub version: String,
    #[serde(deserialize_with = "nd")]
    pub console_display_version: String,
}

/// UniFi OS `/api/system`: the console itself, outside the Network app.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct ConsoleSystem {
    #[serde(deserialize_with = "nd")]
    pub name: String,
    #[serde(rename = "deviceState", deserialize_with = "nd")]
    pub device_state: String,
}

impl ConsoleSystem {
    /// Whether a UniFi OS update is waiting to be installed.
    pub fn update_available(&self) -> bool {
        self.device_state == "updateAvailable"
    }
}

/// One entry of `stat/health`: a subsystem (`wan`, `www`, `lan`, `wlan`,
/// `vpn`) and the controller's verdict on it (`ok`, `warning`, `error`,
/// `unknown`). The counts it also carries are not used.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct HealthSubsystem {
    #[serde(deserialize_with = "nd")]
    pub subsystem: String,
    #[serde(deserialize_with = "nd")]
    pub status: String,
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
    /// Whether the controller has newer firmware for this device.
    #[serde(deserialize_with = "nd")]
    pub upgradable: bool,
    /// The version `upgradable` refers to; empty when there is none.
    #[serde(deserialize_with = "nd")]
    pub upgrade_to_firmware: String,
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
    #[serde(deserialize_with = "lenient_vec")]
    pub radio_table: Vec<RadioEntry>,
    #[serde(deserialize_with = "lenient_vec")]
    pub radio_table_stats: Vec<RadioStats>,
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

/// One row of an access point's `radio_table`: a radio's configuration.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct RadioEntry {
    /// Interface name, `wifi0`, `wifi1`, …; the key into `radio_table_stats`.
    #[serde(deserialize_with = "lenient_text")]
    pub name: String,
    /// Band code: `ng` (2.4 GHz), `na` (5 GHz), `6e` (6 GHz).
    #[serde(deserialize_with = "lenient_text")]
    pub radio: String,
    #[serde(deserialize_with = "lenient_i64")]
    pub channel: Option<i64>,
    /// Channel width in MHz, sent as a string (`"20"`, `"80"`).
    #[serde(deserialize_with = "lenient_i64")]
    pub ht: Option<i64>,
    #[serde(deserialize_with = "lenient_string")]
    pub tx_power_mode: Option<String>,
    #[serde(deserialize_with = "lenient_bool")]
    pub min_rssi_enabled: bool,
    #[serde(deserialize_with = "lenient_i64")]
    pub min_rssi: Option<i64>,
}

/// One row of an access point's `radio_table_stats`: a radio's live figures.
/// Channel utilisation (`cu_*`) is percent of airtime.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct RadioStats {
    #[serde(deserialize_with = "lenient_text")]
    pub name: String,
    #[serde(deserialize_with = "lenient_i64")]
    pub channel: Option<i64>,
    #[serde(deserialize_with = "lenient_i64")]
    pub cu_total: Option<i64>,
    #[serde(deserialize_with = "lenient_i64")]
    pub cu_self_rx: Option<i64>,
    #[serde(deserialize_with = "lenient_i64")]
    pub cu_self_tx: Option<i64>,
    #[serde(deserialize_with = "lenient_f64")]
    pub tx_retries_pct: Option<f64>,
    #[serde(deserialize_with = "lenient_i64")]
    pub num_sta: Option<i64>,
    #[serde(deserialize_with = "lenient_i64")]
    pub satisfaction: Option<i64>,
    /// dBm.
    #[serde(deserialize_with = "lenient_i64")]
    pub tx_power: Option<i64>,
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
    fn envelope_rejects_api_errors() {
        let env: Envelope<Vec<Device>> = serde_json::from_str(
            r#"{"meta":{"rc":"error","msg":"api.err.NoPermission"},"data":[]}"#,
        )
        .unwrap();
        assert_eq!(env.into_data().unwrap_err(), "api.err.NoPermission");

        let env: Envelope<Vec<Device>> =
            serde_json::from_str(r#"{"meta":{"rc":"error"}}"#).unwrap();
        assert_eq!(env.into_data().unwrap_err(), "unknown error");

        for body in [
            r#"{"meta":{"rc":"ok"},"data":[{"name":"x"}]}"#,
            r#"{"meta":null,"data":[{"name":"x"}]}"#,
            r#"{"data":[{"name":"x"}]}"#,
        ] {
            let env: Envelope<Vec<Device>> = serde_json::from_str(body).unwrap();
            assert_eq!(env.into_data().unwrap()[0].name, "x", "{body}");
        }
    }

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
    fn radio_fields_tolerate_odd_types() {
        let d: Device = serde_json::from_str(
            r#"{"name":"ap","radio_table":[
                {"name":"wifi0","radio":"ng","channel":"auto","ht":"20","tx_power_mode":"auto",
                 "min_rssi_enabled":true,"min_rssi":-75},
                {"name":"wifi1","radio":"na","channel":36,"ht":"VHT80","tx_power_mode":null,
                 "min_rssi_enabled":null}],
              "radio_table_stats":[
                {"name":"wifi0","channel":"6","cu_total":71.6,"cu_self_rx":"20","cu_self_tx":null,
                 "tx_retries_pct":"14.2","num_sta":9,"satisfaction":-1,"tx_power":17}]}"#,
        )
        .unwrap();
        let (a, b) = (&d.radio_table[0], &d.radio_table[1]);
        assert_eq!((a.name.as_str(), a.radio.as_str()), ("wifi0", "ng"));
        assert_eq!((a.channel, a.ht), (None, Some(20)));
        assert_eq!(a.tx_power_mode.as_deref(), Some("auto"));
        assert!(a.min_rssi_enabled);
        assert_eq!(a.min_rssi, Some(-75));
        assert_eq!(
            (
                b.channel,
                b.ht,
                b.tx_power_mode.as_deref(),
                b.min_rssi_enabled
            ),
            (Some(36), None, None, false)
        );
        let s = &d.radio_table_stats[0];
        assert_eq!(
            (s.channel, s.cu_total, s.cu_self_rx, s.cu_self_tx),
            (Some(6), Some(72), Some(20), None)
        );
        assert_eq!(s.tx_retries_pct, Some(14.2));
        assert_eq!(
            (s.num_sta, s.satisfaction, s.tx_power),
            (Some(9), Some(-1), Some(17))
        );
    }

    #[test]
    fn malformed_radio_tables_do_not_fail_the_device() {
        for json in [
            r#"{"name":"ap","radio_table":{},"radio_table_stats":"none"}"#,
            r#"{"name":"ap","radio_table":["x",null,{"name":0,"radio":"ng"}],
                "radio_table_stats":[{"name":1,"cu_total":5}]}"#,
        ] {
            let d: Device = serde_json::from_str(json).unwrap_or_else(|e| panic!("{json}: {e}"));
            assert_eq!(d.name, "ap");
            for r in &d.radio_table {
                assert_eq!(r.radio, "ng");
                assert_eq!(r.name, "0");
            }
        }
        let d: Device = serde_json::from_str(
            r#"{"name":"ap","radio_table":["x",{"name":"wifi0","radio":"ng"}]}"#,
        )
        .unwrap();
        assert_eq!(d.radio_table.len(), 1, "non-object rows are skipped");
    }

    #[test]
    fn device_without_radios_has_empty_tables() {
        let d: Device = serde_json::from_str(r#"{"name":"sw","radio_table":null}"#).unwrap();
        assert!(d.radio_table.is_empty());
        assert!(d.radio_table_stats.is_empty());
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
    fn firmware_fields_default_when_missing() {
        let d: Device = serde_json::from_str("{}").unwrap();
        assert!(!d.upgradable);
        assert_eq!(d.upgrade_to_firmware, "");
        let d: Device =
            serde_json::from_str(r#"{"upgradable": true, "upgrade_to_firmware": "7.1.68"}"#)
                .unwrap();
        assert!(d.upgradable);
        assert_eq!(d.upgrade_to_firmware, "7.1.68");
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

    #[test]
    fn console_system_update_available_only_on_exact_state() {
        let s: ConsoleSystem =
            serde_json::from_str(r#"{"name": "console-01", "deviceState": "updateAvailable"}"#)
                .unwrap();
        assert!(s.update_available());
        assert_eq!(s.name, "console-01");
        for state in [
            r#"{"deviceState": "ready"}"#,
            r#"{"deviceState": "updating"}"#,
            r#"{"deviceState": null}"#,
            r#"{}"#,
        ] {
            let s: ConsoleSystem = serde_json::from_str(state).unwrap();
            assert!(!s.update_available(), "{state}");
        }
    }

    #[test]
    fn sysinfo_fields_default_when_missing() {
        let s: SysInfo =
            serde_json::from_str(r#"{"name": "controller-01", "version": "9.4.19"}"#).unwrap();
        assert_eq!(
            (s.name.as_str(), s.version.as_str()),
            ("controller-01", "9.4.19")
        );
        assert_eq!(s.console_display_version, "");
    }
}
