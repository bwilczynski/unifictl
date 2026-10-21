//! WANs: ported from homelab-api/internal/network/wans_service.go.

use jiff::Timestamp;
use jiff::tz::TimeZone;

use super::Service;
use super::common::*;
use crate::error::{Error, Result};
use crate::model::*;
use crate::unifi::fetch;
use crate::unifi::types::{Device, NetworkConf, WanHistory, WanIface};

/// `wans get` outage window when `--since` is not given: 7 days.
pub const DEFAULT_OUTAGE_WINDOW_SECONDS: i64 = 7 * 24 * 60 * 60;

/// A history window that starts within this of the requested start covers
/// the whole of it: the controller aligns the start to a bucket.
const WINDOW_ALIGNMENT_SLACK_MS: i64 = 60 * 60 * 1000;

impl Service {
    pub fn list_wans(&self) -> Result<WanList> {
        fetch!(
            networks = self.api.network_conf(),
            devices = self.api.devices(),
            day = Ok::<_, Error>(self.api.wan_history(AVAILABILITY_WINDOW_SECONDS).ok()),
        );
        let gw = find_gateway(&devices);
        let items = networks
            .iter()
            .filter(|n| n.purpose == "wan")
            .map(|n| build_wan(n, gw, day.as_ref()))
            .collect();
        Ok(WanList { items })
    }

    /// The WAN with its outages over the last `history_seconds`. A failed
    /// history read leaves `outages` out rather than failing the command.
    /// Availability is read over its own 24h window, as in `list_wans`.
    pub fn get_wan(&self, id: &str, history_seconds: i64, tz: &TimeZone) -> Result<WanDetail> {
        fetch!(
            networks = self.api.network_conf(),
            devices = self.api.devices(),
            history = Ok::<_, Error>(self.api.wan_history(history_seconds).ok()),
            day = Ok::<_, Error>(self.api.wan_history(AVAILABILITY_WINDOW_SECONDS).ok()),
        );
        let gw = find_gateway(&devices);
        let n = networks
            .iter()
            .filter(|n| n.purpose == "wan")
            .find(|n| to_kebab(&n.name) == id)
            .ok_or_else(|| Error::NotFound(format!("WAN not found: {id}")))?;
        Ok(WanDetail {
            wan: build_wan(n, gw, day.as_ref()),
            outages: history.and_then(|h| outages(&h, &n.wan_networkgroup, tz)),
        })
    }
}

/// `day` is the WAN history over the last `AVAILABILITY_WINDOW_SECONDS`.
/// Its availability wins over the gateway's `uptime_stats`, whose period
/// restarts each time the WAN comes back up: seconds after an outage it
/// reports 100% over those seconds.
pub(super) fn build_wan(n: &NetworkConf, gw: Option<&Device>, day: Option<&WanHistory>) -> Wan {
    let iface = resolve_iface(gw, &n.wan_networkgroup);
    let stats = gw.and_then(|g| g.uptime_stats.get(&n.wan_networkgroup));
    let (availability, availability_period) = day
        .and_then(|h| history_availability(h, &n.wan_networkgroup))
        .map(|(a, p)| (Some(a), Some(p)))
        .unwrap_or_else(|| {
            (
                stats.and_then(|s| s.availability),
                stats.and_then(|s| s.time_period),
            )
        });
    let status = if iface.is_some_and(|i| i.up) {
        WanStatus::Connected
    } else {
        WanStatus::Disconnected
    };
    Wan {
        id: to_kebab(&n.name),
        name: n.name.clone(),
        ip_address: iface.map(|i| i.ip.clone()).unwrap_or_default(),
        status,
        uptime: stats.and_then(|s| s.uptime),
        downtime: stats.and_then(|s| s.downtime),
        availability,
        availability_period,
        latency: stats
            .and_then(|s| s.latency_average)
            .map(|ms| ms.round() as i64),
        dns_servers: dns_servers(iface, n),
    }
}

