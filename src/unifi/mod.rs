pub mod client;
pub mod types;

#[cfg(any(test, feature = "fixtures"))]
pub mod fixtures;

use crate::error::Result;
use types::{ClientV2, Device, NetworkConf, Sta, WlanConf};

/// The six controller reads the network domain needs. Implemented by the HTTP
/// client and by `FixtureApi`.
pub trait UnifiApi {
    fn devices(&self) -> Result<Vec<Device>>;
    fn clients(&self) -> Result<Vec<Sta>>;
    fn active_clients(&self) -> Result<Vec<ClientV2>>;
    fn offline_clients(&self, history_days: i64) -> Result<Vec<ClientV2>>;
    fn wlan_conf(&self) -> Result<Vec<WlanConf>>;
    fn network_conf(&self) -> Result<Vec<NetworkConf>>;

    /// Active followed by offline clients (homelab-api `GetAllClients`).
    fn all_clients(&self, history_days: i64) -> Result<Vec<ClientV2>> {
        let mut all = self.active_clients()?;
        all.extend(self.offline_clients(history_days)?);
        Ok(all)
    }
}
