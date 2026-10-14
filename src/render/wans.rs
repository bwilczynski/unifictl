//! wans_list.tmpl, wans_get.tmpl.

use jiff::Timestamp;
use jiff::tz::TimeZone;

use super::Table;
use super::format::{format_period, format_uptime};
use super::style;
use crate::model::{Wan, WanDetail, WanList, WanOutages};

fn uptime(w: &Wan) -> String {
    w.uptime.map_or_else(|| "-".to_string(), format_uptime)
}

pub fn list(list: &WanList, wide: bool, color: bool) -> String {
    let mut t = Table::new();
    let mut header = vec!["ID", "NAME", "IP", "UPTIME", "STATUS", "AVAILABILITY"];
    if wide {
        header.extend(["LATENCY", "DNS"]);
    }
    t.row(&header);
    for w in &list.items {
        let mut row = vec![
            w.id.clone(),
            style::name(&w.name, color),
            w.ip_address.clone(),
            uptime(w),
            style::wan_status(w.status, color),
            style::availability(w.availability, color),
        ];
        if wide {
            row.push(style::latency(w.latency, color));
            row.push(if w.dns_servers.is_empty() {
                "-".to_string()
            } else {
                w.dns_servers.join(", ")
            });
        }
        t.row(&row.iter().map(String::as_str).collect::<Vec<_>>());
    }
    t.finish()
}

pub fn get(d: &WanDetail, tz: &TimeZone, color: bool) -> String {
    let w = &d.wan;
    let mut t = Table::new();
    t.row(&["FIELD", "VALUE"]);
    t.row(&["ID", &w.id]);
    t.row(&["NAME", &style::name(&w.name, color)]);
    t.row(&["IP", &w.ip_address]);
    match w.downtime {
        Some(d) if w.uptime.is_none() => t.row(&["DOWNTIME", &format_uptime(d)]),
        _ => t.row(&["UPTIME", &uptime(w)]),
    }
    t.row(&["STATUS", &style::wan_status(w.status, color)]);
    let mut availability = style::availability(w.availability, color);
    if let (Some(_), Some(p)) = (w.availability, w.availability_period) {
        availability.push_str(&format!(" ({})", format_period(p)));
    }
    t.row(&["AVAILABILITY", &availability]);
    t.row(&["LATENCY", &style::latency(w.latency, color)]);
    t.row(&["DNS", &w.dns_servers.join(", ")]);
    let Some(o) = &d.outages else {
        return t.finish();
    };
    t.row(&["OUTAGES", &outages_summary(o, tz, color)]);
    let mut out = t.finish();
    if !o.items.is_empty() {
        let mut items = Table::new();
        items.row(&["START", "END", "DURATION"]);
        for i in &o.items {
            items.row(&[
                &local(i.start_ms, tz, "%b %d %H:%M:%S"),
                &local(i.end_ms, tz, "%b %d %H:%M:%S"),
                &format_uptime(i.duration_seconds),
            ]);
        }
        out.push('\n');
        out.push_str(&items.finish());
    }
    out
}

/// `3 since Oct 05 00:00 (99.95% available)`.
fn outages_summary(o: &WanOutages, tz: &TimeZone, color: bool) -> String {
    let mut s = format!(
        "{} since {}",
        o.items.len(),
        local(o.since_ms, tz, "%b %d %H:%M")
    );
    if o.availability.is_some() {
        s.push_str(&format!(
            " ({} available)",
            style::availability(o.availability, color)
        ));
    }
    s
}

