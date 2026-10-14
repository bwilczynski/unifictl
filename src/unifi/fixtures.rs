//! Fixture-backed `UnifiApi` for unit tests and the hidden `--fixtures` flag.

use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;

use super::UnifiApi;
use super::types::{
    ClientV2, Device, DpiEntry, Envelope, HealthSubsystem, LogQuery, NetworkConf, Sta,
    SystemLogEntry, SystemLogPage, TrafficReport, WanHistory, WlanConf,
};
use crate::error::{Error, Result};

#[derive(Debug, Clone, Default)]
pub struct FixtureApi {
    pub devices: Vec<Device>,
    pub clients: Vec<Sta>,
    pub active: Vec<ClientV2>,
    pub offline: Vec<ClientV2>,
    pub wlans: Vec<WlanConf>,
    pub networks: Vec<NetworkConf>,
    /// `unifi-system-log.json`; empty when that file is absent.
    pub log_entries: Vec<SystemLogEntry>,
    /// `unifi-v2-traffic.json`; empty when absent.
    pub traffic: TrafficReport,
    /// `unifi-v2-aggregated-dashboard.json`; `None` when absent, like a
    /// controller without the endpoint.
    pub wan_history: Option<WanHistory>,
    /// `unifi-dpi-*.json`; `None` when absent, like a controller without an
    /// API key.
    pub dpi_categories: Option<Vec<DpiEntry>>,
    pub dpi_apps: Option<Vec<DpiEntry>>,
    /// `unifi-health.json`; `None` when absent, like a controller without
    /// the endpoint.
    pub health: Option<Vec<HealthSubsystem>>,
    /// Reads that fail, by method name (`"active_clients"`, …), simulating
    /// one endpoint being down.
    pub fail_only: Vec<&'static str>,
    /// When true every call fails, simulating an unreachable controller.
    pub fail: bool,
}

impl FixtureApi {
    /// `tests/fixtures` inside this crate.
    pub fn default_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    /// Loads the `unifi-*.json` files (each `{"data": [...]}`, the traffic
    /// report a bare object) from `dir`; the system log, traffic, WAN history,
    /// DPI and health files are optional.
    pub fn load(dir: &Path) -> Result<Self> {
        Ok(Self {
            devices: read(dir, "unifi-devices.json")?,
            clients: read(dir, "unifi-clients.json")?,
            active: read(dir, "unifi-v2-active.json")?,
            offline: read(dir, "unifi-v2-history.json")?,
            wlans: read(dir, "unifi-wlanconf.json")?,
            networks: read(dir, "unifi-networkconf.json")?,
            log_entries: read_optional(dir, "unifi-system-log.json")?,
            traffic: read_object_optional(dir, "unifi-v2-traffic.json")?,
            wan_history: read_object_optional(dir, "unifi-v2-aggregated-dashboard.json")?,
            dpi_categories: read_if_present(dir, "unifi-dpi-categories.json")?,
            dpi_apps: read_if_present(dir, "unifi-dpi-applications.json")?,
            health: read_if_present(dir, "unifi-health.json")?,
            fail_only: Vec::new(),
            fail: false,
        })
    }

    /// Fails when every read fails (`fail`) or this one does (`fail_only`).
    fn check(&self, read: &'static str) -> Result<()> {
        if self.fail || self.fail_only.contains(&read) {
            return Err(Error::Unifi("unifi request: fixture failure".into()));
        }
        Ok(())
    }
}

fn read<T: DeserializeOwned>(dir: &Path, name: &str) -> Result<Vec<T>> {
    let path = dir.join(name);
    let text = std::fs::read_to_string(&path)
        .map_err(|e| Error::Other(format!("fixture {}: {e}", path.display())))?;
    let env: Envelope<Vec<T>> = serde_json::from_str(&text)
        .map_err(|e| Error::Other(format!("fixture {}: {e}", path.display())))?;
    Ok(env.data)
}

fn read_optional<T: DeserializeOwned>(dir: &Path, name: &str) -> Result<Vec<T>> {
    if dir.join(name).exists() {
        read(dir, name)
    } else {
        Ok(Vec::new())
    }
}

fn read_if_present<T: DeserializeOwned>(dir: &Path, name: &str) -> Result<Option<Vec<T>>> {
    dir.join(name).exists().then(|| read(dir, name)).transpose()
}

