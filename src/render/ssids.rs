//! ssids_list.tmpl, ssids_get.tmpl.

use super::Table;
use super::format::format_bands;
use super::style;
use crate::model::{SsidDetail, SsidList};

pub fn list(list: &SsidList, color: bool) -> String {
    let mut t = Table::new();
    t.row(&["ID", "NAME", "VLAN ID", "BANDS", "CLIENTS"]);
    for s in &list.items {
        t.row(&[
            &s.id,
            &style::name(&s.name, color),
            &s.vlan_id.to_string(),
            &format_bands(&s.bands),
            &s.num_clients.to_string(),
        ]);
    }
    t.finish()
}

pub fn get(s: &SsidDetail, color: bool) -> String {
    let mut t = Table::new();
    t.row(&["FIELD", "VALUE"]);
    t.row(&["ID", &s.id]);
    t.row(&["NAME", &style::name(&s.name, color)]);
    t.row(&["VLAN ID", &s.vlan_id.to_string()]);
    t.row(&["BANDS", &format_bands(&s.bands)]);
    t.row(&["CLIENTS", &s.num_clients.to_string()]);
    t.row(&["SECURITY", s.security_protocol.as_str()]);
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
            id: "home".into(),
            name: "Home".into(),
            bands: vec![WifiBand::Band2g, WifiBand::Band5g],
            vlan_id: 10,
            num_clients: 1,
            clients: vec![ClientRef::new("phone-aa".into(), "Phone".into())],
            broadcasting_aps: vec![],
            security_protocol: SecurityProtocol::Wpa2Wpa3,
        };
        let out = get(&d, false);
        assert!(out.contains("2.4 GHz, 5 GHz"));
        assert!(out.contains("wpa2Wpa3"));
        assert!(out.contains("\n\n--- CLIENTS ---\nNAME\nPhone\n"));
        assert!(out.ends_with("\n\n--- BROADCASTING APs ---\nNAME\n"));
    }
}
