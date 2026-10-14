//! System log: device/client lookup, filtering, paging, conversion to the
//! output model, and `--follow`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Duration;

use jiff::Timestamp;
use jiff::tz::TimeZone;

use super::Service;
use super::common::{client_ref_v2, device_ref, normalize_mac};
use crate::error::{Error, Result};
use crate::model::{ClientRef, DeviceRef, LogClientRef, LogEntry, LogSeverity, screaming_to_camel};
use crate::unifi::fetch;
use crate::unifi::types::{ClientV2, Device, LogParam, LogQuery, SystemLogEntry};

/// Parameters that name a device. Matched by key because other parameters
/// (`CHANNEL`, …) reuse the device's MAC as their `id`.
pub(crate) const DEVICE_KEYS: [&str; 5] = [
    "DEVICE",
    "DEVICE_FROM",
    "DEVICE_TO",
    "DEVICE_WITH_PORT",
    "OBJECT",
];
/// Parameters that name a client (`WIFI_STATS`, `IP`, … reuse its MAC).
pub(crate) const CLIENT_KEYS: [&str; 3] = ["CLIENT", "SRC_CLIENT", "DST_CLIENT"];

/// Devices and clients by MAC, for `--device`/`--client` and JSON references.
pub struct RefIndex {
    devices: HashMap<String, DeviceRef>,
    clients: HashMap<String, ClientRef>,
    /// When the lists were fetched, epoch milliseconds.
    pub(crate) fetched_at_ms: i64,
}

impl RefIndex {
    pub(crate) fn new(devices: &[Device], clients: &[ClientV2], fetched_at_ms: i64) -> Self {
        Self {
            devices: devices
                .iter()
                .map(|d| (normalize_mac(&d.mac), device_ref(d)))
                .collect(),
            clients: clients
                .iter()
                .map(|c| (normalize_mac(&c.mac), client_ref_v2(c)))
                .collect(),
            fetched_at_ms,
        }
    }

    pub(crate) fn device_mac(&self, id: &str) -> Result<String> {
        self.devices
            .iter()
            .find(|(_, r)| r.id == id)
            .map(|(mac, _)| mac.clone())
            .ok_or_else(|| Error::NotFound(format!("Network device not found: {id}")))
    }

    pub(crate) fn client_mac(&self, id: &str) -> Result<String> {
        self.clients
            .iter()
            .find(|(_, r)| r.id == id)
            .map(|(mac, _)| mac.clone())
            .ok_or_else(|| Error::NotFound(format!("Network client not found: {id}")))
    }

    pub(crate) fn knows(&self, mac: &str) -> bool {
        self.devices.contains_key(mac) || self.clients.contains_key(mac)
    }
}

impl Service {
    /// The device list and the online + recently offline clients, as
    /// `devices list` and `clients list` see them.
    pub(crate) fn ref_index(&self, now_ms: i64) -> Result<RefIndex> {
        fetch!(
            devices = self.api.devices(),
            clients = self.api.all_clients(self.history_days),
        );
        Ok(RefIndex::new(&devices, &clients, now_ms))
    }
}

/// `aa:bb:cc:dd:ee:ff`, any case.
pub(crate) fn is_mac(s: &str) -> bool {
    s.len() == 17
        && s.split(':').count() == 6
        && s.split(':')
            .all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_hexdigit()))
}