fn local(ms: i64, tz: &TimeZone, fmt: &str) -> String {
    Timestamp::from_millisecond(ms)
        .unwrap_or(Timestamp::UNIX_EPOCH)
        .to_zoned(tz.clone())
        .strftime(fmt)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    use jiff::tz::TimeZone;

    fn up(id: &str) -> Wan {
        Wan {
            id: id.into(),
            name: id.into(),
            ip_address: "203.0.113.1".into(),
            status: WanStatus::Connected,
            uptime: Some(7200),
            downtime: None,
            availability: Some(99.9306),
            availability_period: Some(86400),
            latency: Some(11),
            dns_servers: vec![],
        }
    }

    fn down(id: &str) -> Wan {
        Wan {
            ip_address: String::new(),
            status: WanStatus::Disconnected,
            uptime: None,
            downtime: Some(90061),
            availability: None,
            availability_period: None,
            latency: None,
            ..up(id)
        }
    }

    #[test]
    fn list_shows_wan_uptime_and_availability() {
        let l = WanList {
            items: vec![up("a"), down("b")],
        };
        let out = list(&l, false, false);
        let lines: Vec<&str> = out.lines().collect();
        assert!(lines[0].ends_with("STATUS        AVAILABILITY"), "{out}");
        assert!(lines[1].contains("2h 0m 0s"));
        assert!(lines[1].ends_with("99.93%"));
        assert!(lines[2].contains(" -  "), "down WAN has no uptime: {out}");
        assert!(lines[2].ends_with("-"));
        assert!(!out.contains("LATENCY"));
    }

    #[test]
    fn wide_list_adds_latency_and_dns() {
        let mut a = up("a");
        a.dns_servers = vec!["1.1.1.1".into(), "8.8.8.8".into()];
        let l = WanList {
            items: vec![a, down("b")],
        };
        let out = list(&l, true, false);
        assert!(out.lines().next().unwrap().ends_with("LATENCY  DNS"));
        assert!(
            out.lines()
                .nth(1)
                .unwrap()
                .ends_with("11 ms    1.1.1.1, 8.8.8.8")
        );
        assert!(out.lines().nth(2).unwrap().ends_with("-        -"));
    }

    #[test]
    fn availability_and_latency_are_graded() {
        let mut w = up("a");
        w.availability = Some(98.5);
        w.latency = Some(120);
        let out = list(&WanList { items: vec![w] }, true, true);
        assert!(out.contains("\x1b[31m98.50%\x1b[0m"), "{out:?}");
        assert!(out.contains("\x1b[31m120 ms\x1b[0m"), "{out:?}");
    }

    #[test]
    fn get_shows_period_and_latency() {
        let out = get(
            &WanDetail {
                wan: up("a"),
                outages: None,
            },
            &TimeZone::UTC,
            false,
        );
        assert!(out.contains("UPTIME        2h 0m 0s\n"), "{out}");
        assert!(out.contains("AVAILABILITY  99.93% (24h)\n"), "{out}");
        assert!(out.contains("LATENCY       11 ms\n"), "{out}");
    }

    #[test]
    fn get_shows_downtime_for_a_down_wan() {
        let out = get(
            &WanDetail {
                wan: down("b"),
                outages: None,
            },
            &TimeZone::UTC,
            false,
        );
        assert!(out.contains("DOWNTIME      1d 1h 1m 1s\n"), "{out}");
        assert!(!out.contains("UPTIME"));
        assert!(out.contains("AVAILABILITY  -\n"), "{out}");
    }

    #[test]
    fn get_without_stats_prints_dashes() {
        let w = Wan {
            downtime: None,
            ..down("b")
        };
        let out = get(
            &WanDetail {
                wan: w,
                outages: None,
            },
            &TimeZone::UTC,
            false,
        );
        assert!(out.contains("UPTIME        -\n"), "{out}");
        assert!(out.contains("LATENCY       -\n"), "{out}");
    }

    fn outage(start_ms: i64, secs: i64) -> WanOutage {
        WanOutage {
            start: String::new(),
            end: String::new(),
            duration_seconds: secs,
            start_ms,
            end_ms: start_ms + secs * 1000,
        }
    }

    fn with_outages(items: Vec<WanOutage>) -> WanDetail {
        WanDetail {
            wan: up("a"),
            outages: Some(WanOutages {
                since: String::new(),
                availability: Some(99.95),
                items,
                since_ms: 1791158400000, // 2026-10-05T00:00:00Z
            }),
        }
    }

    #[test]
    fn get_lists_outages_after_the_fields() {
        let d = with_outages(vec![
            outage(1791256365250, 95), // Oct 06 03:12:45
            outage(1791582008750, 41), // Oct 09 21:40:08
        ]);
        let out = get(&d, &TimeZone::UTC, false);
        let lines: Vec<&str> = out.lines().collect();
        let n = lines.iter().position(|l| l.starts_with("OUTAGES")).unwrap();
        assert_eq!(
            lines[n],
            "OUTAGES       2 since Oct 05 00:00 (99.95% available)"
        );
        assert_eq!(lines[n + 1], "");
        assert_eq!(lines[n + 2], "START            END              DURATION");
        assert_eq!(lines[n + 3], "Oct 06 03:12:45  Oct 06 03:14:20  1m 35s");
        assert_eq!(lines[n + 4], "Oct 09 21:40:08  Oct 09 21:40:49  41s");
        assert_eq!(lines.len(), n + 5);
    }

    #[test]
    fn no_outages_is_a_row_without_a_table() {
        let out = get(&with_outages(vec![]), &TimeZone::UTC, false);
        assert!(
            out.ends_with("OUTAGES       0 since Oct 05 00:00 (99.95% available)\n"),
            "{out}"
        );
        assert!(!out.contains("START"));
    }

    #[test]
    fn no_history_leaves_the_section_out() {
        let d = WanDetail {
            wan: up("a"),
            outages: None,
        };
        let out = get(&d, &TimeZone::UTC, false);
        assert!(!out.contains("OUTAGES"));
        assert!(out.lines().last().unwrap().starts_with("DNS"), "{out}");
    }

    #[test]
    fn unknown_availability_is_left_out() {
        let mut d = with_outages(vec![]);
        d.outages.as_mut().unwrap().availability = None;
        let out = get(&d, &TimeZone::UTC, false);
        assert!(
            out.contains("OUTAGES       0 since Oct 05 00:00\n"),
            "{out}"
        );
    }

    #[test]
    fn zero_length_outage() {
        let out = get(
            &with_outages(vec![outage(1791256365250, 0)]),
            &TimeZone::UTC,
            false,
        );
        assert!(
            out.contains("Oct 06 03:12:45  Oct 06 03:12:45  0s"),
            "{out}"
        );
    }

    #[test]
    fn outage_availability_is_graded() {
        let mut d = with_outages(vec![]);
        d.outages.as_mut().unwrap().availability = Some(98.0);
        let out = get(&d, &TimeZone::UTC, true);
        assert!(out.contains("(\x1b[31m98.00%\x1b[0m available)"), "{out}");
    }

    #[test]
    fn times_are_local() {
        let tz = TimeZone::fixed(jiff::tz::offset(2));
        let out = get(&with_outages(vec![outage(1791256365250, 95)]), &tz, false);
        assert!(out.contains("since Oct 05 02:00"));
        assert!(out.contains("Oct 06 05:12:45  Oct 06 05:14:20"));
    }
}
