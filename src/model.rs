//! Output model: homelab-api's network schemas without `uri`, with IDs that
//! carry no controller prefix. Serialize-only; JSON is camelCase.

use std::collections::BTreeMap;

use serde::{Serialize, Serializer};

macro_rules! str_enum {
    ($name:ident { $($variant:ident => $s:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $name { $($variant),+ }

        impl $name {
            pub fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $s),+ }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(self.as_str())
            }
        }
    };
}

str_enum!(DeviceType { AccessPoint => "accessPoint", Switch => "switch", Gateway => "gateway", Unknown => "unknown" });
str_enum!(DeviceStatus { Connected => "connected", Disconnected => "disconnected" });
str_enum!(PortState { Up => "up", Down => "down", Disabled => "disabled" });
str_enum!(LinkSpeed { E => "e", Fe => "fe", Gbe1 => "gbe1", Gbe2p5 => "gbe2_5", Gbe5 => "gbe5", Gbe10 => "gbe10" });
str_enum!(PoeMode { Auto => "auto", Off => "off", Passive24v => "passive24v", Passthrough => "passthrough" });
str_enum!(VlanMode { Access => "access", Trunk => "trunk" });
str_enum!(TaggedScope { All => "all", Custom => "custom" });
str_enum!(LagRole { Master => "master", Member => "member" });
str_enum!(ConnectionType { Wired => "wired", Wireless => "wireless" });
str_enum!(ClientStatus { Online => "online", Offline => "offline" });
str_enum!(DhcpMode { Server => "server", Relay => "relay", Disabled => "disabled" });
str_enum!(WanStatus { Connected => "connected", Disconnected => "disconnected" });
str_enum!(WifiBand { Band2g => "band2g", Band5g => "band5g", Band6g => "band6g" });
str_enum!(SecurityProtocol { Open => "open", Wpa2 => "wpa2", Wpa3 => "wpa3", Wpa2Wpa3 => "wpa2Wpa3" });

impl LinkSpeed {
    /// Nominal speed in Mbps.
    pub fn mbps(self) -> i64 {
        match self {
            Self::E => 10,
            Self::Fe => 100,
            Self::Gbe1 => 1000,
            Self::Gbe2p5 => 2500,
            Self::Gbe5 => 5000,
            Self::Gbe10 => 10000,
        }
    }
}
str_enum!(LogSeverity { Info => "info", Low => "low", Warning => "warning", Medium => "medium", High => "high", VeryHigh => "veryHigh", Unknown => "unknown" });

impl LogSeverity {
    /// The controller's SCREAMING_SNAKE value; anything new is `Unknown`.
    pub fn from_controller(s: &str) -> Self {
        match s {
            "INFO" => Self::Info,
            "LOW" => Self::Low,
            "WARNING" => Self::Warning,
            "MEDIUM" => Self::Medium,
            "HIGH" => Self::High,
            "VERY_HIGH" => Self::VeryHigh,
            _ => Self::Unknown,
        }
    }
}

/// Go serializes a nil slice as `null`; used where Go can produce one.
#[allow(clippy::ptr_arg)]
fn empty_as_null<T: Serialize, S: Serializer>(v: &Vec<T>, s: S) -> Result<S::Ok, S::Error> {
    if v.is_empty() {
        s.serialize_none()
    } else {
        v.serialize(s)
    }
}

fn opt_empty_as_null<T: Serialize, S: Serializer>(
    v: &Option<Vec<T>>,
    s: S,
) -> Result<S::Ok, S::Error> {
    match v {
        Some(items) if !items.is_empty() => items.serialize(s),
        _ => s.serialize_none(),
    }
}

// ---------- refs ----------

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct DeviceRef {
    pub kind: &'static str,
    pub id: String,
    pub name: String,
}

