//! wans_list.tmpl, wans_get.tmpl.

use super::Table;
use super::format::{format_period, format_uptime};
use super::style;
use crate::model::{Wan, WanList};

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

pub fn get(w: &Wan, color: bool) -> String {
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
    t.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

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
        let out = get(&up("a"), false);
        assert!(out.contains("UPTIME        2h 0m 0s\n"), "{out}");
        assert!(out.contains("AVAILABILITY  99.93% (24h)\n"), "{out}");
        assert!(out.contains("LATENCY       11 ms\n"), "{out}");
    }

    #[test]
    fn get_shows_downtime_for_a_down_wan() {
        let out = get(&down("b"), false);
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
        let out = get(&w, false);
        assert!(out.contains("UPTIME        -\n"), "{out}");
        assert!(out.contains("LATENCY       -\n"), "{out}");
    }
}
