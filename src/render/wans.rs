//! wans_list.tmpl, wans_get.tmpl.

use super::Table;
use super::format::format_uptime;
use super::style;
use crate::model::{WanDetail, WanList};

pub fn list(list: &WanList, color: bool) -> String {
    let mut t = Table::new();
    t.row(&["ID", "NAME", "IP", "UPTIME", "STATUS"]);
    for w in &list.items {
        t.row(&[
            &w.id,
            &w.name,
            &w.ip_address,
            &format_uptime(w.uptime),
            &style::wan_status(w.status, color),
        ]);
    }
    t.finish()
}

pub fn get(w: &WanDetail, color: bool) -> String {
    let mut t = Table::new();
    t.row(&["FIELD", "VALUE"]);
    t.row(&["ID", &w.id]);
    t.row(&["NAME", &w.name]);
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
            }],
        };
        let out = list(&l, false);
        assert!(out.lines().nth(1).unwrap().contains("2h 0m 0s"));
    }
}
