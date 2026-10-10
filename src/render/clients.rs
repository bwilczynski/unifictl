//! clients_list.tmpl, clients_get_{wired,wireless}.tmpl.

use super::Table;
use super::format::{format_link_speed, format_uptime};
use super::style;
use crate::model::{NetworkClientDetail, NetworkClientList};

pub fn list(list: &NetworkClientList, color: bool) -> String {
    let mut t = Table::new();
    t.row(&["ID", "NAME", "MAC", "IP", "STATUS", "CONNECTION"]);
    for c in &list.items {
        t.row(&[
            &c.id,
            &c.name,
            &c.mac,
            c.ip.as_deref().unwrap_or(""),
            &style::client_status(c.status, color),
            c.connection_type.as_str(),
        ]);
    }
    t.finish()
}

pub fn get(detail: &NetworkClientDetail, color: bool) -> String {
    let mut t = Table::new();
    t.row(&["FIELD", "VALUE"]);
    match detail {
        NetworkClientDetail::Wired(c) => {
            common(
                &mut t,
                &c.id,
                &c.name,
                &c.mac,
                c.ip.as_deref(),
                c.connection_type.as_str(),
                &style::client_status(c.status, color),
            );
            t.row(&["SWITCH", &c.connected_to.device.name]);
            if let Some(port) = c.connected_to.port {
                t.row(&["PORT", &port.to_string()]);
            }
            if let Some(ls) = c.connected_to.link_speed {
                t.row(&["LINK SPEED", format_link_speed(ls)]);
            }
            if let Some(up) = c.uptime {
                t.row(&["UPTIME", &format_uptime(up)]);
            }
        }
        NetworkClientDetail::Wireless(c) => {
            common(
                &mut t,
                &c.id,
                &c.name,
                &c.mac,
                c.ip.as_deref(),
                c.connection_type.as_str(),
                &style::client_status(c.status, color),
            );
            t.row(&["AP", &c.connected_to.device.name]);
            t.row(&["SSID", &c.connected_to.ssid]);
            if let Some(s) = c.connected_to.signal_strength {
                t.row(&["SIGNAL", &format!("{s} dBm")]);
            }
            if let Some(up) = c.uptime {
                t.row(&["UPTIME", &format_uptime(up)]);
            }
        }
    }
    t.finish()
}

fn common(
    t: &mut Table,
    id: &str,
    name: &str,
    mac: &str,
    ip: Option<&str>,
    conn: &str,
    status: &str,
) {
    t.row(&["ID", id]);
    t.row(&["NAME", name]);
    t.row(&["MAC", mac]);
    t.row(&["IP", ip.unwrap_or("")]);
    t.row(&["CONNECTION", conn]);
    t.row(&["STATUS", status]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::Service;
    use crate::unifi::fixtures::FixtureApi;

    fn svc() -> Service {
        Service::new(
            Box::new(FixtureApi::load(&FixtureApi::default_dir()).unwrap()),
            30,
        )
    }

    #[test]
    fn list_rows() {
        let out = list(&svc().list_clients(None).unwrap(), false);
        assert!(out.starts_with("ID"));
        assert_eq!(out.lines().count(), 6);
    }

    #[test]
    fn wired_detail_fields() {
        let out = get(&svc().get_client("nas-1-0002").unwrap(), false);
        let field = |name: &str| {
            out.lines()
                .find(|l| l.starts_with(name))
                .map(|l| l[name.len()..].trim().to_string())
        };
        assert_eq!(field("SWITCH").as_deref(), Some("US 8 60W"));
        assert_eq!(field("PORT").as_deref(), Some("3"));
        assert_eq!(field("LINK SPEED").as_deref(), Some("1 GbE"));
        assert!(field("UPTIME").is_some());
    }

    #[test]
    fn offline_wireless_detail_omits_signal_and_uptime() {
        let out = get(&svc().get_client("kindle-paperwhite-bb04").unwrap(), false);
        assert!(
            out.lines()
                .any(|l| l.starts_with("AP") && l.ends_with("UAP-01"))
        );
        assert!(
            out.lines()
                .any(|l| l.starts_with("SSID") && l.ends_with("homelab"))
        );
        assert!(!out.contains("SIGNAL") && !out.contains("UPTIME"));
    }
}