/// `group`'s availability over `h`'s window and that window in seconds:
/// `AVAILABILITY_WINDOW_SECONDS` unless the controller's history is shorter.
/// `None` without a window or an entry with a figure.
fn history_availability(h: &WanHistory, group: &str) -> Option<(f64, i64)> {
    let meta = h.dashboard_meta.as_ref()?;
    let covered_ms = Some(meta.end_timestamp - meta.start_timestamp)
        .filter(|&ms| meta.start_timestamp > 0 && ms > 0)?;
    let availability = h
        .wan_history
        .as_ref()?
        .wan_history_details
        .iter()
        .find(|d| d.network_group == group)?
        .uptime?;
    let period = if covered_ms + WINDOW_ALIGNMENT_SLACK_MS >= AVAILABILITY_WINDOW_SECONDS * 1000 {
        AVAILABILITY_WINDOW_SECONDS
    } else {
        covered_ms / 1000
    };
    Some((availability, period))
}

/// `group`'s outages, oldest first; `None` without a window start or an
/// entry.
pub(super) fn outages(h: &WanHistory, group: &str, tz: &TimeZone) -> Option<WanOutages> {
    let since_ms = Some(h.dashboard_meta.as_ref()?.start_timestamp).filter(|&t| t > 0)?;
    let detail = h
        .wan_history
        .as_ref()?
        .wan_history_details
        .iter()
        .find(|d| d.network_group == group)?;
    let mut items: Vec<WanOutage> = detail
        .downtime_history
        .iter()
        .map(|d| {
            let end_ms = d.timestamp + d.downtime_in_seconds * 1000;
            WanOutage {
                start: rfc3339(d.timestamp, tz),
                end: rfc3339(end_ms, tz),
                duration_seconds: d.downtime_in_seconds,
                start_ms: d.timestamp,
                end_ms,
            }
        })
        .collect();
    items.sort_by_key(|o| o.start_ms);
    Some(WanOutages {
        since: rfc3339(since_ms, tz),
        availability: detail.uptime,
        items,
        since_ms,
    })
}

fn rfc3339(ms: i64, tz: &TimeZone) -> String {
    Timestamp::from_millisecond(ms)
        .unwrap_or(Timestamp::UNIX_EPOCH)
        .to_zoned(tz.clone())
        .strftime("%Y-%m-%dT%H:%M:%S%:z")
        .to_string()
}

pub(super) fn find_gateway(devices: &[Device]) -> Option<&Device> {
    devices.iter().find(|d| is_gateway_type(&d.kind))
}

/// "WAN" → wan1, "WAN2" → wan2.
fn resolve_iface<'a>(gw: Option<&'a Device>, group: &str) -> Option<&'a WanIface> {
    let gw = gw?;
    match group {
        "WAN" => gw.wan1.as_ref(),
        "WAN2" => gw.wan2.as_ref(),
        _ => None,
    }
}

