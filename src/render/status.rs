//! `unifictl status`: per line an area, its verdict and a summary, with the
//! culprits on the lines below.

use jiff::Timestamp;
use jiff::tz::TimeZone;

use super::Table;
use super::format::{format_period, format_uptime};
use super::logs;
use super::style;
use crate::model::{SiteStatus, StatusArea, StatusRow, Verdict, WanStatus};

/// Most names in one detail line; the rest are counted.
const MAX_NAMES: usize = 5;

pub fn table(status: &SiteStatus, tz: &TimeZone, color: bool) -> String {
    let mut t = Table::new();
    for row in &status.rows {
        let (summary, details) = text(row, tz, color);
        let verdict = style::verdict(row.verdict, color);
        t.row(&[label(&row.area), &verdict, &summary]);
        for d in &details {
            t.row(&["", "", d]);
        }
    }
    t.finish()
}

fn label(area: &StatusArea) -> &'static str {
    match area {
        StatusArea::Wan { .. } => "WAN",
        StatusArea::Devices { .. } => "DEVICES",
        StatusArea::Wifi { .. } => "WI-FI",
        StatusArea::Wired { .. } => "WIRED",
        StatusArea::Alarms { .. } => "ALARMS",
        StatusArea::Updates { .. } => "UPDATES",
    }
}

/// The summary and the detail lines of `row`.
fn text(row: &StatusRow, tz: &TimeZone, color: bool) -> (String, Vec<String>) {
    if let Some(why) = &row.unavailable {
        return (why.clone(), Vec::new());
    }
    let dash = || "-".to_string();
    match &row.area {
        StatusArea::Wan { wan, outages_24h } => {
            let name = style::name(&wan.name, color);
            let summary = match wan.status {
                WanStatus::Disconnected if row.verdict == Verdict::Idle => {
                    format!("{name}  not connected since boot")
                }
                WanStatus::Disconnected => {
                    format!(
                        "{name}  down {}",
                        wan.downtime.map_or_else(dash, format_uptime)
                    )
                }
                WanStatus::Connected => {
                    let mut parts = vec![
                        name,
                        format!("up {}", wan.uptime.map_or_else(dash, format_uptime)),
                    ];
                    if let (Some(_), Some(period)) = (wan.availability, wan.availability_period) {
                        parts.push(format!(
                            "{} over {}",
                            style::availability(wan.availability, color),
                            format_period(period)
                        ));
                    }
                    if wan.latency.is_some() {
                        parts.push(style::latency(wan.latency, color));
                    }
                    parts.join("  ")
                }
            };
            let details = outages_24h
                .last()
                .map(|o| {
                    format!(
                        "{} in 24h (latest {} at {})",
                        plural(outages_24h.len(), "outage", "outages"),
                        format_period(o.duration_seconds),
                        clock(o.start_ms, tz)
                    )
                })
                .into_iter()
                .collect();
            (summary, details)
        }
        StatusArea::Devices {
            online,
            total,
            offline,
            transitioning,
        } => {
            let summary = if online == total {
                format!("{online} online")
            } else {
                format!("{online} of {total} online")
            };
            let mut details = Vec::new();
            if !offline.is_empty() {
                let ids = offline.iter().map(|d| style::name(&d.id, color)).collect();
                details.push(format!("offline: {}", names(ids)));
            }
            let mut states: Vec<&str> = Vec::new();
            for t in transitioning {
                if !states.contains(&t.state) {
                    states.push(t.state);
                }
            }
            for state in states {
                let ids = transitioning
                    .iter()
                    .filter(|t| t.state == state)
                    .map(|t| style::name(&t.device.id, color))
                    .collect();
                details.push(format!("{state}: {}", names(ids)));
            }
            (summary, details)
        }
        StatusArea::Wifi {
            access_points,
            clients,
            poor_clients,
        } => {
            let summary = format!(
                "{}, {}",
                plural(*access_points, "AP", "APs"),
                plural(clients.unwrap_or(0), "client", "clients")
            );
            let details = if poor_clients.is_empty() {
                Vec::new()
            } else {
                let ids = poor_clients
                    .iter()
                    .map(|c| format!("{} ({})", style::name(&c.id, color), c.experience))
                    .collect();
                vec![format!("poor experience: {}", names(ids))]
            };
            (summary, details)
        }
        StatusArea::Wired { switches, clients } => (
            format!(
                "{}, {}",
                plural(*switches, "switch", "switches"),
                plural(clients.unwrap_or(0), "client", "clients")
            ),
            Vec::new(),
        ),
        StatusArea::Alarms { count, latest } => match (count.unwrap_or(0), latest) {
            (n, Some(e)) if n > 0 => (
                format!(
                    "{} in 24h",
                    plural(n, "high-severity event", "high-severity events")
                ),
                vec![format!("latest: {}", logs::line(e, tz, color).trim_end())],
            ),
            _ => ("no high-severity events in 24h".to_string(), Vec::new()),
        },
        StatusArea::Updates { updates } => {
            let mut lines = updates.iter().map(|u| {
                format!(
                    "{} {} → {}",
                    style::name(&u.device.id, color),
                    style::printable(&u.from),
                    style::printable(&u.to)
                )
            });
            match lines.next() {
                Some(first) => (first, lines.collect()),
                None => ("all up to date".to_string(), Vec::new()),
            }
        }
    }
}

