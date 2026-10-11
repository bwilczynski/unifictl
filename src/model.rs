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
}

/// Fields shared by every `NetworkDeviceDetail` variant.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceBase {
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
    pub base: DeviceBase,
    pub ports: Vec<DevicePort>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GatewayDetail {
    #[serde(flatten)]
    pub base: DeviceBase,
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
    pub base: DeviceBase,
    pub num_clients: i64,
    pub connected_clients: Vec<ApClient>,
}

/// Discriminated on `type` (carried in `DeviceBase`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum NetworkDeviceDetail {
    Switch(SwitchDetail),
    AccessPoint(AccessPointDetail),
    Gateway(GatewayDetail),
    Unknown(DeviceBase),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NetworkDevice {
    pub id: String,
    pub name: String,
    pub mac: String,
    pub ip: String,
    #[serde(rename = "type")]
    pub device_type: DeviceType,
    pub status: DeviceStatus,
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

// ---------- clients ----------

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
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NetworkClientList {
    pub items: Vec<NetworkClient>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WiredClientDetail {
    pub id: String,
    pub name: String,
    pub mac: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip: Option<String>,
    pub connection_type: ConnectionType,
    pub status: ClientStatus,
    pub connected_to: Connection,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uptime: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WirelessConnection {
    pub device: DeviceRef,
    pub ssid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal_strength: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WirelessClientDetail {
    pub id: String,
    pub name: String,
    pub mac: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip: Option<String>,
    pub connection_type: ConnectionType,
    pub status: ClientStatus,
    pub connected_to: WirelessConnection,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uptime: Option<i64>,
}

/// Discriminated on `connectionType`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum NetworkClientDetail {
    Wired(WiredClientDetail),
    Wireless(WirelessClientDetail),
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
#[serde(rename_all = "camelCase")]
pub struct Vlan {
    pub id: String,
    pub name: String,
    pub vlan_id: i64,
    pub subnet: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VlanList {
    pub items: Vec<Vlan>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DhcpRange {
    pub start: String,
    pub end: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VlanDetail {
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
#[serde(rename_all = "camelCase")]
pub struct Wan {
    pub id: String,
    pub name: String,
    pub ip_address: String,
    pub status: WanStatus,
    pub uptime: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WanList {
    pub items: Vec<Wan>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WanDetail {
    pub id: String,
    pub name: String,
    pub ip_address: String,
    pub status: WanStatus,
    pub uptime: i64,
    pub dns_servers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Ssid {
    pub id: String,
    pub name: String,
    pub bands: Vec<WifiBand>,
    pub vlan_id: i64,
    pub num_clients: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SsidList {
    pub items: Vec<Ssid>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SsidDetail {
    pub id: String,
    pub name: String,
    pub bands: Vec<WifiBand>,
    pub vlan_id: i64,
    pub num_clients: i64,
    pub clients: Vec<ClientRef>,
    #[serde(serialize_with = "empty_as_null")]
    pub broadcasting_aps: Vec<DeviceRef>,
    pub security_protocol: SecurityProtocol,
}

// ---------- -o ids ----------

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
    VlanList,
    SsidList,
    WanList
);

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
            base: DeviceBase {
                id: "us-8".into(),
                name: "US 8".into(),
                mac: "aa:bb:cc:dd:01:01".into(),
                ip: "192.168.1.2".into(),
                device_type: DeviceType::Switch,
                status: DeviceStatus::Connected,
                model: "USC8".into(),
                firmware_version: "1.0".into(),
                uptime: 5,
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
        assert!(v.get("uri").is_none());
    }

    #[test]
    fn empty_broadcasting_aps_serialize_as_null() {
        let s = SsidDetail {
            id: "x".into(),
            name: "x".into(),
            bands: vec![WifiBand::Band2g],
            vlan_id: 1,
            num_clients: 0,
            clients: vec![],
            broadcasting_aps: vec![],
            security_protocol: SecurityProtocol::Wpa2,
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