fn read_object_optional<T: DeserializeOwned + Default>(dir: &Path, name: &str) -> Result<T> {
    let path = dir.join(name);
    if !path.exists() {
        return Ok(T::default());
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|e| Error::Other(format!("fixture {}: {e}", path.display())))?;
    serde_json::from_str(&text)
        .map_err(|e| Error::Other(format!("fixture {}: {e}", path.display())))
}

fn no_catalogue() -> Error {
    Error::Unifi("needs an API key".into())
}

impl UnifiApi for FixtureApi {
    fn devices(&self) -> Result<Vec<Device>> {
        self.check("devices")?;
        Ok(self.devices.clone())
    }
    fn clients(&self) -> Result<Vec<Sta>> {
        self.check("clients")?;
        Ok(self.clients.clone())
    }
    fn active_clients(&self) -> Result<Vec<ClientV2>> {
        self.check("active_clients")?;
        Ok(self.active.clone())
    }
    fn offline_clients(&self, _history_days: i64) -> Result<Vec<ClientV2>> {
        self.check("offline_clients")?;
        Ok(self.offline.clone())
    }
    fn wlan_conf(&self) -> Result<Vec<WlanConf>> {
        self.check("wlan_conf")?;
        Ok(self.wlans.clone())
    }
    fn network_conf(&self) -> Result<Vec<NetworkConf>> {
        self.check("network_conf")?;
        Ok(self.networks.clone())
    }

    /// Applies the window, severities and categories like the controller,
    /// newest first, then pages.
    fn system_log(&self, q: &LogQuery) -> Result<SystemLogPage> {
        self.check("system_log")?;
        let mut hits: Vec<SystemLogEntry> = self
            .log_entries
            .iter()
            .filter(|e| {
                q.timestamp_from.is_none_or(|from| e.timestamp >= from)
                    && q.timestamp_to.is_none_or(|to| e.timestamp <= to)
                    && (q.severities.is_empty() || q.severities.contains(&e.severity.as_str()))
                    && (q.categories.is_empty() || q.categories.contains(&e.category.as_str()))
            })
            .cloned()
            .collect();
        hits.sort_by(|a, b| (b.timestamp, &b.id).cmp(&(a.timestamp, &a.id)));
        let size = q.page_size.max(1) as usize;
        let total_page_count = hits.len().div_ceil(size) as i64;
        let data = hits
            .into_iter()
            .skip(q.page_number.max(0) as usize * size)
            .take(size)
            .collect();
        Ok(SystemLogPage {
            data,
            total_page_count,
        })
    }

