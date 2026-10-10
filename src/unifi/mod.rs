pub mod client;
pub mod types;

#[cfg(any(test, feature = "fixtures"))]
pub mod fixtures;

use std::thread::ScopedJoinHandle;

use crate::error::Result;
use types::{ClientV2, Device, NetworkConf, Sta, WlanConf};

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

/// The six controller reads the network domain needs. Implemented by the HTTP
/// client and by `FixtureApi`. `Sync` so independent reads can run on
/// parallel threads (see [`fetch!`]).
pub trait UnifiApi: Sync {
    fn devices(&self) -> Result<Vec<Device>>;
    fn clients(&self) -> Result<Vec<Sta>>;
    fn active_clients(&self) -> Result<Vec<ClientV2>>;
    fn offline_clients(&self, history_days: i64) -> Result<Vec<ClientV2>>;
    fn wlan_conf(&self) -> Result<Vec<WlanConf>>;
    fn network_conf(&self) -> Result<Vec<NetworkConf>>;

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