/// `1 client`, `2 clients`.
fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// `a, b, c`: at most `MAX_NAMES`, then `+N more`.
fn names(items: Vec<String>) -> String {
    let more = items.len().saturating_sub(MAX_NAMES);
    let mut shown: Vec<String> = items.into_iter().take(MAX_NAMES).collect();
    if more > 0 {
        shown.push(format!("+{more} more"));
    }
    shown.join(", ")
}

/// `HH:MM` in `tz`.
fn clock(ms: i64, tz: &TimeZone) -> String {
    Timestamp::from_millisecond(ms)
        .unwrap_or(Timestamp::UNIX_EPOCH)
        .to_zoned(tz.clone())
        .strftime("%H:%M")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::testutil::{fixtures, svc};
    use crate::unifi::fixtures::FixtureApi;

    /// 2026-10-11T12:00:00Z, as in the service tests.
    const NOW: i64 = 1_791_720_000_000;

    fn render(api: FixtureApi, color: bool) -> String {
        let s = svc(api).status(NOW, &TimeZone::UTC).unwrap();
        table(&s, &TimeZone::UTC, color)
    }

    #[test]
    fn fixture_table() {
        // Detail lines start under the summary column: 9 + 6 spaces.
        let want = [
            "WAN      ok    Internet 1  up 1h 10m 53s  100.00% over 1h 11m 10s  13 ms",
            "WAN      idle  Internet 2  not connected since boot",
            "DEVICES  ok    7 online",
            "WI-FI    warn  2 APs, 2 clients",
            "               poor experience: sonos-one-sl-l-bb03 (45)",
            "WIRED    ok    4 switches, 1 client",
            "ALARMS   ok    no high-severity events in 24h",
            "UPDATES  info  uap-02 6.8.2.15592 → 6.8.3.15600",
        ];
        assert_eq!(render(fixtures(), false), want.join("\n") + "\n");
    }

    #[test]
    fn problems_are_named_below_their_row() {
        let mut api = fixtures();
        for d in &mut api.devices {
            d.state = 0;
        }
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
        recent.downtime_in_seconds = 250;
        detail.downtime_history.push(recent);
        api.log_entries[2].timestamp = NOW - 3_600_000;
        let out = render(api, false);
        assert!(
            out.contains("1 outage in 24h (latest 4m 10s at 11:00)\n"),
            "{out}"
        );
        assert!(out.contains("DEVICES  error  0 of 7 online\n"), "{out}");
        assert!(
            out.contains(
                "offline: us-8, uap-02, switch-flex-mini, uap-01, usw-flex-2-5g-8, +2 more\n"
            ),
            "{out}"
        );
        assert!(
            out.contains("ALARMS   warn   1 high-severity event in 24h\n"),
            "{out}"
        );
        assert!(out.contains("latest: Oct 11 11:00:00 security: "), "{out}");
    }

    #[test]
    fn unknown_rows_say_what_could_not_be_read() {
        let api = FixtureApi {
            fail_only: vec!["active_clients"],
            ..fixtures()
        };
        let out = render(api, false);
        assert!(
            out.contains("WI-FI    unknown  couldn't read clients\n"),
            "{out}"
        );
        assert!(
            out.contains("WIRED    unknown  couldn't read clients\n"),
            "{out}"
        );
    }

    #[test]
    fn healthy_rows_without_updates() {
        let mut api = fixtures();
        api.devices[1].upgradable = false;
        assert!(render(api, false).contains("UPDATES  ok    all up to date\n"));
    }

    #[test]
    fn verdicts_and_names_are_coloured() {
        let out = render(fixtures(), true);
        assert!(
            out.starts_with("WAN      \x1b[32mok\x1b[0m    \x1b[34mInternet 1\x1b[0m  up "),
            "{out:?}"
        );
        assert!(out.contains("\x1b[2midle\x1b[0m"));
        assert!(out.contains("\x1b[33mwarn\x1b[0m"));
        assert!(out.contains("\x1b[34minfo\x1b[0m"));
        assert!(out.contains("poor experience: \x1b[34msonos-one-sl-l-bb03\x1b[0m (45)"));
        assert!(out.contains("\x1b[34muap-02\x1b[0m 6.8.2.15592 → 6.8.3.15600"));
    }
}
