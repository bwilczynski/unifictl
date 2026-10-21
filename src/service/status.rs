//! `unifictl status`: a verdict for each area of the site, from the reads
//! the other commands make plus `stat/health`.

use jiff::tz::TimeZone;

use super::Service;
use super::clients::network_client;
use super::common::{device_ref, map_device_type};
use super::logs::{LogFilter, LogScope, to_model};
use super::wans::{build_wan, find_gateway, outages};
use crate::error::{Error, Result};
use crate::model::{
    DeviceRef, DeviceTransition, DeviceType, FirmwareUpdate, LogSeverity, PoorClient, SiteStatus,
    StatusArea, StatusRow, Verdict, Wan, WanOutage, WanStatus,
};
use crate::render::health::{self, Health};
use crate::unifi::fetch;
use crate::unifi::types::{
    ClientV2, Device, HealthSubsystem, NetworkConf, SystemLogEntry, WanHistory,
};

/// The window for outages and alarms: the last 24 hours.
pub const STATUS_WINDOW_SECONDS: i64 = 24 * 60 * 60;

// The WAN history read for outages also gives availability, which
// `build_wan` labels as `AVAILABILITY_WINDOW_SECONDS`.
const _: () = assert!(STATUS_WINDOW_SECONDS == crate::model::AVAILABILITY_WINDOW_SECONDS);

/// A WAN down for at least the gateway's uptime minus this has been down
/// since boot: an unplugged backup rather than an outage.
const SINCE_BOOT_SLACK_SECONDS: i64 = 300;

impl Service {
    /// A row per area as of `now_ms`. The device list and network config
    /// are required; health, clients, WAN history and the system log are
    /// not, and leave their rows `unknown` or without that check when they
    /// cannot be read.
    pub fn status(&self, now_ms: i64, tz: &TimeZone) -> Result<SiteStatus> {
        fetch!(
            devices = self.api.devices(),
            networks = self.api.network_conf(),
            clients = Ok::<_, Error>(self.api.active_clients().ok()),
            health = Ok::<_, Error>(self.api.health().ok()),
            history = Ok::<_, Error>(self.api.wan_history(STATUS_WINDOW_SECONDS).ok()),
            alarms = Ok::<_, Error>(self.alarm_log(now_ms).ok()),
        );
        let sub = Subsystems(health.as_deref().unwrap_or_default());
        let since_ms = now_ms - STATUS_WINDOW_SECONDS * 1000;
        let mut rows = wan_rows(&networks, &devices, history.as_ref(), &sub, since_ms, tz);
        rows.push(devices_row(&devices));
        rows.push(wifi_row(&devices, clients.as_deref(), &sub));
        rows.push(wired_row(&devices, clients.as_deref(), &sub));
        rows.push(alarms_row(alarms.as_deref(), tz));
        rows.push(updates_row(&devices));
        Ok(SiteStatus { rows })
    }

    /// High and very-high system log entries over the window, newest first.
    fn alarm_log(&self, now_ms: i64) -> Result<Vec<SystemLogEntry>> {
        let filter = LogFilter {
            min_severity: Some(LogSeverity::High),
            ..Default::default()
        };
        let from = now_ms - STATUS_WINDOW_SECONDS * 1000;
        let (entries, _) =
            self.collect_log(&filter, Some(from), now_ms, &LogScope::default(), None)?;
        Ok(entries)
    }
}

/// `stat/health` by subsystem; empty when it could not be read.
struct Subsystems<'a>(&'a [HealthSubsystem]);

impl Subsystems<'_> {
    /// What `stat/health` says about `name`: `ok` unless it reports a
    /// warning or an error.
    fn verdict(&self, name: &str) -> Verdict {
        match self
            .0
            .iter()
            .find(|s| s.subsystem == name)
            .map(|s| s.status.as_str())
        {
            Some("warning") => Verdict::Warn,
            Some("error") => Verdict::Error,
            _ => Verdict::Ok,
        }
    }
}

fn row(verdict: Verdict, area: StatusArea) -> StatusRow {
    StatusRow {
        verdict,
        unavailable: None,
        area,
    }
}

