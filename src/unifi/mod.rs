pub mod client;
pub mod types;

#[cfg(any(test, feature = "fixtures"))]
pub mod fixtures;

use std::thread::ScopedJoinHandle;

use crate::error::Result;
use types::{
    ClientV2, Device, DpiEntry, LogQuery, NetworkConf, Sta, SystemLogPage, TrafficReport,
    WanHistory, WlanConf,
};

/// Runs independent controller reads concurrently, so a command waits for
/// the slowest endpoint rather than the sum of all of them, and binds each
/// result to its name:
///
/// ```ignore
/// fetch!(devices = self.api.devices(), clients = self.api.clients());
/// ```
///
/// Returns the first error in argument order from the enclosing function.
macro_rules! fetch {
    ($($name:ident = $read:expr),+ $(,)?) => {
        let ($($name,)+) = std::thread::scope(|s| {
            $(let $name = s.spawn(|| $read);)+
            Ok::<_, $crate::error::Error>(($($crate::unifi::joined($name)?,)+))
        })?;
    };
}
pub(crate) use fetch;

/// Result of a `fetch!` thread; a panic in the read is re-raised here.
pub(crate) fn joined<T>(handle: ScopedJoinHandle<'_, Result<T>>) -> Result<T> {
    handle
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
}

/// The controller reads the network domain needs. Implemented by the HTTP
/// client and by `FixtureApi`. `Sync` so independent reads can run on
/// parallel threads (see [`fetch!`]).
pub trait UnifiApi: Sync {
    fn devices(&self) -> Result<Vec<Device>>;
    fn clients(&self) -> Result<Vec<Sta>>;
    fn active_clients(&self) -> Result<Vec<ClientV2>>;
    fn offline_clients(&self, history_days: i64) -> Result<Vec<ClientV2>>;
    fn wlan_conf(&self) -> Result<Vec<WlanConf>>;
    fn network_conf(&self) -> Result<Vec<NetworkConf>>;
    /// One page of the system log. Never cached.
    fn system_log(&self, query: &LogQuery) -> Result<SystemLogPage>;
    /// Traffic per client and application between two epoch milliseconds.
    /// Never cached.
    fn traffic(&self, start_ms: i64, end_ms: i64) -> Result<TrafficReport>;
    /// The dashboard's WAN history over the last `history_seconds`. Never
    /// cached.
    fn wan_history(&self, history_seconds: i64) -> Result<WanHistory>;
    /// DPI categories by number; needs an API key (Integration API).
    fn dpi_categories(&self) -> Result<Vec<DpiEntry>>;
    /// DPI applications by `category << 16 | application`; needs an API key.
    fn dpi_applications(&self) -> Result<Vec<DpiEntry>>;

    /// Active followed by offline clients (homelab-api `GetAllClients`).
    fn all_clients(&self, history_days: i64) -> Result<Vec<ClientV2>> {
        fetch!(
            active = self.active_clients(),
            offline = self.offline_clients(history_days),
        );
        let mut all = active;
        all.extend(offline);
        Ok(all)
    }
}
