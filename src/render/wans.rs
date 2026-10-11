//! wans_list.tmpl, wans_get.tmpl.

use super::Table;
use super::format::format_uptime;
use super::style;
use crate::model::{Wan, WanList};

pub fn list(list: &WanList, wide: bool, color: bool) -> String {
    let mut t = Table::new();
    let mut header = vec!["ID", "NAME", "IP", "UPTIME", "STATUS"];
    if wide {
        header.push("DNS");
    }
    t.row(&header);
    for w in &list.items {
        let mut row = vec![
            w.id.clone(),
            style::name(&w.name, color),
            w.ip_address.clone(),
            format_uptime(w.uptime),
            style::wan_status(w.status, color),
        ];
        if wide {
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
    t.row(&["UPTIME", &format_uptime(w.uptime)]);
    t.row(&["STATUS", &style::wan_status(w.status, color)]);
    t.row(&["DNS", &w.dns_servers.join(", ")]);
    t.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    #[test]
    fn list_formats_uptime() {
        let l = WanList {
            items: vec![Wan {
                id: "internet-1".into(),
                name: "Internet 1".into(),
                ip_address: "203.0.113.1".into(),
                status: WanStatus::Connected,
                uptime: 7200,
                dns_servers: vec![],
            }],
        };
        let out = list(&l, false, false);
        assert!(out.lines().nth(1).unwrap().contains("2h 0m 0s"));
    }

    #[test]
    fn wide_list_adds_dns() {
        let w = |id: &str, dns: Vec<String>| Wan {
            id: id.into(),
            name: id.into(),
            ip_address: "203.0.113.1".into(),
            status: WanStatus::Connected,
            uptime: 60,
            dns_servers: dns,
        };
        let l = WanList {
            items: vec![
                w("a", vec!["1.1.1.1".into(), "8.8.8.8".into()]),
                w("b", vec![]),
            ],
        };
        let out = list(&l, true, false);
        assert!(out.lines().next().unwrap().ends_with("DNS"));
        assert!(out.lines().nth(1).unwrap().ends_with("1.1.1.1, 8.8.8.8"));
        assert!(out.lines().nth(2).unwrap().ends_with("-"));
        assert!(!list(&l, false, false).contains("DNS"));
    }
}