/// A row whose data could not be read.
fn unavailable(what: &str, area: StatusArea) -> StatusRow {
    StatusRow {
        verdict: Verdict::Unknown,
        unavailable: Some(format!("couldn't read {what}")),
        area,
    }
}

fn wan_rows(
    networks: &[NetworkConf],
    devices: &[Device],
    history: Option<&WanHistory>,
    sub: &Subsystems,
    since_ms: i64,
    tz: &TimeZone,
) -> Vec<StatusRow> {
    let gw = find_gateway(devices);
    networks
        .iter()
        .filter(|n| n.purpose == "wan")
        .map(|n| {
            let wan = build_wan(n, gw, history);
            let outages_24h: Vec<WanOutage> = history
                .and_then(|h| outages(h, &n.wan_networkgroup, tz))
                .map(|o| {
                    o.items
                        .into_iter()
                        .filter(|o| o.end_ms >= since_ms)
                        .collect()
                })
                .unwrap_or_default();
            let verdict = wan_verdict(&wan, &outages_24h, gw.map(|g| g.uptime), sub);
            row(verdict, StatusArea::Wan { wan, outages_24h })
        })
        .collect()
}

fn wan_verdict(
    wan: &Wan,
    outages_24h: &[WanOutage],
    gateway_uptime: Option<i64>,
    sub: &Subsystems,
) -> Verdict {
    if wan.status == WanStatus::Disconnected {
        let since_boot = match (wan.downtime, gateway_uptime) {
            (Some(down), Some(up)) => down + SINCE_BOOT_SLACK_SECONDS >= up,
            _ => true,
        };
        return if since_boot {
            Verdict::Idle
        } else {
            Verdict::Error
        };
    }
    // Graded on the displayed value, like `style::availability`.
    let availability = wan
        .availability
        .map(|p| health::availability((p * 100.0).round() / 100.0));
    let latency = wan.latency.map(health::latency);
    let degraded = [availability, latency]
        .into_iter()
        .flatten()
        .any(|g| g != Health::Good);
    let own = if degraded || !outages_24h.is_empty() {
        Verdict::Warn
    } else {
        Verdict::Ok
    };
    own.worse(sub.verdict("wan")).worse(sub.verdict("www"))
}

/// Controller device states on the way to connected.
fn transition(state: i64) -> Option<&'static str> {
    match state {
        2 => Some("pending"),
        4 => Some("upgrading"),
        5 => Some("provisioning"),
        7 => Some("adopting"),
        _ => None,
    }
}

/// Connected is state 1; anything neither connected nor in transition
/// (disconnected, heartbeat missed, adoption failed, isolated, …) is
/// offline.
fn devices_row(devices: &[Device]) -> StatusRow {
    let online = devices.iter().filter(|d| d.state == 1).count();
    let transitioning: Vec<DeviceTransition> = devices
        .iter()
        .filter_map(|d| {
            transition(d.state).map(|state| DeviceTransition {
                device: device_ref(d),
                state,
            })
        })
        .collect();
    let offline: Vec<DeviceRef> = devices
        .iter()
        .filter(|d| d.state != 1 && transition(d.state).is_none())
        .map(device_ref)
        .collect();
    let verdict = if !offline.is_empty() {
        Verdict::Error
    } else if !transitioning.is_empty() {
        Verdict::Warn
    } else {
        Verdict::Ok
    };
    row(
        verdict,
        StatusArea::Devices {
            online,
            total: devices.len(),
            offline,
            transitioning,
        },
    )
}

fn count_type(devices: &[Device], t: DeviceType) -> usize {
    devices
        .iter()
        .filter(|d| map_device_type(&d.kind) == t)
        .count()
}