    /// The whole report, whatever the window.
    fn traffic(&self, _start_ms: i64, _end_ms: i64) -> Result<TrafficReport> {
        self.check("traffic")?;
        Ok(self.traffic.clone())
    }
    /// The whole history, whatever the window.
    fn wan_history(&self, _history_seconds: i64) -> Result<WanHistory> {
        self.check("wan_history")?;
        self.wan_history
            .clone()
            .ok_or_else(|| Error::Unifi("unifi request: status 404".into()))
    }
    fn dpi_categories(&self) -> Result<Vec<DpiEntry>> {
        self.check("dpi_categories")?;
        self.dpi_categories.clone().ok_or_else(no_catalogue)
    }
    fn dpi_applications(&self) -> Result<Vec<DpiEntry>> {
        self.check("dpi_applications")?;
        self.dpi_apps.clone().ok_or_else(no_catalogue)
    }
    fn health(&self) -> Result<Vec<HealthSubsystem>> {
        self.check("health")?;
        self.health
            .clone()
            .ok_or_else(|| Error::Unifi("unifi request: status 404".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::unifi::types::{LogQuery, SystemLogPage};

    #[test]
    fn loads_all_fixture_files() {
        let api = FixtureApi::load(&FixtureApi::default_dir()).unwrap();
        assert_eq!(api.devices.len(), 7);
        assert_eq!(api.clients.len(), 5);
        assert_eq!(api.active.len(), 3);
        assert_eq!(api.offline.len(), 2);
        assert_eq!(api.wlans.len(), 2);
        assert_eq!(api.networks.len(), 6);
        assert_eq!(api.devices[6].name, "CGF-01");
        assert_eq!(api.devices[6].kind, "udm");
        assert_eq!(api.log_entries.len(), 10);
        assert_eq!(api.traffic.client_usage_by_app.len(), 3);
        assert_eq!(api.traffic.total_usage_by_app.len(), 6);
        let nas = &api.traffic.client_usage_by_app[1];
        assert_eq!(nas.client.name.as_deref(), Some(""));
        assert_eq!(nas.client.hostname.as_deref(), Some("nas-1"));
        assert!(nas.client.is_wired);
        assert_eq!(nas.usage_by_app[0].total_bytes, 3745513472);
        assert_eq!(nas.usage_by_app[0].activity_seconds, 7200);
        assert_eq!(api.traffic.total_usage_by_app[0].client_count, 1);
        assert_eq!(api.dpi_categories().unwrap().len(), 4);
        assert_eq!(api.dpi_applications().unwrap()[1].id, 262145);
        let health = api.health.as_ref().unwrap();
        assert_eq!(health.len(), 5);
        assert!(
            health
                .iter()
                .all(|s| s.status == "ok" || s.subsystem == "vpn")
        );
        assert!(api.devices[1].upgradable);
        assert_eq!(api.devices[1].upgrade_to_firmware, "6.8.3.15600");
        assert!(!api.devices[0].upgradable);
    }

    #[test]
    fn fail_only_errors_the_named_reads() {
        let api = FixtureApi {
            fail_only: vec!["active_clients", "health"],
            ..FixtureApi::load(&FixtureApi::default_dir()).unwrap()
        };
        assert!(api.active_clients().is_err());
        assert!(api.health().is_err());
        assert!(api.devices().is_ok());
        assert!(api.offline_clients(30).is_ok());
    }

    #[test]
    fn missing_health_file_is_like_a_controller_without_it() {
        assert!(FixtureApi::default().health().is_err());
    }

    #[test]
    fn fail_flag_errors_every_call() {
        let api = FixtureApi {
            fail: true,
            ..Default::default()
        };
        assert!(api.devices().is_err());
        assert!(api.all_clients(30).is_err());
    }

    fn ids(page: &SystemLogPage) -> Vec<&str> {
        page.data.iter().map(|e| &e.id[22..]).collect()
    }

    #[test]
    fn system_log_filters_sorts_newest_first_and_pages() {
        let api = FixtureApi::load(&FixtureApi::default_dir()).unwrap();
        let all = LogQuery {
            page_size: 1000,
            ..Default::default()
        };
        let page = api.system_log(&all).unwrap();
        assert_eq!(
            ids(&page),
            ["0a", "09", "08", "07", "06", "05", "04", "03", "02", "01"]
        );
        assert_eq!(page.total_page_count, 1);

        let q = LogQuery {
            timestamp_from: Some(1791547200000),
            timestamp_to: Some(1791698400000),
            ..all.clone()
        };
        assert_eq!(ids(&api.system_log(&q).unwrap()), ["05", "04", "03"]);

        let q = LogQuery {
            severities: vec!["HIGH", "VERY_HIGH"],
            ..all.clone()
        };
        assert_eq!(ids(&api.system_log(&q).unwrap()), ["03", "01"]);

        let q = LogQuery {
            categories: vec!["SECURITY", "AUDIT"],
            ..all.clone()
        };
        assert_eq!(ids(&api.system_log(&q).unwrap()), ["05", "04", "03"]);

        let q = LogQuery {
            page_size: 3,
            page_number: 3,
            ..all
        };
        let page = api.system_log(&q).unwrap();
        assert_eq!(ids(&page), ["01"]);
        assert_eq!(page.total_page_count, 4);
    }

    #[test]
    fn optional_fixtures_may_be_absent() {
        let dir = tempfile::tempdir().unwrap();
        for f in [
            "unifi-devices.json",
            "unifi-clients.json",
            "unifi-v2-active.json",
            "unifi-v2-history.json",
            "unifi-wlanconf.json",
            "unifi-networkconf.json",
        ] {
            std::fs::copy(FixtureApi::default_dir().join(f), dir.path().join(f)).unwrap();
        }
        let api = FixtureApi::load(dir.path()).unwrap();
        assert!(api.log_entries.is_empty());
        assert!(api.traffic.client_usage_by_app.is_empty());
        assert!(api.traffic(0, 1).unwrap().total_usage_by_app.is_empty());
        let err = api.dpi_categories().unwrap_err().to_string();
        assert!(err.contains("needs an API key"), "{err}");
        assert!(api.dpi_applications().is_err());
    }
}
