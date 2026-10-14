//! Network domain logic, ported from homelab-api/internal/network. One
//! controller per `Service`; IDs carry no controller prefix.

mod clients;
mod common;
mod devices;
mod logs;
mod ports;
mod ssids;
mod topology;
mod traffic;
mod vlans;
mod wans;

pub use logs::{FOLLOW_INTERVAL, Follower, LogFilter, LogPage, RefIndex, backoff, follow_loop};
pub use ports::PortFilter;
pub use traffic::{TrafficView, TrafficWindow};
pub use wans::DEFAULT_OUTAGE_WINDOW_SECONDS;

use crate::unifi::UnifiApi;

pub const DEFAULT_HISTORY_DAYS: i64 = 30;

pub struct Service {
    pub(crate) api: Box<dyn UnifiApi>,
    /// Days of offline-client history to include.
    pub(crate) history_days: i64,
}

impl Service {
    pub fn new(api: Box<dyn UnifiApi>, history_days: i64) -> Self {
        let history_days = if history_days <= 0 {
            DEFAULT_HISTORY_DAYS
        } else {
            history_days
        };
        Self { api, history_days }
    }
}

#[cfg(test)]
pub(crate) mod testutil {
    use super::Service;
    use crate::unifi::fixtures::FixtureApi;

    pub(crate) fn fixtures() -> FixtureApi {
        FixtureApi::load(&FixtureApi::default_dir()).expect("load fixtures")
    }

    pub(crate) fn svc(api: FixtureApi) -> Service {
        Service::new(Box::new(api), 30)
    }
}
