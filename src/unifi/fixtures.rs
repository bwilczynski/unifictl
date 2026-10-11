//! Fixture-backed `UnifiApi` for unit tests and the hidden `--fixtures` flag.

use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;

use super::UnifiApi;
use super::types::{
    ClientV2, Device, Envelope, LogQuery, NetworkConf, Sta, SystemLogEntry, SystemLogPage, WlanConf,
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
    /// When true every call fails, simulating an unreachable controller.
    pub fail: bool,
}

impl FixtureApi {
    /// `tests/fixtures` inside this crate.
    pub fn default_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    /// Loads the `unifi-*.json` files (each `{"data": [...]}`) from `dir`; the
    /// system log is optional.
    pub fn load(dir: &Path) -> Result<Self> {
        Ok(Self {
            devices: read(dir, "unifi-devices.json")?,
            clients: read(dir, "unifi-clients.json")?,
            active: read(dir, "unifi-v2-active.json")?,
            offline: read(dir, "unifi-v2-history.json")?,
            wlans: read(dir, "unifi-wlanconf.json")?,
            networks: read(dir, "unifi-networkconf.json")?,
            log_entries: read_optional(dir, "unifi-system-log.json")?,
            fail: false,
        })
    }

    fn check(&self) -> Result<()> {
        if self.fail {
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

impl UnifiApi for FixtureApi {
    fn devices(&self) -> Result<Vec<Device>> {
        self.check()?;
        Ok(self.devices.clone())
    }
    fn clients(&self) -> Result<Vec<Sta>> {
        self.check()?;
        Ok(self.clients.clone())
    }
    fn active_clients(&self) -> Result<Vec<ClientV2>> {
        self.check()?;
        Ok(self.active.clone())
    }
    fn offline_clients(&self, _history_days: i64) -> Result<Vec<ClientV2>> {
        self.check()?;
        Ok(self.offline.clone())
    }
    fn wlan_conf(&self) -> Result<Vec<WlanConf>> {
        self.check()?;
        Ok(self.wlans.clone())
    }
    fn network_conf(&self) -> Result<Vec<NetworkConf>> {
        self.check()?;
        Ok(self.networks.clone())
    }

    /// Applies the window, severities and categories like the controller,
    /// newest first, then pages.
    fn system_log(&self, q: &LogQuery) -> Result<SystemLogPage> {
        self.check()?;
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
    fn missing_log_fixture_is_an_empty_log() {
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
    }
}