impl DeviceRef {
    pub fn new(id: String, name: String) -> Self {
        Self {
            kind: "device",
            id,
            name,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ClientRef {
    pub kind: &'static str,
    pub id: String,
    pub name: String,
}

impl ClientRef {
    pub fn new(id: String, name: String) -> Self {
        Self {
            kind: "client",
            id,
            name,
        }
    }
}

/// `NetworkConnectionRef`: a device or a client, discriminated on `kind`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ConnectionRef {
    Device(DeviceRef),
    Client(ClientRef),
}

impl ConnectionRef {
    pub fn id(&self) -> &str {
        match self {
            Self::Device(d) => &d.id,
            Self::Client(c) => &c.id,
        }
    }
    pub fn name(&self) -> &str {
        match self {
            Self::Device(d) => &d.name,
            Self::Client(c) => &c.name,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VlanRef {
    pub id: String,
    pub name: String,
    pub vlan_id: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WanRef {
    pub id: String,
    pub name: String,
}

// ---------- devices ----------

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Traffic {
    pub rx_bytes_total: i64,
    pub tx_bytes_total: i64,
    pub rx_bytes_per_sec: i64,
    pub tx_bytes_per_sec: i64,
}

/// `NetworkConnection`: where a device or wired client attaches upstream.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    pub device: DeviceRef,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_speed: Option<LinkSpeed>,
    /// What both ends of a device uplink allow; not set for clients.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_link_speed: Option<LinkSpeed>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_duplex: Option<bool>,
}

/// A device as `devices list` shows it; every `NetworkDeviceDetail` variant
/// flattens it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkDevice {
    pub id: String,
    pub name: String,
    pub mac: String,
    pub ip: String,
    #[serde(rename = "type")]
    pub device_type: DeviceType,
    pub status: DeviceStatus,
    pub model: String,
    pub firmware_version: String,
    pub uptime: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experience: Option<i64>,
    pub traffic: Traffic,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uplink: Option<Connection>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LagMembership {
    pub id: i64,
    pub role: LagRole,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TaggedVlans {
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "opt_empty_as_null"
    )]
    pub items: Option<Vec<VlanRef>>,
    pub scope: TaggedScope,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VlanConfig {
    pub mode: VlanMode,
    pub native_vlan: VlanRef,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tagged_vlans: Option<TaggedVlans>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DevicePort {
    pub number: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub state: PortState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_speed: Option<LinkSpeed>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_uptime: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sfp_module_present: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub poe_mode: Option<PoeMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub poe_power_watts: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lag_membership: Option<LagMembership>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vlan_config: Option<VlanConfig>,
    pub traffic: Traffic,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connected_to: Option<ConnectionRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SwitchDetail {
    #[serde(flatten)]
    pub base: NetworkDevice,
    pub ports: Vec<DevicePort>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GatewayDetail {
    #[serde(flatten)]
    pub base: NetworkDevice,
    pub ports: Vec<DevicePort>,
    pub wans: Vec<WanRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApClient {
    pub client: ClientRef,
    pub ssid: String,
    pub signal_strength: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessPointDetail {
    #[serde(flatten)]
    pub base: NetworkDevice,
    pub num_clients: i64,
    pub connected_clients: Vec<ApClient>,
    pub radios: Vec<Radio>,
}

/// Discriminated on `type` (carried in `DeviceBase`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum NetworkDeviceDetail {
    Switch(SwitchDetail),
    AccessPoint(AccessPointDetail),
    Gateway(GatewayDetail),
    Unknown(NetworkDevice),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NetworkDeviceList {
    pub items: Vec<NetworkDevice>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NetworkPort {
    #[serde(flatten)]
    pub port: DevicePort,
    pub device: DeviceRef,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NetworkPortList {
    pub items: Vec<NetworkPort>,
}

// ---------- radios ----------

/// One Wi-Fi radio of an access point: its configuration and, while the AP
/// is connected, its live figures. Every figure is `null` when unknown.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Radio {
    /// `<device-id>/<band>` (`ap-office/2.4g`), or `<device-id>/<name>` when
    /// the AP has two radios on one band.
    pub id: String,
    /// Interface name: `wifi0`, `wifi1`, …
    pub name: String,
    /// `2.4G`, `5G`, `6G`, or the controller's code for anything else.
    pub band: String,
    pub channel: Option<i64>,
    /// Channel width, MHz.
    pub width: Option<i64>,
    pub tx_power_mode: Option<String>,
    /// dBm.
    pub tx_power: Option<i64>,
    /// dBm; only when the minimum-RSSI kick is enabled.
    pub min_rssi: Option<i64>,
    /// Percent of airtime the channel is busy.
    pub utilisation: Option<i64>,
    /// Percent of airtime used by others: utilisation minus own RX and TX.
    pub interference: Option<i64>,
    /// Percent of transmissions retried.
    pub retries: Option<f64>,
    pub clients: Option<i64>,
    /// The radio's experience score, 0–100.
    pub satisfaction: Option<i64>,
}

/// A radio with the access point it belongs to (`radios list`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NetworkRadio {
    #[serde(flatten)]
    pub radio: Radio,
    pub device: DeviceRef,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NetworkRadioList {
    pub items: Vec<NetworkRadio>,
}

// ---------- clients ----------

/// Live traffic rates from the client's point of view: `rx` is its download.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Rates {
    pub rx_bytes_per_sec: i64,
    pub tx_bytes_per_sec: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WirelessConnection {
    pub device: DeviceRef,
    pub ssid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal_strength: Option<i64>,
    /// Wi-Fi PHY rate, Mbps.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_mbps: Option<i64>,
}

/// Where a client attaches: a switch port or an access point.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ClientConnection {
    Wired(Connection),
    Wireless(WirelessConnection),
}

/// A client, the same in `clients list` and `clients get`. Live fields are
/// only set while it is online.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkClient {
    pub id: String,
    pub name: String,
    pub mac: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip: Option<String>,
    pub connection_type: ConnectionType,
    pub status: ClientStatus,
    pub connected_to: ClientConnection,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experience: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub traffic: Option<Rates>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uptime: Option<i64>,
    /// Epoch seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NetworkClientList {
    pub items: Vec<NetworkClient>,
}

// ---------- topology ----------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TopologyDeviceNode {
    pub kind: &'static str,
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub device_type: DeviceType,
    pub status: DeviceStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_clients: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TopologyClientNode {
    pub kind: &'static str,
    pub id: String,
    pub name: String,
    pub connection_type: ConnectionType,
    pub status: ClientStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum TopologyNode {
    Device(TopologyDeviceNode),
    Client(TopologyClientNode),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TopologyWiredEdge {
    pub kind: &'static str,
    pub source: ConnectionRef,
    pub target: DeviceRef,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_speed: Option<LinkSpeed>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TopologyWirelessEdge {
    pub kind: &'static str,
    pub source: ClientRef,
    pub target: DeviceRef,
    pub ssid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal_strength: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum TopologyEdge {
    Wired(TopologyWiredEdge),
    Wireless(TopologyWirelessEdge),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NetworkTopology {
    pub nodes: Vec<TopologyNode>,
    pub edges: Vec<TopologyEdge>,
}

// ---------- vlans / wans / ssids ----------

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VlanList {
    pub items: Vec<Vlan>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DhcpRange {
    pub start: String,
    pub end: String,
}

/// A VLAN, the same in `vlans list` and `vlans get`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Vlan {
    pub id: String,
    pub name: String,
    pub vlan_id: i64,
    pub subnet: String,
    pub gateway_ip: String,
    pub broadcast_ip: String,
    pub dhcp_mode: DhcpMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dhcp_range: Option<DhcpRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relay_server: Option<String>,
    pub dns_servers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WanList {
    pub items: Vec<Wan>,
}

/// A WAN, the same in `wans list` and `wans get`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Wan {
    pub id: String,
    pub name: String,
    pub ip_address: String,
    pub status: WanStatus,
    /// Seconds since this WAN came up; `None` while down or unreported.
    pub uptime: Option<i64>,
    /// Seconds since this WAN went down; `None` while up or unreported.
    pub downtime: Option<i64>,
    /// Percent over `availability_period`.
    pub availability: Option<f64>,
    /// Seconds.
    pub availability_period: Option<i64>,
    /// Average latency in milliseconds over `availability_period`.
    pub latency: Option<i64>,
    pub dns_servers: Vec<String>,
}

/// `wans get`: a WAN and its outage history.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WanDetail {
    #[serde(flatten)]
    pub wan: Wan,
    /// `None` when the controller reports no history for this WAN.
    pub outages: Option<WanOutages>,
}

/// Outages over the window the controller covered, which ends now.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WanOutages {
    /// RFC 3339 with the local offset.
    pub since: String,
    /// Percent over the window.
    pub availability: Option<f64>,
    /// Oldest first.
    pub items: Vec<WanOutage>,
    /// Epoch milliseconds, for the text renderer.
    #[serde(skip)]
    pub since_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WanOutage {
    /// RFC 3339 with the local offset.
    pub start: String,
    pub end: String,
    pub duration_seconds: i64,
    /// Epoch milliseconds, for the text renderer.
    #[serde(skip)]
    pub start_ms: i64,
    #[serde(skip)]
    pub end_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Ssid {
    pub id: String,
    pub name: String,
    pub bands: Vec<WifiBand>,
    pub vlan_id: i64,
    pub num_clients: i64,
    pub security_protocol: SecurityProtocol,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SsidList {
    pub items: Vec<Ssid>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SsidDetail {
    #[serde(flatten)]
    pub base: Ssid,
    pub clients: Vec<ClientRef>,
    #[serde(serialize_with = "empty_as_null")]
    pub broadcasting_aps: Vec<DeviceRef>,
}

// ---------- -o ids ----------

/// A DPI category: the controller's number and name.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DpiCategoryRef {
    pub id: i64,
    pub name: String,
}

/// A DPI application; `id` is `category << 16 | application`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AppRef {
    pub id: i64,
    pub name: String,
    pub category: DpiCategoryRef,
    /// False when `name` is a fallback (`Unknown (…)`, `Unidentified`).
    #[serde(skip)]
    pub named: bool,
}

/// A client's internet traffic over the window, with its largest application.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientTraffic {
    pub id: String,
    pub name: String,
    pub mac: String,
    pub bytes_received: i64,
    pub bytes_transmitted: i64,
    pub total_bytes: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_app: Option<AppRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClientTrafficList {
    pub items: Vec<ClientTraffic>,
}

/// One application's share of a client's traffic.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientAppTraffic {
    #[serde(flatten)]
    pub app: AppRef,
    pub bytes_received: i64,
    pub bytes_transmitted: i64,
    pub total_bytes: i64,
    pub activity_seconds: i64,
}

/// One client's traffic by application.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClientAppTrafficList {
    pub id: String,
    pub name: String,
    pub mac: String,
    pub items: Vec<ClientAppTraffic>,
}

/// One application's traffic across the site.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppTraffic {
    #[serde(flatten)]
    pub app: AppRef,
    pub client_count: i64,
    pub bytes_received: i64,
    pub bytes_transmitted: i64,
    pub total_bytes: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AppTrafficList {
    pub items: Vec<AppTraffic>,
}

/// List types whose items have an ID, for `-o ids`.
pub trait Ids {
    fn ids(&self) -> Vec<&str>;
}

macro_rules! impl_ids {
    ($($list:ty),+ $(,)?) => {
        $(impl Ids for $list {
            fn ids(&self) -> Vec<&str> {
                self.items.iter().map(|i| i.id.as_str()).collect()
            }
        })+
    };
}

impl_ids!(
    NetworkDeviceList,
    NetworkClientList,
    ClientTrafficList,
    VlanList,
    SsidList,
    WanList
);

impl Ids for NetworkRadioList {
    fn ids(&self) -> Vec<&str> {
        self.items.iter().map(|i| i.radio.id.as_str()).collect()
    }
}

/// A client named in a log entry; `id` is `None` for a client no longer in
/// the client list.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LogClientRef {
    pub kind: &'static str,
    pub id: Option<String>,
    pub name: String,
}

/// One system log entry, as `logs -o json` prints it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub id: String,
    /// RFC 3339 with milliseconds and the local offset.
    pub time: String,
    pub severity: LogSeverity,
    pub category: String,
    pub subcategory: String,
    pub event: String,
    pub title: String,
    pub message: String,
    pub devices: Vec<DeviceRef>,
    pub clients: Vec<LogClientRef>,
    pub details: BTreeMap<String, String>,
    /// Epoch milliseconds, for the text renderer.
    #[serde(skip)]
    pub timestamp_ms: i64,
}

/// `VERY_HIGH` → `veryHigh`.
pub fn screaming_to_camel(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for (i, word) in s.split('_').filter(|w| !w.is_empty()).enumerate() {
        let lower = word.to_lowercase();
        if i == 0 {
            out.push_str(&lower);
        } else {
            let mut chars = lower.chars();
            if let Some(first) = chars.next() {
                out.extend(first.to_uppercase());
                out.push_str(chars.as_str());
            }
        }
    }
    out
}

// ---------- status ----------

str_enum!(Verdict { Ok => "ok", Warn => "warn", Error => "error", Info => "info", Idle => "idle", Unknown => "unknown" });

impl Verdict {
    /// How bad, for combining checks: only warn and error rank above ok.
    fn rank(self) -> u8 {
        match self {
            Self::Error => 2,
            Self::Warn => 1,
            Self::Ok | Self::Info | Self::Idle | Self::Unknown => 0,
        }
    }

    /// The worse of the two; `self` on a tie.
    pub fn worse(self, other: Self) -> Self {
        if other.rank() > self.rank() {
            other
        } else {
            self
        }
    }
}

/// `unifictl status`: one row per area, in display order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SiteStatus {
    pub rows: Vec<StatusRow>,
}

impl SiteStatus {
    /// Whether any row is an error; `status` then exits 1.
    pub fn has_error(&self) -> bool {
        self.rows.iter().any(|r| r.verdict == Verdict::Error)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusRow {
    pub verdict: Verdict,
    /// Why the row is `unknown`: what could not be read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable: Option<String>,
    #[serde(flatten)]
    pub area: StatusArea,
}

/// The data behind a row; `area` names it in JSON.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    tag = "area",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum StatusArea {
    Wan {
        wan: Wan,
        /// Outages overlapping the last 24 hours, oldest first.
        outages_24h: Vec<WanOutage>,
    },
    Devices {
        online: usize,
        total: usize,
        offline: Vec<DeviceRef>,
        transitioning: Vec<DeviceTransition>,
    },
    Wifi {
        access_points: usize,
        /// Online wireless clients; `None` when they could not be read.
        clients: Option<usize>,
        /// Worst first.
        poor_clients: Vec<PoorClient>,
    },
    Wired {
        switches: usize,
        clients: Option<usize>,
    },
    Alarms {
        /// High and very-high log entries in the last 24 hours.
        count: Option<usize>,
        latest: Option<LogEntry>,
    },
    Updates {
        updates: Vec<FirmwareUpdate>,
    },
}

impl StatusArea {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Wan { .. } => "wan",
            Self::Devices { .. } => "devices",
            Self::Wifi { .. } => "wifi",
            Self::Wired { .. } => "wired",
            Self::Alarms { .. } => "alarms",
            Self::Updates { .. } => "updates",
        }
    }
}

/// A device on its way to connected: `pending`, `upgrading`, `provisioning`
/// or `adopting`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DeviceTransition {
    #[serde(flatten)]
    pub device: DeviceRef,
    pub state: &'static str,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PoorClient {
    pub id: String,
    pub name: String,
    pub experience: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FirmwareUpdate {
    #[serde(flatten)]
    pub device: DeviceRef,
    pub from: String,
    pub to: String,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn traffic() -> Traffic {
        Traffic::default()
    }

    fn port(number: i64) -> DevicePort {
        DevicePort {
            number,
            label: None,
            state: PortState::Down,
            link_speed: None,
            link_uptime: None,
            sfp_module_present: None,
            poe_mode: None,
            poe_power_watts: None,
            lag_membership: None,
            vlan_config: None,
            traffic: traffic(),
            connected_to: None,
        }
    }

    #[test]
    fn refs_carry_their_kind() {
        let r = DeviceRef::new("us-8".into(), "US 8".into());
        assert_eq!(
            serde_json::to_value(&r).unwrap(),
            json!({"kind": "device", "id": "us-8", "name": "US 8"})
        );
        let c = ConnectionRef::Client(ClientRef::new("nas-1-0002".into(), "nas-1".into()));
        assert_eq!(
            serde_json::to_value(&c).unwrap(),
            json!({"kind": "client", "id": "nas-1-0002", "name": "nas-1"})
        );
    }

    #[test]
    fn unresolved_device_ref_matches_go_zero_value() {
        let conn = Connection::default();
        assert_eq!(
            serde_json::to_value(&conn).unwrap(),
            json!({"device": {"kind": "", "id": "", "name": ""}})
        );
    }

    #[test]
    fn port_omits_absent_optionals() {
        assert_eq!(
            serde_json::to_value(port(3)).unwrap(),
            json!({"number": 3, "state": "down",
                   "traffic": {"rxBytesTotal": 0, "txBytesTotal": 0, "rxBytesPerSec": 0, "txBytesPerSec": 0}})
        );
    }

    #[test]
    fn tagged_vlans_items_follow_go_pointer_semantics() {
        let all = TaggedVlans {
            items: None,
            scope: TaggedScope::All,
        };
        assert_eq!(serde_json::to_value(&all).unwrap(), json!({"scope": "all"}));
        let custom_empty = TaggedVlans {
            items: Some(vec![]),
            scope: TaggedScope::Custom,
        };
        assert_eq!(
            serde_json::to_value(&custom_empty).unwrap(),
            json!({"items": null, "scope": "custom"})
        );
    }

    #[test]
    fn switch_detail_flattens_base() {
        let detail = NetworkDeviceDetail::Switch(SwitchDetail {
            base: NetworkDevice {
                id: "us-8".into(),
                name: "US 8".into(),
                mac: "aa:bb:cc:dd:01:01".into(),
                ip: "192.168.1.2".into(),
                device_type: DeviceType::Switch,
                status: DeviceStatus::Connected,
                model: "USC8".into(),
                firmware_version: "1.0".into(),
                uptime: 5,
                experience: None,
                traffic: traffic(),
                uplink: None,
            },
            ports: vec![],
        });
        let v = serde_json::to_value(&detail).unwrap();
        assert_eq!(v["type"], "switch");
        assert_eq!(v["firmwareVersion"], "1.0");
        assert_eq!(v["ports"], json!([]));
        assert!(v.get("uplink").is_none());
        assert!(v.get("experience").is_none(), "absent score is omitted");
        assert!(v.get("uri").is_none());
    }

    #[test]
    fn empty_broadcasting_aps_serialize_as_null() {
        let s = SsidDetail {
            base: Ssid {
                id: "x".into(),
                name: "x".into(),
                bands: vec![WifiBand::Band2g],
                vlan_id: 1,
                num_clients: 0,
                security_protocol: SecurityProtocol::Wpa2,
            },
            clients: vec![],
            broadcasting_aps: vec![],
        };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["broadcastingAps"], serde_json::Value::Null);
        assert_eq!(v["clients"], json!([]));
        assert_eq!(v["bands"], json!(["band2g"]));
        assert_eq!(v["securityProtocol"], "wpa2");
    }

    #[test]
    fn link_speed_uses_spec_values() {
        assert_eq!(LinkSpeed::Gbe2p5.as_str(), "gbe2_5");
        assert_eq!(serde_json::to_value(LinkSpeed::Fe).unwrap(), json!("fe"));
    }

    #[test]
    fn screaming_to_camel_cases() {
        for (input, want) in [
            ("VERY_HIGH", "veryHigh"),
            ("CLIENT_DEVICES", "clientDevices"),
            ("WIFI_STATS", "wifiStats"),
            ("WLAN", "wlan"),
            ("", ""),
        ] {
            assert_eq!(screaming_to_camel(input), want, "{input}");
        }
    }

    #[test]
    fn log_severity_from_controller() {
        assert_eq!(
            LogSeverity::from_controller("VERY_HIGH"),
            LogSeverity::VeryHigh
        );
        assert_eq!(
            LogSeverity::from_controller("WARNING"),
            LogSeverity::Warning
        );
        assert_eq!(
            LogSeverity::from_controller("CRITICAL"),
            LogSeverity::Unknown
        );
        assert_eq!(LogSeverity::VeryHigh.as_str(), "veryHigh");
    }
}