/// `template` with each `{UPPER_SNAKE}` placeholder replaced by that
/// parameter's name, or `?` when it has none. Other braces are kept.
pub(crate) fn fill(template: &str, params: &BTreeMap<String, LogParam>) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find(['{', '}']) {
            Some(close) if after.as_bytes()[close] == b'}' && is_key(&after[..close]) => {
                let key = &after[..close];
                let value = params.get(key).and_then(LogParam::name);
                out.push_str(value.as_deref().unwrap_or("?"));
                rest = &after[close + 1..];
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn is_key(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// The output model of `e`. With `refs`, device and client parameters become
/// references; every other named parameter goes to `details`.
pub(crate) fn to_model(e: &SystemLogEntry, refs: Option<&RefIndex>, tz: &TimeZone) -> LogEntry {
    let mut devices: Vec<DeviceRef> = Vec::new();
    let mut clients: Vec<LogClientRef> = Vec::new();
    let mut details = BTreeMap::new();
    let mut referenced = HashSet::new();
    for (key, p) in &e.parameters {
        let mac = p.id().map(|m| normalize_mac(&m));
        if let (Some(refs), Some(mac)) = (refs, &mac) {
            if DEVICE_KEYS.contains(&key.as_str()) {
                if let Some(d) = refs.devices.get(mac) {
                    if referenced.insert(mac.clone()) {
                        devices.push(d.clone());
                    }
                    continue;
                }
            } else if CLIENT_KEYS.contains(&key.as_str()) {
                if referenced.insert(mac.clone()) {
                    clients.push(match refs.clients.get(mac) {
                        Some(c) => LogClientRef {
                            kind: "client",
                            id: Some(c.id.clone()),
                            name: c.name.clone(),
                        },
                        None => LogClientRef {
                            kind: "client",
                            id: None,
                            name: p.name().unwrap_or_else(|| mac.clone()),
                        },
                    });
                }
                continue;
            }
        }
        if key == "CONSOLE_NAME" {
            continue;
        }
        if let Some(name) = p.name() {
            details.insert(screaming_to_camel(key), name);
        }
    }
    let time = Timestamp::from_millisecond(e.timestamp)
        .unwrap_or(Timestamp::UNIX_EPOCH)
        .to_zoned(tz.clone())
        .strftime("%Y-%m-%dT%H:%M:%S%.3f%:z")
        .to_string();
    LogEntry {
        id: e.id.clone(),
        time,
        severity: LogSeverity::from_controller(&e.severity),
        category: screaming_to_camel(&e.category),
        subcategory: screaming_to_camel(&e.subcategory),
        event: screaming_to_camel(&e.event),
        title: fill(&e.title_raw, &e.parameters),
        message: fill(&e.message_raw, &e.parameters),
        devices,
        clients,
        details,
        timestamp_ms: e.timestamp,
    }
}

/// Largest page requested from the controller.
const PAGE_SIZE: usize = 1000;

/// What `unifictl logs` asks for. Times are epoch milliseconds.
#[derive(Debug, Clone, Default)]
pub struct LogFilter {
    pub since_ms: Option<i64>,
    pub until_ms: Option<i64>,
    /// `None` and `Low` both mean every severity.
    pub min_severity: Option<LogSeverity>,
    /// Controller category names (`SECURITY`, …); empty means all.
    pub categories: Vec<&'static str>,
    /// Device IDs; an entry must mention one of them.
    pub devices: Vec<String>,
    /// Client IDs; an entry must mention one of them.
    pub clients: Vec<String>,
    /// Keep only the newest N matching entries.
    pub lines: Option<usize>,
    pub newest_first: bool,
    /// Build device and client references (JSON output).
    pub with_refs: bool,
}

/// `--device`/`--client` resolved to MACs, and the references for JSON.
#[derive(Default)]
pub(crate) struct LogScope {
    device_macs: Vec<String>,
    client_macs: Vec<String>,
    pub(crate) refs: Option<RefIndex>,
}

impl LogScope {
    /// Whether any of the entry's parameter IDs is a wanted device (if any
    /// are wanted) and a wanted client (if any are wanted).
    fn matches(&self, e: &SystemLogEntry) -> bool {
        let macs: Vec<String> = e
            .parameters
            .values()
            .filter_map(LogParam::id)
            .map(|m| normalize_mac(&m))
            .collect();
        let wanted = |want: &[String]| want.is_empty() || want.iter().any(|w| macs.contains(w));
        wanted(&self.device_macs) && wanted(&self.client_macs)
    }

    fn filters(&self) -> bool {
        !self.device_macs.is_empty() || !self.client_macs.is_empty()
    }
}

/// The newest entry fetched so far (matching or not) and every entry ID at
/// that millisecond, so `--follow` neither repeats nor skips entries.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Cursor {
    pub(crate) last_ms: i64,
    pub(crate) ids: HashSet<String>,
}

impl Cursor {
    fn see(&mut self, e: &SystemLogEntry) {
        if e.timestamp > self.last_ms || self.ids.is_empty() {
            self.last_ms = e.timestamp;
            self.ids.clear();
        }
        if e.timestamp == self.last_ms {
            self.ids.insert(e.id.clone());
        }
    }

    /// Whether `e` was already fetched.
    pub(crate) fn has(&self, e: &SystemLogEntry) -> bool {
        e.timestamp < self.last_ms || (e.timestamp == self.last_ms && self.ids.contains(&e.id))
    }
}

/// Entries for one `logs` run, plus what `--follow` needs to continue.
pub struct LogPage {
    pub entries: Vec<LogEntry>,
    pub(crate) scope: LogScope,
    pub(crate) cursor: Cursor,
}

/// Controller severities at or above `min`; empty means no filter.
pub(crate) fn severities(min: Option<LogSeverity>) -> Vec<&'static str> {
    const RANKED: [(&str, u8); 6] = [
        ("INFO", 0),
        ("LOW", 0),
        ("WARNING", 1),
        ("MEDIUM", 1),
        ("HIGH", 2),
        ("VERY_HIGH", 3),
    ];
    let floor = match min {
        None | Some(LogSeverity::Info | LogSeverity::Low | LogSeverity::Unknown) => {
            return Vec::new();
        }
        Some(LogSeverity::Warning | LogSeverity::Medium) => 1,
        Some(LogSeverity::High) => 2,
        Some(LogSeverity::VeryHigh) => 3,
    };
    RANKED
        .iter()
        .filter(|(_, rank)| *rank >= floor)
        .map(|(name, _)| *name)
        .collect()
}

/// Oldest first (ties by ID), or newest first.
pub(crate) fn order(entries: &mut [LogEntry], newest_first: bool) {
    entries.sort_by(|a, b| (a.timestamp_ms, &a.id).cmp(&(b.timestamp_ms, &b.id)));
    if newest_first {
        entries.reverse();
    }
}

impl Service {
    /// The entries `f` selects, newest `f.lines` of them.
    pub fn logs(&self, f: &LogFilter, now_ms: i64, tz: &TimeZone) -> Result<LogPage> {
        let scope = self.log_scope(f, now_ms)?;
        let to = f.until_ms.unwrap_or(now_ms);
        let (raw, mut cursor) = self.collect_log(f, f.since_ms, to, &scope, f.lines)?;
        if cursor.ids.is_empty() {
            cursor.last_ms = now_ms;
        }
        let mut entries: Vec<LogEntry> = raw
            .iter()
            .map(|e| to_model(e, scope.refs.as_ref(), tz))
            .collect();
        order(&mut entries, f.newest_first);
        Ok(LogPage {
            entries,
            scope,
            cursor,
        })
    }

    /// Fetches the device and client lists only when something needs them.
    pub(crate) fn log_scope(&self, f: &LogFilter, now_ms: i64) -> Result<LogScope> {
        if f.devices.is_empty() && f.clients.is_empty() && !f.with_refs {
            return Ok(LogScope::default());
        }
        let refs = self.ref_index(now_ms)?;
        let device_macs = f
            .devices
            .iter()
            .map(|id| refs.device_mac(id))
            .collect::<Result<_>>()?;
        let client_macs = f
            .clients
            .iter()
            .map(|id| refs.client_mac(id))
            .collect::<Result<_>>()?;
        Ok(LogScope {
            device_macs,
            client_macs,
            refs: f.with_refs.then_some(refs),
        })
    }

    /// Matching entries in `[from, to]`, newest first, at most `limit`, and
    /// a cursor at the newest entry fetched. Pages until `limit` is reached
    /// or the log runs out; entries repeated across pages are dropped.
    pub(crate) fn collect_log(
        &self,
        f: &LogFilter,
        from: Option<i64>,
        to: i64,
        scope: &LogScope,
        limit: Option<usize>,
    ) -> Result<(Vec<SystemLogEntry>, Cursor)> {
        let mut out = Vec::new();
        let mut cursor = Cursor::default();
        if limit == Some(0) {
            return Ok((out, cursor));
        }
        let page_size = match limit {
            Some(n) if !scope.filters() => n.min(PAGE_SIZE),
            _ => PAGE_SIZE,
        };
        let mut seen = HashSet::new();
        for page_number in 0_i64.. {
            let page = self.api.system_log(&LogQuery {
                timestamp_from: from,
                timestamp_to: Some(to),
                severities: severities(f.min_severity),
                categories: f.categories.clone(),
                page_size: page_size as i64,
                page_number,
            })?;
            let last = page.data.is_empty() || page_number + 1 >= page.total_page_count;
            for e in page.data {
                if !seen.insert(e.id.clone()) {
                    continue;
                }
                cursor.see(&e);
                if scope.matches(&e) {
                    out.push(e);
                    if limit.is_some_and(|n| out.len() >= n) {
                        return Ok((out, cursor));
                    }
                }
            }
            if last {
                break;
            }
        }
        Ok((out, cursor))
    }
}

/// Pause between polls while following.
pub const FOLLOW_INTERVAL: Duration = Duration::from_secs(5);
/// Longest pause between retries after failures.
const MAX_BACKOFF: Duration = Duration::from_secs(60);
/// How stale the device and client lists may get before an unknown MAC
/// triggers a refetch.
const REFS_REFRESH_MS: i64 = 60_000;

/// Continues a `logs` run: each poll returns the matching entries that
/// arrived since the previous one.
pub struct Follower {
    filter: LogFilter,
    scope: LogScope,
    cursor: Cursor,
}

impl LogPage {
    /// The entries to print first, and a follower that continues after them.
    pub fn follow(self, filter: LogFilter) -> (Vec<LogEntry>, Follower) {
        let follower = Follower {
            filter,
            scope: self.scope,
            cursor: self.cursor,
        };
        (self.entries, follower)
    }
}

impl Follower {
    /// New matching entries up to `now_ms`, oldest first.
    pub fn poll(&mut self, svc: &Service, now_ms: i64, tz: &TimeZone) -> Result<Vec<LogEntry>> {
        // Fetch every entry in the window so the cursor moves past
        // non-matching ones too; filter by `self.scope` afterwards.
        let (raw, _) = svc.collect_log(
            &self.filter,
            Some(self.cursor.last_ms),
            now_ms,
            &LogScope::default(),
            None,
        )?;
        let fresh: Vec<SystemLogEntry> = raw.into_iter().filter(|e| !self.cursor.has(e)).collect();
        for e in &fresh {
            self.cursor.see(e);
        }
        self.refresh_refs(svc, &fresh, now_ms);
        let mut entries: Vec<LogEntry> = fresh
            .iter()
            .filter(|e| self.scope.matches(e))
            .map(|e| to_model(e, self.scope.refs.as_ref(), tz))
            .collect();
        order(&mut entries, false);
        Ok(entries)
    }

    /// Refetches the lists when a device or client key holds a MAC they
    /// lack, at most once per `REFS_REFRESH_MS`. A failed refetch keeps the
    /// old lists.
    fn refresh_refs(&mut self, svc: &Service, fresh: &[SystemLogEntry], now_ms: i64) {
        let Some(refs) = &mut self.scope.refs else {
            return;
        };
        if now_ms - refs.fetched_at_ms < REFS_REFRESH_MS {
            return;
        }
        let unknown = fresh.iter().flat_map(|e| &e.parameters).any(|(key, p)| {
            (DEVICE_KEYS.contains(&key.as_str()) || CLIENT_KEYS.contains(&key.as_str()))
                && p.id()
                    .is_some_and(|m| is_mac(&m) && !refs.knows(&normalize_mac(&m)))
        });
        if !unknown {
            return;
        }
        match svc.ref_index(now_ms) {
            Ok(new) => *refs = new,
            Err(_) => refs.fetched_at_ms = now_ms,
        }
    }
}

/// Delay before the next poll after `failures` consecutive failures.
pub fn backoff(failures: u32) -> Duration {
    if failures == 0 {
        return FOLLOW_INTERVAL;
    }
    (FOLLOW_INTERVAL * 2u32.pow(failures.min(5) - 1)).min(MAX_BACKOFF)
}

/// Polls forever: sleeps, polls, hands each non-empty batch to `emit`.
/// Returns `Ok` when `emit` says the reader has gone, and the error on an
/// authentication failure. Other failures are retried with [`backoff`],
/// warning once per streak.
pub fn follow_loop(
    mut poll: impl FnMut() -> Result<Vec<LogEntry>>,
    mut emit: impl FnMut(&[LogEntry]) -> Result<bool>,
    mut sleep: impl FnMut(Duration),
    mut warn: impl FnMut(&str),
) -> Result<()> {
    let mut failures = 0u32;
    loop {
        sleep(backoff(failures));
        match poll() {
            Ok(entries) => {
                failures = 0;
                if !entries.is_empty() && !emit(&entries)? {
                    return Ok(());
                }
            }
            Err(e @ Error::Auth(_)) => return Err(e),
            Err(e) => {
                failures = failures.saturating_add(1);
                if failures == 1 {
                    warn(&format!("controller unreachable, retrying: {e}"));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use jiff::tz::TimeZone;
    use serde_json::json;

    use super::*;
    use crate::service::testutil::{fixtures, svc};
    use crate::unifi::fixtures::FixtureApi;
    use crate::unifi::types::LogParam;

    pub(super) const NOW_MS: i64 = 1791720000000;

    pub(super) fn entry(api: &FixtureApi, suffix: &str) -> SystemLogEntry {
        api.log_entries
            .iter()
            .find(|e| e.id.ends_with(suffix))
            .cloned()
            .unwrap()
    }

    fn params(v: serde_json::Value) -> BTreeMap<String, LogParam> {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn fill_replaces_placeholders_and_marks_missing() {
        let p = params(json!({
            "A": {"id": "1", "name": "alpha"},
            "N": {"id": "2", "name": 7},
            "NULL": {"id": "3", "name": null},
            "GONE": null
        }));
        assert_eq!(fill("{A} and {N}", &p), "alpha and 7");
        assert_eq!(fill("{NULL}/{GONE}/{MISSING}", &p), "?/?/?");
        assert_eq!(fill("no placeholders", &p), "no placeholders");
    }

    #[test]
    fn fill_keeps_non_placeholder_braces() {
        let p = params(json!({"A": {"name": "alpha"}}));
        assert_eq!(fill("{} {lower} {A} {X", &p), "{} {lower} alpha {X");
        assert_eq!(fill("{{A}}", &p), "{alpha}");
        assert_eq!(fill("{", &p), "{");
    }

    #[test]
    fn roam_entry_has_refs_and_details() {
        let api = fixtures();
        let e = entry(&api, "07");
        let s = svc(api);
        let refs = s.ref_index(NOW_MS).unwrap();
        let m = to_model(&e, Some(&refs), &TimeZone::UTC);
        assert_eq!(
            serde_json::to_string(&m).unwrap(),
            r#"{"id":"65f000000000000000000007","time":"2026-10-11T11:00:00.000+00:00","severity":"low","category":"clientDevices","subcategory":"monitoringWifi","event":"clientRoamed","title":"WiFi Client Roamed","message":"MacBook Pro roamed from UAP-02 to UAP-01. Connection Info: Ch. 36 (5 GHz, 80 MHz), -60 dBm.","devices":[{"kind":"device","id":"uap-02","name":"UAP-02"},{"kind":"device","id":"uap-01","name":"UAP-01"}],"clients":[{"kind":"client","id":"macbook-pro-0001","name":"MacBook Pro"}],"details":{"wifiStats":"Ch. 36 (5 GHz, 80 MHz), -60 dBm","wlan":"home-wifi"}}"#
        );
        assert_eq!(m.timestamp_ms, 1791716400000);
    }

    #[test]
    fn config_entry_object_is_a_device_and_missing_values_are_marked() {
        let api = fixtures();
        let e = entry(&api, "05");
        let s = svc(api);
        let m = to_model(&e, Some(&s.ref_index(NOW_MS).unwrap()), &TimeZone::UTC);
        assert_eq!(
            m.message,
            "Network Application made a change to ? in UAP-02 ? settings. Source IP: ?"
        );
        assert_eq!(m.devices.len(), 1);
        assert_eq!(m.devices[0].id, "uap-02");
        assert_eq!(
            m.details,
            BTreeMap::from([("admin".to_string(), "Network Application".to_string())])
        );
    }

    #[test]
    fn unknown_client_keeps_log_name_with_null_id() {
        let api = fixtures();
        let e = entry(&api, "09");
        let s = svc(api);
        let m = to_model(&e, Some(&s.ref_index(NOW_MS).unwrap()), &TimeZone::UTC);
        assert_eq!(
            serde_json::to_value(&m.clients).unwrap(),
            json!([{"kind": "client", "id": null, "name": "Guest Phone"}])
        );
        assert_eq!(m.devices[0].id, "uap-01");
    }

    #[test]
    fn title_placeholders_are_filled_and_numbers_kept_as_strings() {
        let api = fixtures();
        let e = entry(&api, "02");
        let m = to_model(&e, None, &TimeZone::UTC);
        assert_eq!(m.title, "WAN1 Internet Restored");
        assert_eq!(
            m.message,
            "Internet connection WAN1 ExampleISP on port 5 is restored."
        );
        assert_eq!(m.severity, LogSeverity::Medium);
    }

    #[test]
    fn without_refs_no_references_are_built() {
        let api = fixtures();
        let m = to_model(&entry(&api, "07"), None, &TimeZone::UTC);
        assert!(m.devices.is_empty() && m.clients.is_empty());
    }

    #[test]
    fn unknown_severity_and_empty_category() {
        let e = SystemLogEntry {
            id: "x".into(),
            timestamp: NOW_MS,
            severity: "CRITICAL".into(),
            message_raw: "plain".into(),
            ..Default::default()
        };
        let m = to_model(&e, None, &TimeZone::UTC);
        assert_eq!(m.severity, LogSeverity::Unknown);
        assert_eq!(m.category, "");
        assert_eq!(m.time, "2026-10-11T12:00:00.000+00:00");
    }

    #[test]
    fn ref_index_resolves_ids_to_macs() {
        let s = svc(fixtures());
        let refs = s.ref_index(NOW_MS).unwrap();
        assert_eq!(refs.device_mac("uap-02").unwrap(), "aa:bb:cc:dd:02:01");
        assert_eq!(
            refs.client_mac("kindle-paperwhite-bb04").unwrap(),
            "e0:f7:28:aa:bb:04"
        );
        assert_eq!(
            refs.device_mac("nope").unwrap_err().to_string(),
            "Network device not found: nope"
        );
        assert_eq!(
            refs.client_mac("nope").unwrap_err().to_string(),
            "Network client not found: nope"
        );
        assert!(refs.knows("3c:ee:ee:00:00:01") && !refs.knows("a0:ee:ee:00:00:99"));
    }

    #[test]
    fn is_mac_shapes() {
        assert!(is_mac("aa:bb:cc:dd:02:01"));
        assert!(!is_mac("admin-1") && !is_mac("aa:bb:cc:dd:02") && !is_mac("zz:bb:cc:dd:02:01"));
    }

    use std::sync::{Arc, Mutex};

    use crate::unifi::UnifiApi;
    use crate::unifi::types::{
        ClientV2, Device, DpiEntry, LogQuery, NetworkConf, Sta, SystemLogPage, TrafficReport,
        WanHistory, WlanConf,
    };

    fn ids(page: &LogPage) -> Vec<&str> {
        page.entries.iter().map(|e| &e.id[22..]).collect()
    }

    fn query(f: LogFilter) -> Vec<String> {
        let page = svc(fixtures()).logs(&f, NOW_MS, &TimeZone::UTC).unwrap();
        ids(&page).into_iter().map(String::from).collect()
    }

    #[test]
    fn default_is_oldest_first() {
        let got = query(LogFilter {
            lines: Some(50),
            ..Default::default()
        });
        assert_eq!(
            got,
            ["01", "02", "03", "04", "05", "06", "07", "08", "09", "0a"]
        );
    }

    #[test]
    fn lines_keep_the_newest_and_reverse_flips() {
        assert_eq!(
            query(LogFilter {
                lines: Some(2),
                ..Default::default()
            }),
            ["09", "0a"]
        );
        assert_eq!(
            query(LogFilter {
                lines: Some(3),
                newest_first: true,
                ..Default::default()
            }),
            ["0a", "09", "08"]
        );
        assert!(
            query(LogFilter {
                lines: Some(0),
                ..Default::default()
            })
            .is_empty()
        );
    }

    #[test]
    fn severity_and_category_filters() {
        assert_eq!(
            query(LogFilter {
                min_severity: Some(LogSeverity::High),
                ..Default::default()
            }),
            ["01", "03"]
        );
        assert_eq!(
            query(LogFilter {
                categories: vec!["SECURITY"],
                ..Default::default()
            }),
            ["03", "04"]
        );
    }

    #[test]
    fn severities_sent_to_the_controller() {
        assert!(severities(None).is_empty());
        assert!(severities(Some(LogSeverity::Low)).is_empty());
        assert_eq!(
            severities(Some(LogSeverity::Medium)),
            ["WARNING", "MEDIUM", "HIGH", "VERY_HIGH"]
        );
        assert_eq!(severities(Some(LogSeverity::VeryHigh)), ["VERY_HIGH"]);
    }

    #[test]
    fn device_and_client_filters_and_or() {
        let f = |devices: &[&str], clients: &[&str]| LogFilter {
            devices: devices.iter().map(|s| s.to_string()).collect(),
            clients: clients.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        };
        assert_eq!(query(f(&["uap-02"], &[])), ["05", "06", "07"]);
        assert_eq!(query(f(&[], &["macbook-pro-0001"])), ["04", "07", "0a"]);
        assert_eq!(query(f(&["uap-01"], &["macbook-pro-0001"])), ["07", "0a"]);
        assert_eq!(
            query(f(&["uap-01", "uap-02"], &[])),
            ["05", "06", "07", "09", "0a"]
        );
    }

    #[test]
    fn unknown_ids_are_not_found() {
        let f = LogFilter {
            devices: vec!["nope".into()],
            ..Default::default()
        };
        let err = svc(fixtures())
            .logs(&f, NOW_MS, &TimeZone::UTC)
            .err()
            .expect("unknown device");
        assert_eq!(err.to_string(), "Network device not found: nope");
    }

    #[test]
    fn time_window() {
        assert_eq!(
            query(LogFilter {
                since_ms: Some(1791676800000),
                ..Default::default()
            }),
            ["05", "06", "07", "08", "09", "0a"]
        );
        assert_eq!(
            query(LogFilter {
                until_ms: Some(1791504000000),
                ..Default::default()
            }),
            ["01", "02"]
        );
    }

    #[test]
    fn text_output_skips_the_lists_but_json_resolves_refs() {
        let page = svc(fixtures())
            .logs(&LogFilter::default(), NOW_MS, &TimeZone::UTC)
            .unwrap();
        assert!(page.scope.refs.is_none());
        assert!(page.entries.iter().all(|e| e.devices.is_empty()));

        let page = svc(fixtures())
            .logs(
                &LogFilter {
                    with_refs: true,
                    ..Default::default()
                },
                NOW_MS,
                &TimeZone::UTC,
            )
            .unwrap();
        assert!(page.scope.refs.is_some());
        assert_eq!(page.entries[6].devices[0].id, "uap-02");
    }

    /// Serves the fixture log in pages of 2; every page after the first
    /// starts one entry early, as if a new entry had arrived (so there is one
    /// page more than 10 entries need). Records every query.
    struct Overlapping {
        inner: FixtureApi,
        queries: Mutex<Vec<LogQuery>>,
    }

    impl UnifiApi for Arc<Overlapping> {
        fn devices(&self) -> Result<Vec<Device>> {
            self.inner.devices()
        }
        fn clients(&self) -> Result<Vec<Sta>> {
            self.inner.clients()
        }
        fn active_clients(&self) -> Result<Vec<ClientV2>> {
            self.inner.active_clients()
        }
        fn offline_clients(&self, days: i64) -> Result<Vec<ClientV2>> {
            self.inner.offline_clients(days)
        }
        fn wlan_conf(&self) -> Result<Vec<WlanConf>> {
            self.inner.wlan_conf()
        }
        fn network_conf(&self) -> Result<Vec<NetworkConf>> {
            self.inner.network_conf()
        }
        fn system_log(&self, q: &LogQuery) -> Result<SystemLogPage> {
            self.queries.lock().unwrap().push(q.clone());
            let all = self
                .inner
                .system_log(&LogQuery {
                    page_size: 1000,
                    page_number: 0,
                    ..q.clone()
                })?
                .data;
            let n = q.page_number as usize;
            let start = (n * 2).saturating_sub(n.min(1));
            Ok(SystemLogPage {
                data: all.iter().skip(start).take(2).cloned().collect(),
                total_page_count: (all.len() + 1).div_ceil(2) as i64,
            })
        }
        fn traffic(&self, start_ms: i64, end_ms: i64) -> Result<TrafficReport> {
            self.inner.traffic(start_ms, end_ms)
        }
        fn wan_history(&self, history_seconds: i64) -> Result<WanHistory> {
            self.inner.wan_history(history_seconds)
        }
        fn dpi_categories(&self) -> Result<Vec<DpiEntry>> {
            self.inner.dpi_categories()
        }
        fn dpi_applications(&self) -> Result<Vec<DpiEntry>> {
            self.inner.dpi_applications()
        }
    }

    fn overlapping() -> (Service, Arc<Overlapping>) {
        let api = Arc::new(Overlapping {
            inner: fixtures(),
            queries: Mutex::new(Vec::new()),
        });
        (Service::new(Box::new(api.clone()), 30), api)
    }

    #[test]
    fn pages_until_done_without_duplicates_with_a_pinned_window() {
        let (s, api) = overlapping();
        let page = s
            .logs(&LogFilter::default(), NOW_MS, &TimeZone::UTC)
            .unwrap();
        assert_eq!(page.entries.len(), 10);
        let queries = api.queries.lock().unwrap();
        assert_eq!(queries.len(), 6);
        assert!(
            queries
                .iter()
                .all(|q| q.timestamp_to == Some(NOW_MS) && q.page_size == 1000)
        );
    }

    #[test]
    fn a_limit_without_mac_filters_sizes_the_page_and_stops_early() {
        let (s, api) = overlapping();
        let f = LogFilter {
            lines: Some(1),
            ..Default::default()
        };
        let page = s.logs(&f, NOW_MS, &TimeZone::UTC).unwrap();
        assert_eq!(ids(&page), ["0a"]);
        let queries = api.queries.lock().unwrap();
        assert_eq!(queries.len(), 1);
        assert_eq!(queries[0].page_size, 1);
    }

    #[test]
    fn mac_filters_read_further_back() {
        let (s, api) = overlapping();
        let f = LogFilter {
            devices: vec!["uap-02".into()],
            lines: Some(3),
            ..Default::default()
        };
        assert_eq!(
            ids(&s.logs(&f, NOW_MS, &TimeZone::UTC).unwrap()),
            ["05", "06", "07"]
        );
        assert_eq!(api.queries.lock().unwrap()[0].page_size, 1000);
    }

    #[test]
    fn cursor_tracks_the_newest_fetched_entry() {
        let page = svc(fixtures())
            .logs(
                &LogFilter {
                    devices: vec!["uap-02".into()],
                    ..Default::default()
                },
                NOW_MS,
                &TimeZone::UTC,
            )
            .unwrap();
        // The newest *matching* entry is 07, but 08–0a were fetched too.
        assert_eq!(page.cursor.last_ms, 1791719400000);
        assert_eq!(
            page.cursor.ids,
            HashSet::from([
                "65f000000000000000000009".to_string(),
                "65f00000000000000000000a".to_string()
            ])
        );
    }

    #[test]
    fn empty_log_cursor_starts_at_now() {
        let mut api = fixtures();
        api.log_entries.clear();
        let page = svc(api)
            .logs(&LogFilter::default(), NOW_MS, &TimeZone::UTC)
            .unwrap();
        assert!(page.entries.is_empty());
        assert_eq!(page.cursor.last_ms, NOW_MS);
    }

    fn without(mut api: FixtureApi, suffixes: &[&str]) -> FixtureApi {
        api.log_entries
            .retain(|e| !suffixes.iter().any(|s| e.id.ends_with(s)));
        api
    }

    #[test]
    fn follow_returns_only_new_entries_once() {
        let first = svc(without(fixtures(), &["09", "0a"]));
        let page = first
            .logs(
                &LogFilter {
                    lines: Some(10),
                    ..Default::default()
                },
                NOW_MS,
                &TimeZone::UTC,
            )
            .unwrap();
        let (shown, mut follower) = page.follow(LogFilter::default());
        assert_eq!(shown.len(), 8);

        let full = svc(fixtures());
        let got: Vec<String> = follower
            .poll(&full, NOW_MS + 5000, &TimeZone::UTC)
            .unwrap()
            .iter()
            .map(|e| e.id[22..].to_string())
            .collect();
        assert_eq!(got, ["09", "0a"]);
        assert!(
            follower
                .poll(&full, NOW_MS + 10000, &TimeZone::UTC)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn follow_does_not_repeat_entries_sharing_a_millisecond() {
        let first = svc(without(fixtures(), &["0a"]));
        let page = first
            .logs(&LogFilter::default(), NOW_MS, &TimeZone::UTC)
            .unwrap();
        let (_, mut follower) = page.follow(LogFilter::default());
        let got = follower
            .poll(&svc(fixtures()), NOW_MS + 5000, &TimeZone::UTC)
            .unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].id, "65f00000000000000000000a");
    }

    #[test]
    fn follow_applies_the_device_filter() {
        let f = LogFilter {
            devices: vec!["uap-02".into()],
            ..Default::default()
        };
        let page = svc(without(fixtures(), &["09", "0a"]))
            .logs(&f, NOW_MS, &TimeZone::UTC)
            .unwrap();
        let (_, mut follower) = page.follow(f);
        assert!(
            follower
                .poll(&svc(fixtures()), NOW_MS + 5000, &TimeZone::UTC)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn follow_refetches_refs_for_unknown_macs_at_most_once_a_minute() {
        let f = LogFilter {
            with_refs: true,
            ..Default::default()
        };
        let mut no_macbook = without(fixtures(), &["09", "0a"]);
        no_macbook.active.retain(|c| c.mac != "3c:ee:ee:00:00:01");
        let page = svc(no_macbook).logs(&f, NOW_MS, &TimeZone::UTC).unwrap();
        let (_, mut follower) = page.follow(f);

        // 30 s later: MacBook is unknown, but the lists are too fresh to refetch.
        let got = follower
            .poll(&svc(fixtures()), NOW_MS + 30_000, &TimeZone::UTC)
            .unwrap();
        assert_eq!(got[1].clients[0].id, None);

        // 61 s after the first fetch, a new entry with the unknown MAC refetches.
        let mut later = fixtures();
        let mut e = entry(&later, "0a");
        e.id = "65f00000000000000000000b".into();
        e.timestamp += 300_000;
        later.log_entries.push(e);
        let got = follower
            .poll(&svc(later), NOW_MS + 61_000, &TimeZone::UTC)
            .unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].clients[0].id.as_deref(), Some("macbook-pro-0001"));
    }

    #[test]
    fn backoff_doubles_to_a_minute() {
        let secs: Vec<u64> = (0..8).map(|n| backoff(n).as_secs()).collect();
        assert_eq!(secs, [5, 5, 10, 20, 40, 60, 60, 60]);
        assert_eq!(backoff(u32::MAX).as_secs(), 60);
    }

    fn fake_entry() -> LogEntry {
        to_model(
            &SystemLogEntry {
                id: "x".into(),
                ..Default::default()
            },
            None,
            &TimeZone::UTC,
        )
    }

    #[test]
    fn follow_loop_backs_off_warns_once_per_streak_and_stops_when_reader_leaves() {
        let mut script: Vec<Result<Vec<LogEntry>>> = vec![
            Err(Error::Unifi("down".into())),
            Err(Error::Unifi("down".into())),
            Err(Error::Unifi("down".into())),
            Ok(vec![fake_entry()]),
            Ok(vec![]),
            Err(Error::Unifi("down again".into())),
            Ok(vec![fake_entry()]),
        ];
        script.reverse();
        let mut sleeps = Vec::new();
        let mut warnings = Vec::new();
        let mut emitted = 0;
        let result = follow_loop(
            || script.pop().expect("polled past the script"),
            |batch| {
                emitted += batch.len();
                Ok(emitted < 2)
            },
            |d| sleeps.push(d.as_secs()),
            |w| warnings.push(w.to_string()),
        );
        assert!(result.is_ok());
        assert_eq!(emitted, 2);
        assert_eq!(sleeps, [5, 5, 10, 20, 5, 5, 5]);
        assert_eq!(
            warnings,
            [
                "controller unreachable, retrying: down",
                "controller unreachable, retrying: down again"
            ]
        );
    }

    #[test]
    fn follow_loop_stops_on_auth_errors() {
        let mut warned = false;
        let err = follow_loop(
            || Err(Error::Auth("unifi request: invalid API key (401)".into())),
            |_| Ok(true),
            |_| {},
            |_| warned = true,
        )
        .unwrap_err();
        assert!(matches!(err, Error::Auth(_)));
        assert!(!warned);
    }
}
