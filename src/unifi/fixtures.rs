//! Fixture-backed `UnifiApi` for unit tests and the hidden `--fixtures` flag.

use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;

use super::UnifiApi;
use super::types::{ClientV2, Device, Envelope, NetworkConf, Sta, WlanConf};
use crate::error::{Error, Result};

#[derive(Debug, Clone, Default)]
pub struct FixtureApi {
    pub devices: Vec<Device>,
    pub clients: Vec<Sta>,
    pub active: Vec<ClientV2>,
    pub offline: Vec<ClientV2>,
    pub wlans: Vec<WlanConf>,
    pub networks: Vec<NetworkConf>,
    /// When true every call fails, simulating an unreachable controller.
    pub fail: bool,
}

impl FixtureApi {
    /// `tests/fixtures` inside this crate.
    pub fn default_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    /// Loads the six `unifi-*.json` files (each `{"data": [...]}`) from `dir`.
    pub fn load(dir: &Path) -> Result<Self> {
        Ok(Self {
            devices: read(dir, "unifi-devices.json")?,
            clients: read(dir, "unifi-clients.json")?,
            active: read(dir, "unifi-v2-active.json")?,
            offline: read(dir, "unifi-v2-history.json")?,
            wlans: read(dir, "unifi-wlanconf.json")?,
            networks: read(dir, "unifi-networkconf.json")?,
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
