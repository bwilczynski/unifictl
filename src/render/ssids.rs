//! ssids_list.tmpl, ssids_get.tmpl.

use super::Table;
use super::format::format_bands;
use crate::model::{SsidDetail, SsidList};

pub fn list(list: &SsidList) -> String {
    let mut t = Table::new();
    t.row(&["ID", "NAME", "VLAN ID", "BANDS", "CLIENTS"]);
    for s in &list.items {
        t.row(&[
            &s.id,
            &s.name,
            &s.vlan_id.to_string(),
            &format_bands(&s.bands),
            &s.num_clients.to_string(),
        ]);
    }
    t.finish()
}

pub fn get(s: &SsidDetail) -> String {
    let mut t = Table::new();
    t.row(&["FIELD", "VALUE"]);
    t.row(&["ID", &s.id]);
    t.row(&["NAME", &s.name]);
    t.row(&["VLAN ID", &s.vlan_id.to_string()]);
    t.row(&["BANDS", &format_bands(&s.bands)]);
    t.row(&["CLIENTS", &s.num_clients.to_string()]);
    t.row(&["SECURITY", s.security_protocol.as_str()]);
    t.flush();
    t.blank();
    t.line("--- CLIENTS ---");
    t.line("NAME");
    for c in &s.clients {
        t.line(&c.name);
    }
    t.flush();
    t.blank();
    t.line("--- BROADCASTING APs ---");
    t.line("NAME");
    for ap in &s.broadcasting_aps {
        t.line(&ap.name);
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
        let out = get(&d);
        assert!(out.contains("2.4 GHz, 5 GHz"));
        assert!(out.contains("wpa2Wpa3"));
        assert!(out.contains("\n\n--- CLIENTS ---\nNAME\nPhone\n"));
        assert!(out.ends_with("\n\n--- BROADCASTING APs ---\nNAME\n"));
    }
}
