//! ssids_list.tmpl, ssids_get.tmpl.

use super::Table;
use super::format::format_bands;
use super::style;
use crate::model::{SsidDetail, SsidList};

pub fn list(list: &SsidList, wide: bool, color: bool) -> String {
    let mut t = Table::new();
    let mut header = vec!["ID", "NAME", "VLAN ID", "BANDS", "CLIENTS"];
    if wide {
        header.push("SECURITY");
    }
    t.row(&header);
    for s in &list.items {
        let mut row = vec![
            style::printable(&s.id),
            style::name(&s.name, color),
            s.vlan_id.to_string(),
            format_bands(&s.bands),
            s.num_clients.to_string(),
        ];
        if wide {
            row.push(s.security_protocol.as_str().to_string());
        }
        t.row(&row.iter().map(String::as_str).collect::<Vec<_>>());
    }
    t.finish()
}

pub fn get(s: &SsidDetail, color: bool) -> String {
    let mut t = Table::new();
    t.row(&["FIELD", "VALUE"]);
    t.row(&["ID", &style::printable(&s.base.id)]);
    t.row(&["NAME", &style::name(&s.base.name, color)]);
    t.row(&["VLAN ID", &s.base.vlan_id.to_string()]);
    t.row(&["BANDS", &format_bands(&s.base.bands)]);
    t.row(&["CLIENTS", &s.base.num_clients.to_string()]);
    t.row(&["SECURITY", s.base.security_protocol.as_str()]);
    t.flush();
    t.blank();
    t.line("--- CLIENTS ---");
    t.line("NAME");
    for c in &s.clients {
        t.line(&style::name(&c.name, color));
    }
    t.flush();
    t.blank();
    t.line("--- BROADCASTING APs ---");
    t.line("NAME");
    for ap in &s.broadcasting_aps {
        t.line(&style::name(&ap.name, color));
    }
    t.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    #[test]
    fn detail_sections() {
        let d = SsidDetail {
            base: Ssid {
                id: "home".into(),
                name: "Home".into(),
                bands: vec![WifiBand::Band2g, WifiBand::Band5g],
                vlan_id: 10,
                num_clients: 1,
                security_protocol: SecurityProtocol::Wpa2Wpa3,
            },
            clients: vec![ClientRef::new("phone-aa".into(), "Phone".into())],
            broadcasting_aps: vec![],
        };
        let out = get(&d, false);
        assert!(out.contains("2.4 GHz, 5 GHz"));
        assert!(out.contains("wpa2Wpa3"));
        assert!(out.contains("\n\n--- CLIENTS ---\nNAME\nPhone\n"));
        assert!(out.ends_with("\n\n--- BROADCASTING APs ---\nNAME\n"));
    }

    #[test]
    fn wide_list_adds_security() {
        let l = SsidList {
            items: vec![Ssid {
                id: "home-wifi".into(),
                name: "home-wifi".into(),
                bands: vec![WifiBand::Band5g],
                vlan_id: 10,
                num_clients: 2,
                security_protocol: SecurityProtocol::Wpa2Wpa3,
            }],
        };
        let out = list(&l, true, false);
        assert!(out.lines().next().unwrap().ends_with("SECURITY"));
        assert!(out.lines().nth(1).unwrap().ends_with("wpa2Wpa3"));
        assert!(!list(&l, false, false).contains("SECURITY"));
    }
}