fn wifi_row(devices: &[Device], clients: Option<&[ClientV2]>, sub: &Subsystems) -> StatusRow {
    let access_points = count_type(devices, DeviceType::AccessPoint);
    let Some(clients) = clients else {
        return unavailable(
            "clients",
            StatusArea::Wifi {
                access_points,
                clients: None,
                poor_clients: Vec::new(),
            },
        );
    };
    let wireless: Vec<_> = clients
        .iter()
        .filter(|c| !c.is_wired)
        .map(network_client)
        .collect();
    let mut poor_clients: Vec<PoorClient> = wireless
        .iter()
        .filter_map(|c| {
            let experience = c.experience?;
            (health::experience(experience) == Health::Poor).then(|| PoorClient {
                id: c.id.clone(),
                name: c.name.clone(),
                experience,
            })
        })
        .collect();
    poor_clients.sort_by(|a, b| (a.experience, &a.id).cmp(&(b.experience, &b.id)));
    let own = if poor_clients.is_empty() {
        Verdict::Ok
    } else {
        Verdict::Warn
    };
    row(
        own.worse(sub.verdict("wlan")),
        StatusArea::Wifi {
            access_points,
            clients: Some(wireless.len()),
            poor_clients,
        },
    )
}

fn wired_row(devices: &[Device], clients: Option<&[ClientV2]>, sub: &Subsystems) -> StatusRow {
    let switches = count_type(devices, DeviceType::Switch);
    let Some(clients) = clients else {
        return unavailable(
            "clients",
            StatusArea::Wired {
                switches,
                clients: None,
            },
        );
    };
    row(
        sub.verdict("lan"),
        StatusArea::Wired {
            switches,
            clients: Some(clients.iter().filter(|c| c.is_wired).count()),
        },
    )
}

fn alarms_row(log: Option<&[SystemLogEntry]>, tz: &TimeZone) -> StatusRow {
    let Some(log) = log else {
        return unavailable(
            "the system log",
            StatusArea::Alarms {
                count: None,
                latest: None,
            },
        );
    };
    // Entries are history; `error` is kept for what is wrong now.
    let verdict = if log.is_empty() {
        Verdict::Ok
    } else {
        Verdict::Warn
    };
    row(
        verdict,
        StatusArea::Alarms {
            count: Some(log.len()),
            latest: log.first().map(|e| to_model(e, None, tz)),
        },
    )
}