/// Configured DNS wins; otherwise what the interface reports.
fn dns_servers(iface: Option<&WanIface>, n: &NetworkConf) -> Vec<String> {
    let configured = collect_dns(&[&n.wan_dns1, &n.wan_dns2]);
    if !configured.is_empty() {
        return configured;
    }
    iface
        .map(|i| collect_dns(&i.dns.iter().map(String::as_str).collect::<Vec<_>>()))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use crate::service::testutil::{fixtures, svc};
    use crate::unifi::fixtures::FixtureApi;
    use jiff::tz::TimeZone;

    const WEEK: i64 = DEFAULT_OUTAGE_WINDOW_SECONDS;

    fn gateway(api: &mut FixtureApi) -> &mut crate::unifi::types::Device {
        api.devices.iter_mut().find(|d| d.kind == "udm").unwrap()
    }

    fn detail(api: FixtureApi, id: &str) -> WanDetail {
        svc(api).get_wan(id, WEEK, &TimeZone::UTC).unwrap()
    }

    #[test]
    fn list() {
        let items = svc(fixtures()).list_wans().unwrap().items;
        assert_eq!(items.len(), 2);
        let w = &items[0];
        assert_eq!(w.id, "internet-1");
        assert_eq!(w.name, "Internet 1");
        assert_eq!(w.ip_address, "203.0.113.1");
        assert_eq!(w.status, WanStatus::Connected);
    }

    #[test]
    fn uptime_is_the_wans_own_not_the_gateways() {
        let items = svc(fixtures()).list_wans().unwrap().items;
        let (up, down) = (&items[0], &items[1]);
        assert_eq!(up.uptime, Some(4253), "gateway uptime is 5627");
        assert_eq!(up.downtime, None);
        assert_eq!(up.latency, Some(13));
        assert_eq!(down.status, WanStatus::Disconnected);
        assert_eq!(down.uptime, None);
        assert_eq!(down.downtime, Some(5577));
        assert_eq!(down.availability, None);
        assert_eq!(down.latency, None);
    }

    fn history_detail(api: &mut FixtureApi) -> &mut crate::unifi::types::WanHistoryDetail {
        &mut api
            .wan_history
            .as_mut()
            .unwrap()
            .wan_history
            .as_mut()
            .unwrap()
            .wan_history_details[0]
    }

    #[test]
    fn availability_is_the_last_24h_from_the_history() {
        let items = svc(fixtures()).list_wans().unwrap().items;
        assert_eq!(items[0].availability, Some(99.95), "live figure is 100.0");
        assert_eq!(items[0].availability_period, Some(86400));
        assert_eq!(items[1].availability, None, "no history entry, down");
    }

    #[test]
    fn a_reconnect_does_not_reset_availability() {
        let mut api = fixtures();
        let stats = gateway(&mut api).uptime_stats.get_mut("WAN").unwrap();
        stats.availability = Some(100.0);
        stats.time_period = Some(58);
        history_detail(&mut api).uptime = Some(99.82);
        let w = &svc(api).list_wans().unwrap().items[0];
        assert_eq!(w.availability, Some(99.82));
        assert_eq!(w.availability_period, Some(86400));
    }

    #[test]
    fn availability_falls_back_to_the_live_figure_without_history() {
        let api = FixtureApi {
            wan_history: None,
            ..fixtures()
        };
        let w = &svc(api).list_wans().unwrap().items[0];
        assert_eq!(w.availability, Some(100.0));
        assert_eq!(w.availability_period, Some(4270));

        let mut api = fixtures();
        history_detail(&mut api).uptime = None;
        let w = &svc(api).list_wans().unwrap().items[0];
        assert_eq!(w.availability_period, Some(4270), "entry without a figure");

        let mut api = fixtures();
        api.wan_history.as_mut().unwrap().dashboard_meta = None;
        let w = &svc(api).list_wans().unwrap().items[0];
        assert_eq!(w.availability_period, Some(4270), "no window");
    }

    #[test]
    fn short_history_reports_the_window_it_covers() {
        let mut api = fixtures();
        let meta = api
            .wan_history
            .as_mut()
            .unwrap()
            .dashboard_meta
            .as_mut()
            .unwrap();
        meta.start_timestamp = meta.end_timestamp - 3 * 3_600_000;
        let w = &svc(api).list_wans().unwrap().items[0];
        assert_eq!(w.availability, Some(99.95));
        assert_eq!(w.availability_period, Some(3 * 3600));
    }

    #[test]
    fn a_bucket_aligned_start_still_covers_the_day() {
        let mut api = fixtures();
        let meta = api
            .wan_history
            .as_mut()
            .unwrap()
            .dashboard_meta
            .as_mut()
            .unwrap();
        meta.start_timestamp = meta.end_timestamp - 86_400_000 + 1_800_000;
        let w = &svc(api).list_wans().unwrap().items[0];
        assert_eq!(w.availability_period, Some(86400));
    }

    fn api_without_uptime_stats() -> FixtureApi {
        let mut api = fixtures();
        for d in &mut api.devices {
            d.uptime_stats.clear();
        }
        api
    }

    #[test]
    fn gateway_without_uptime_stats_reports_nothing() {
        let api = api_without_uptime_stats();
        let w = &svc(api).list_wans().unwrap().items[0];
        assert_eq!(w.status, WanStatus::Connected);
        assert_eq!(w.uptime, None, "never the gateway's uptime");
        assert_eq!(w.availability, Some(99.95), "from the history");
        assert_eq!(w.latency, None);

        let api = FixtureApi {
            wan_history: None,
            ..api_without_uptime_stats()
        };
        assert_eq!(svc(api).list_wans().unwrap().items[0].availability, None);
    }

    #[test]
    fn list_empty() {
        assert!(
            svc(FixtureApi::default())
                .list_wans()
                .unwrap()
                .items
                .is_empty()
        );
    }

    #[test]
    fn no_gateway_means_disconnected() {
        let api = FixtureApi {
            devices: vec![],
            ..fixtures()
        };
        let w = &svc(api).list_wans().unwrap().items[0];
        assert_eq!(w.status, WanStatus::Disconnected);
        assert_eq!(w.ip_address, "");
        assert_eq!(w.uptime, None);
    }

    #[test]
    fn get() {
        let d = detail(fixtures(), "internet-1");
        assert_eq!(d.wan.ip_address, "203.0.113.1");
        assert_eq!(d.wan.status, WanStatus::Connected);
        assert_eq!(d.wan.dns_servers, ["8.8.8.8", "8.8.4.4"]);
    }

    #[test]
    fn outages_come_from_the_wans_history_entry() {
        let o = detail(fixtures(), "internet-1").outages.unwrap();
        assert_eq!(o.since, "2026-10-05T00:00:00+00:00");
        assert_eq!(o.since_ms, 1791158400000);
        assert_eq!(o.availability, Some(99.95));
        assert_eq!(o.items.len(), 2);
        let first = &o.items[0];
        assert_eq!(first.start, "2026-10-06T03:12:45+00:00");
        assert_eq!(first.end, "2026-10-06T03:14:20+00:00");
        assert_eq!(first.duration_seconds, 95);
        assert_eq!(first.end_ms - first.start_ms, 95_000);
    }

    #[test]
    fn outages_are_oldest_first() {
        let mut api = fixtures();
        let h = api.wan_history.as_mut().unwrap();
        h.wan_history.as_mut().unwrap().wan_history_details[0]
            .downtime_history
            .reverse();
        let o = detail(api, "internet-1").outages.unwrap();
        assert!(o.items[0].start_ms < o.items[1].start_ms);
    }

    #[test]
    fn wan_without_a_history_entry_has_no_outages() {
        assert_eq!(detail(fixtures(), "internet-2").outages, None);
    }

    #[test]
    fn missing_meta_means_no_outages() {
        let mut api = fixtures();
        api.wan_history.as_mut().unwrap().dashboard_meta = None;
        assert_eq!(detail(api, "internet-1").outages, None);
    }

    #[test]
    fn meta_without_a_start_means_no_outages() {
        let mut api = fixtures();
        api.wan_history.as_mut().unwrap().dashboard_meta = Some(Default::default());
        assert_eq!(detail(api, "internet-1").outages, None);
    }

    #[test]
    fn missing_history_keeps_the_live_fields() {
        let api = FixtureApi {
            wan_history: None,
            ..fixtures()
        };
        let d = detail(api, "internet-1");
        assert_eq!(d.outages, None);
        assert_eq!(d.wan.status, WanStatus::Connected);
        assert_eq!(d.wan.uptime, Some(4253));
    }

    #[test]
    fn no_outages_is_an_empty_list_not_none() {
        let mut api = fixtures();
        api.wan_history
            .as_mut()
            .unwrap()
            .wan_history
            .as_mut()
            .unwrap()
            .wan_history_details[0]
            .downtime_history = vec![];
        let o = detail(api, "internet-1").outages.unwrap();
        assert!(o.items.is_empty());
        assert_eq!(o.availability, Some(99.95));
    }

    #[test]
    fn get_not_found() {
        let err = svc(fixtures())
            .get_wan("nonexistent", WEEK, &TimeZone::UTC)
            .unwrap_err();
        assert!(matches!(err, Error::NotFound(_)));
        assert_eq!(err.to_string(), "WAN not found: nonexistent");
    }

    #[test]
    fn list_item_equals_get() {
        let s = svc(fixtures());
        for w in s.list_wans().unwrap().items {
            let d = s.get_wan(&w.id, WEEK, &TimeZone::UTC).unwrap();
            assert_eq!(d.wan, w, "{}", w.id);
        }
    }

    #[test]
    fn local_offset_in_timestamps() {
        let tz = TimeZone::fixed(jiff::tz::offset(2));
        let d = svc(fixtures()).get_wan("internet-1", WEEK, &tz).unwrap();
        assert_eq!(d.outages.unwrap().since, "2026-10-05T02:00:00+02:00");
    }
}