/// Informational: an available update is never a problem.
fn updates_row(devices: &[Device]) -> StatusRow {
    let updates: Vec<FirmwareUpdate> = devices
        .iter()
        .filter(|d| d.upgradable && !d.upgrade_to_firmware.is_empty())
        .map(|d| FirmwareUpdate {
            device: device_ref(d),
            from: d.version.clone(),
            to: d.upgrade_to_firmware.clone(),
        })
        .collect();
    let verdict = if updates.is_empty() {
        Verdict::Ok
    } else {
        Verdict::Info
    };
    row(verdict, StatusArea::Updates { updates })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::testutil::{fixtures, svc};
    use crate::unifi::fixtures::FixtureApi;
    use crate::unifi::types::HealthSubsystem;
    use jiff::tz::TimeZone;

    /// `UNIFICTL_NOW` in the CLI tests: 2026-10-11T12:00:00Z.
    const NOW: i64 = 1_791_720_000_000;

    fn status(api: FixtureApi) -> SiteStatus {
        svc(api).status(NOW, &TimeZone::UTC).unwrap()
    }

    fn verdicts(s: &SiteStatus) -> Vec<Verdict> {
        s.rows.iter().map(|r| r.verdict).collect()
    }

    /// The first row of `name`.
    fn area<'a>(s: &'a SiteStatus, name: &str) -> &'a StatusRow {
        s.rows.iter().find(|r| r.area.name() == name).unwrap()
    }

    fn subsystems(entries: &[(&str, &str)]) -> Option<Vec<HealthSubsystem>> {
        Some(
            entries
                .iter()
                .map(|(s, st)| HealthSubsystem {
                    subsystem: s.to_string(),
                    status: st.to_string(),
                })
                .collect(),
        )
    }

    fn gateway(api: &mut FixtureApi) -> &mut Device {
        api.devices.iter_mut().find(|d| d.kind == "udm").unwrap()
    }

    #[test]
    fn worse_ranks_error_over_warn_over_the_rest() {
        assert_eq!(Verdict::Ok.worse(Verdict::Warn), Verdict::Warn);
        assert_eq!(Verdict::Error.worse(Verdict::Warn), Verdict::Error);
        assert_eq!(Verdict::Warn.worse(Verdict::Ok), Verdict::Warn);
        assert_eq!(Verdict::Info.worse(Verdict::Ok), Verdict::Info);
        assert_eq!(Verdict::Ok.worse(Verdict::Unknown), Verdict::Ok);
    }

    #[test]
    fn fixture_rows_and_verdicts() {
        let s = status(fixtures());
        let names: Vec<&str> = s.rows.iter().map(|r| r.area.name()).collect();
        assert_eq!(
            names,
            [
                "wan", "wan", "devices", "wifi", "wired", "alarms", "updates"
            ]
        );
        assert_eq!(
            verdicts(&s),
            [
                Verdict::Ok,
                Verdict::Idle,
                Verdict::Ok,
                Verdict::Warn,
                Verdict::Ok,
                Verdict::Ok,
                Verdict::Info
            ]
        );
        assert!(!s.has_error());
    }

    #[test]
    fn wan_down_since_boot_is_idle_but_down_after_boot_is_error() {
        let s = status(fixtures());
        let StatusArea::Wan { wan, .. } = &s.rows[1].area else {
            panic!("{:?}", s.rows[1])
        };
        assert_eq!(wan.id, "internet-2");
        let mut api = fixtures();
        gateway(&mut api)
            .uptime_stats
            .get_mut("WAN2")
            .unwrap()
            .downtime = Some(600);
        let s = status(api);
        assert_eq!(s.rows[1].verdict, Verdict::Error);
        assert!(s.has_error());
    }

    #[test]
    fn wan_without_downtime_reported_is_idle() {
        let mut api = fixtures();
        gateway(&mut api).uptime_stats.remove("WAN2");
        assert_eq!(status(api).rows[1].verdict, Verdict::Idle);
    }

    #[test]
    fn no_gateway_means_idle_wans_not_errors() {
        let mut api = fixtures();
        api.devices.retain(|d| d.kind != "udm");
        let s = status(api);
        assert_eq!(s.rows[0].verdict, Verdict::Idle);
        assert_eq!(s.rows[1].verdict, Verdict::Idle);
        assert!(!s.has_error());
    }

    #[test]
    fn outage_in_the_window_warns() {
        let s = status(fixtures());
        let StatusArea::Wan { outages_24h, .. } = &s.rows[0].area else {
            panic!()
        };
        assert!(outages_24h.is_empty(), "fixture outages are older than 24h");

        let mut api = fixtures();
        let detail = &mut api
            .wan_history
            .as_mut()
            .unwrap()
            .wan_history
            .as_mut()
            .unwrap()
            .wan_history_details[0];
        let mut recent = detail.downtime_history[0].clone();
        recent.timestamp = NOW - 3_600_000;
        detail.downtime_history.push(recent);
        let s = status(api);
        assert_eq!(s.rows[0].verdict, Verdict::Warn);
        let StatusArea::Wan { outages_24h, .. } = &s.rows[0].area else {
            panic!()
        };
        assert_eq!(outages_24h.len(), 1);
        assert_eq!(outages_24h[0].start_ms, NOW - 3_600_000);
    }

    #[test]
    fn availability_after_a_reconnect_covers_the_day() {
        let mut api = fixtures();
        let stats = gateway(&mut api).uptime_stats.get_mut("WAN").unwrap();
        stats.availability = Some(100.0);
        stats.time_period = Some(58);
        api.wan_history
            .as_mut()
            .unwrap()
            .wan_history
            .as_mut()
            .unwrap()
            .wan_history_details[0]
            .uptime = Some(99.82);
        let s = status(api);
        let StatusArea::Wan { wan, .. } = &s.rows[0].area else {
            panic!()
        };
        assert_eq!(wan.availability, Some(99.82));
        assert_eq!(wan.availability_period, Some(86400));
        assert_eq!(s.rows[0].verdict, Verdict::Warn, "99.82% is below 99.9%");
    }

    #[test]
    fn slow_wan_warns() {
        let mut api = fixtures();
        gateway(&mut api)
            .uptime_stats
            .get_mut("WAN")
            .unwrap()
            .latency_average = Some(150.0);
        assert_eq!(status(api).rows[0].verdict, Verdict::Warn);
    }

    #[test]
    fn missing_wan_history_skips_the_outage_check() {
        let api = FixtureApi {
            wan_history: None,
            ..fixtures()
        };
        assert_eq!(status(api).rows[0].verdict, Verdict::Ok);
    }

    #[test]
    fn health_makes_rows_worse_never_better() {
        let api = FixtureApi {
            health: subsystems(&[("www", "warning"), ("lan", "error"), ("wlan", "ok")]),
            ..fixtures()
        };
        let s = status(api);
        assert_eq!(s.rows[0].verdict, Verdict::Warn, "www");
        assert_eq!(
            s.rows[1].verdict,
            Verdict::Idle,
            "down since boot ignores health"
        );
        assert_eq!(area(&s, "wired").verdict, Verdict::Error);
        assert_eq!(
            area(&s, "wifi").verdict,
            Verdict::Warn,
            "poor client, health ok"
        );
        assert!(s.has_error());
    }

    #[test]
    fn without_health_verdicts_are_our_own() {
        let api = FixtureApi {
            health: None,
            ..fixtures()
        };
        assert_eq!(verdicts(&status(api)), verdicts(&status(fixtures())));
    }

    #[test]
    fn unrecognised_health_statuses_change_nothing() {
        let api = FixtureApi {
            health: subsystems(&[("wan", "unknown"), ("lan", "degraded")]),
            ..fixtures()
        };
        assert_eq!(verdicts(&status(api)), verdicts(&status(fixtures())));
    }

    #[test]
    fn offline_and_transitioning_devices() {
        let mut api = fixtures();
        api.devices[0].state = 0; // US 8
        api.devices[1].state = 4; // UAP-02
        let s = status(api);
        let row = area(&s, "devices");
        assert_eq!(row.verdict, Verdict::Error);
        let StatusArea::Devices {
            online,
            total,
            offline,
            transitioning,
        } = &row.area
        else {
            panic!()
        };
        assert_eq!((*online, *total), (5, 7));
        let offline: Vec<&str> = offline.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(offline, ["us-8"]);
        assert_eq!(transitioning.len(), 1);
        assert_eq!(
            (transitioning[0].device.id.as_str(), transitioning[0].state),
            ("uap-02", "upgrading")
        );
    }

    #[test]
    fn transitioning_alone_warns() {
        let mut api = fixtures();
        api.devices[1].state = 2;
        assert_eq!(area(&status(api), "devices").verdict, Verdict::Warn);
    }

    #[test]
    fn poor_wireless_clients_are_named_worst_first() {
        let mut api = fixtures();
        api.active[0].wifi_experience_score = Some(30.0); // MacBook Pro
        let s = status(api);
        let StatusArea::Wifi {
            access_points,
            clients,
            poor_clients,
        } = &area(&s, "wifi").area
        else {
            panic!()
        };
        assert_eq!((*access_points, *clients), (2, Some(2)));
        let poor: Vec<(&str, i64)> = poor_clients
            .iter()
            .map(|c| (c.id.as_str(), c.experience))
            .collect();
        assert_eq!(
            poor,
            [("macbook-pro-0001", 30), ("sonos-one-sl-l-bb03", 45)]
        );
    }

    #[test]
    fn unscored_wireless_clients_are_not_poor() {
        let mut api = fixtures();
        api.active[2].wifi_experience_score = None; // Sonos
        assert_eq!(area(&status(api), "wifi").verdict, Verdict::Ok);
    }

    #[test]
    fn experience_50_is_not_poor() {
        let mut api = fixtures();
        api.active[2].wifi_experience_score = Some(50.0);
        assert_eq!(area(&status(api), "wifi").verdict, Verdict::Ok);
    }

    #[test]
    fn wired_counts_switches_and_wired_clients() {
        let s = status(fixtures());
        let StatusArea::Wired { switches, clients } = &area(&s, "wired").area else {
            panic!()
        };
        assert_eq!((*switches, *clients), (4, Some(1)));
    }

    #[test]
    fn unreadable_clients_leave_wifi_and_wired_unknown() {
        let api = FixtureApi {
            fail_only: vec!["active_clients"],
            ..fixtures()
        };
        let s = status(api);
        for name in ["wifi", "wired"] {
            let row = area(&s, name);
            assert_eq!(row.verdict, Verdict::Unknown, "{name}");
            assert_eq!(row.unavailable.as_deref(), Some("couldn't read clients"));
        }
        assert!(!s.has_error());
    }

    #[test]
    fn high_severity_entries_in_the_window_are_alarms_but_never_errors() {
        let mut api = fixtures();
        api.log_entries[2].timestamp = NOW - 3_600_000; // HIGH, THREAT_DETECTED
        let s = status(api.clone());
        let row = area(&s, "alarms");
        assert_eq!(row.verdict, Verdict::Warn);
        let StatusArea::Alarms { count, latest } = &row.area else {
            panic!()
        };
        assert_eq!(*count, Some(1));
        assert_eq!(latest.as_ref().unwrap().event, "threatDetected");

        api.log_entries[0].timestamp = NOW - 7_200_000; // VERY_HIGH, older
        let s = status(api);
        let row = area(&s, "alarms");
        assert_eq!(row.verdict, Verdict::Warn, "a past event is never an error");
        let StatusArea::Alarms { count, latest } = &row.area else {
            panic!()
        };
        assert_eq!(*count, Some(2));
        assert_eq!(
            latest.as_ref().unwrap().event,
            "threatDetected",
            "newest first"
        );
    }

    #[test]
    fn unreadable_log_leaves_alarms_unknown() {
        let api = FixtureApi {
            fail_only: vec!["system_log"],
            ..fixtures()
        };
        let s = status(api);
        let row = area(&s, "alarms");
        assert_eq!(row.verdict, Verdict::Unknown);
        assert_eq!(
            row.unavailable.as_deref(),
            Some("couldn't read the system log")
        );
    }

    #[test]
    fn updates_list_upgradable_devices() {
        let s = status(fixtures());
        let StatusArea::Updates { updates } = &area(&s, "updates").area else {
            panic!()
        };
        let u: Vec<(&str, &str, &str)> = updates
            .iter()
            .map(|u| (u.device.id.as_str(), u.from.as_str(), u.to.as_str()))
            .collect();
        assert_eq!(u, [("uap-02", "6.8.2.15592", "6.8.3.15600")]);

        let mut api = fixtures();
        api.devices[1].upgradable = false;
        assert_eq!(area(&status(api), "updates").verdict, Verdict::Ok);
    }

    #[test]
    fn devices_and_networks_are_required() {
        for read in ["devices", "network_conf"] {
            let api = FixtureApi {
                fail_only: vec![read],
                ..fixtures()
            };
            assert!(svc(api).status(NOW, &TimeZone::UTC).is_err(), "{read}");
        }
    }

    #[test]
    fn empty_controller() {
        let s = status(FixtureApi::default());
        let names: Vec<&str> = s.rows.iter().map(|r| r.area.name()).collect();
        assert_eq!(names, ["devices", "wifi", "wired", "alarms", "updates"]);
        assert!(!s.has_error());
        let StatusArea::Devices { online, total, .. } = &s.rows[0].area else {
            panic!()
        };
        assert_eq!((*online, *total), (0, 0));
    }

    #[test]
    fn json_rows_carry_area_and_data() {
        let v = serde_json::to_value(status(fixtures())).unwrap();
        assert_eq!(
            v["rows"][2],
            serde_json::json!({
                "verdict": "ok", "area": "devices", "online": 7, "total": 7,
                "offline": [], "transitioning": []
            })
        );
        assert_eq!(v["rows"][0]["area"], "wan");
        assert_eq!(v["rows"][0]["wan"]["id"], "internet-1");
        assert_eq!(v["rows"][0]["outages24h"], serde_json::json!([]));
        assert_eq!(v["rows"][3]["poorClients"][0]["experience"], 45);
        assert_eq!(
            v["rows"][6]["updates"][0],
            serde_json::json!({
                "kind": "device", "id": "uap-02", "name": "UAP-02",
                "from": "6.8.2.15592", "to": "6.8.3.15600"
            })
        );
        assert!(v["rows"][3].get("unavailable").is_none());
    }
}
