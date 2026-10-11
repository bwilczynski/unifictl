//! clients_list.tmpl, clients_get_{wired,wireless}.tmpl.

use super::Table;
use super::format::{format_bytes_per_sec, format_last_seen, format_uptime};
use super::style;
use crate::model::{ClientConnection, ClientStatus, NetworkClient, NetworkClientList, Rates};

/// Lean by default; `wide` adds identity and link columns after them.
pub fn list(list: &NetworkClientList, wide: bool, color: bool) -> String {
    let mut t = Table::new();
    let mut header = vec!["ID", "NAME", "IP", "STATUS", "CONNECTION", "EXPERIENCE"];
    if wide {
        header.extend([
            "MAC",
            "UPLINK",
            "SSID",
            "SIGNAL",
            "LINK",
            "DOWN",
            "UP",
            "UPTIME",
            "LAST SEEN",
        ]);
    }
    t.row(&header);
    for c in &list.items {
        let mut row = vec![
            c.id.clone(),
            style::name(&c.name, color),
            c.ip.clone().unwrap_or_default(),
            style::client_status(c.status, color),
            c.connection_type.as_str().to_string(),
            style::experience(c.experience, color),
        ];
        if wide {
            row.extend(wide_cells(c, color));
        }
        t.row(&row.iter().map(String::as_str).collect::<Vec<_>>());
    }
    t.finish()
}

fn dash() -> String {
    "-".to_string()
}

/// MAC, UPLINK, SSID, SIGNAL, LINK, DOWN, UP, UPTIME, LAST SEEN.
fn wide_cells(c: &NetworkClient, color: bool) -> [String; 9] {
    let (uplink, ssid, signal, link) = match &c.connected_to {
        ClientConnection::Wired(w) => (
            match (w.device.name.as_str(), w.port) {
                ("", _) => dash(),
                (name, Some(port)) => format!("{name} #{port}"),
                (name, None) => name.to_string(),
            },
            dash(),
            dash(),
            style::wired_speed(w.link_speed, color),
        ),
        ClientConnection::Wireless(w) => (
            if w.device.name.is_empty() {
                dash()
            } else {
                w.device.name.clone()
            },
            if w.ssid.is_empty() || c.status == ClientStatus::Offline {
                dash()
            } else {
                w.ssid.clone()
            },
            style::signal(w.signal_strength, color),
            w.rate_mbps.map_or_else(dash, |m| format!("{m} Mbps")),
        ),
    };
    let rate = |f: fn(&Rates) -> i64, paint: fn(&str, bool) -> String| {
        c.traffic
            .as_ref()
            .map_or_else(dash, |r| paint(&format_bytes_per_sec(f(r)), color))
    };
    [
        c.mac.clone(),
        uplink,
        ssid,
        signal,
        link,
        rate(|r| r.rx_bytes_per_sec, style::down),
        rate(|r| r.tx_bytes_per_sec, style::up),
        c.uptime.map_or_else(dash, format_uptime),
        c.last_seen.map_or_else(dash, format_last_seen),
    ]
}

pub fn get(c: &NetworkClient, color: bool) -> String {
    let mut t = Table::new();
    t.row(&["FIELD", "VALUE"]);
    t.row(&["ID", &c.id]);
    t.row(&["NAME", &style::name(&c.name, color)]);
    t.row(&["MAC", &c.mac]);
    t.row(&["IP", c.ip.as_deref().unwrap_or("")]);
    t.row(&["CONNECTION", c.connection_type.as_str()]);
    t.row(&["STATUS", &style::client_status(c.status, color)]);
    if c.experience.is_some() {
        t.row(&["EXPERIENCE", &style::experience(c.experience, color)]);
    }
    match &c.connected_to {
        ClientConnection::Wired(w) => {
            t.row(&["SWITCH", &w.device.name]);
            if let Some(port) = w.port {
                t.row(&["PORT", &port.to_string()]);
            }
            if w.link_speed.is_some() {
                t.row(&["LINK SPEED", &style::wired_speed(w.link_speed, color)]);
            }
        }
        ClientConnection::Wireless(w) => {
            t.row(&["AP", &w.device.name]);
            t.row(&["SSID", &w.ssid]);
            if w.signal_strength.is_some() {
                t.row(&["SIGNAL", &style::signal(w.signal_strength, color)]);
            }
            if let Some(rate) = w.rate_mbps {
                t.row(&["WI-FI RATE", &format!("{rate} Mbps")]);
            }
        }
    }
    if let Some(r) = &c.traffic {
        t.row(&[
            "DOWN",
            &style::down(&format_bytes_per_sec(r.rx_bytes_per_sec), color),
        ]);
        t.row(&[
            "UP",
            &style::up(&format_bytes_per_sec(r.tx_bytes_per_sec), color),
        ]);
    }
    if let Some(up) = c.uptime {
        t.row(&["UPTIME", &format_uptime(up)]);
    }
    if let Some(seen) = c.last_seen {
        t.row(&["LAST SEEN", &format_last_seen(seen)]);
    }
    t.finish()
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
    fn default_list_is_lean_with_experience() {
        let out = list(&svc().list_clients(None).unwrap(), false, false);
        assert_eq!(
            out,
            "\
ID                      NAME               IP             STATUS   CONNECTION  EXPERIENCE
macbook-pro-0001        MacBook Pro        192.168.10.67  online   wireless    92%
nas-1-0002              nas-1              192.168.100.5  online   wired       -
sonos-one-sl-l-bb03     Sonos One SL (L)   192.168.10.55  online   wireless    45%
kindle-paperwhite-bb04  Kindle Paperwhite  192.168.10.37  offline  wireless    -
host-02-bb05            host-02            192.168.10.42  offline  wired       -
"
        );
    }

    fn cells(line: &str) -> Vec<&str> {
        line.split("  ")
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .collect()
    }

    #[test]
    fn wide_list_adds_columns_after_default() {
        let out = list(&svc().list_clients(None).unwrap(), true, false);
        assert_eq!(
            cells(out.lines().next().unwrap()),
            [
                "ID",
                "NAME",
                "IP",
                "STATUS",
                "CONNECTION",
                "EXPERIENCE",
                "MAC",
                "UPLINK",
                "SSID",
                "SIGNAL",
                "LINK",
                "DOWN",
                "UP",
                "UPTIME",
                "LAST SEEN"
            ]
        );
        let row = |id: &str| cells(out.lines().find(|l| l.starts_with(id)).unwrap());
        assert_eq!(
            row("macbook-pro-0001")[6..],
            [
                "3c:ee:ee:00:00:01",
                "UAP-01",
                "homelab",
                "-69 dBm",
                "866 Mbps",
                "512.0 KB/s",
                "10.0 KB/s",
                "7h 31m 15s",
                "2026-10-11 10:00"
            ]
        );
        let nas = row("nas-1-0002");
        assert_eq!(nas[7..11], ["US 8 60W #3", "-", "-", "1 GbE"]);
        let off = row("host-02-bb05");
        assert_eq!(
            off[7..],
            [
                "Switch Flex Mini #4",
                "-",
                "-",
                "-",
                "-",
                "-",
                "-",
                "2026-05-15 07:23"
            ]
        );
    }

    #[test]
    fn wide_list_grades_signal_and_link() {
        let out = list(&svc().list_clients(None).unwrap(), true, true);
        let line = |id: &str| out.lines().find(|l| l.starts_with(id)).unwrap().to_string();
        assert!(line("macbook-pro-0001").contains("\x1b[33m-69 dBm\x1b[0m"));
        assert!(line("sonos-one-sl-l-bb03").contains("\x1b[31m-80 dBm\x1b[0m"));
        assert!(line("sonos-one-sl-l-bb03").contains("\x1b[31m45%\x1b[0m"));
        assert!(line("nas-1-0002").contains("\x1b[32m1 GbE\x1b[0m"));
        assert!(line("macbook-pro-0001").contains("\x1b[36m512.0 KB/s\x1b[0m"));
        assert!(line("macbook-pro-0001").contains("\x1b[35m10.0 KB/s\x1b[0m"));
        assert!(
            !line("macbook-pro-0001").contains("\x1b[32m866"),
            "PHY rate is not graded"
        );
    }

    #[test]
    fn wide_unknown_upstream_is_dash() {
        let mut l = svc().list_clients(None).unwrap();
        for c in &mut l.items {
            match &mut c.connected_to {
                ClientConnection::Wired(w) => w.device = Default::default(),
                ClientConnection::Wireless(w) => w.device = Default::default(),
            }
        }
        let out = list(&l, true, false);
        let mb = cells(out.lines().find(|x| x.starts_with("macbook")).unwrap());
        assert_eq!(mb[7], "-");
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

    #[test]
    fn wireless_detail_shows_health_rows() {
        let out = get(&svc().get_client("macbook-pro-0001").unwrap(), true);
        let field = |name: &str| {
            out.lines()
                .find(|l| l.starts_with(name))
                .unwrap()
                .to_string()
        };
        assert!(field("EXPERIENCE").ends_with("\x1b[32m92%\x1b[0m"));
        assert!(field("SIGNAL").ends_with("\x1b[33m-69 dBm\x1b[0m"));
        assert!(field("WI-FI RATE").ends_with("866 Mbps"));
        assert!(field("DOWN").ends_with("\x1b[36m512.0 KB/s\x1b[0m"));
        assert!(field("UP ").ends_with("\x1b[35m10.0 KB/s\x1b[0m"));
        assert!(field("LAST SEEN").ends_with("2026-10-11 10:00"));
    }

    #[test]
    fn wired_detail_grades_link_speed() {
        let out = get(&svc().get_client("nas-1-0002").unwrap(), true);
        assert!(
            out.lines()
                .any(|l| l.starts_with("LINK SPEED") && l.ends_with("\x1b[32m1 GbE\x1b[0m"))
        );
        assert!(!out.contains("EXPERIENCE"));
    }

    #[test]
    fn wide_offline_wireless_ssid_is_dash() {
        let out = list(&svc().list_clients(None).unwrap(), true, false);
        let kindle = cells(
            out.lines()
                .find(|l| l.starts_with("kindle-paperwhite-bb04"))
                .unwrap(),
        );
        assert_eq!(kindle[7..11], ["UAP-01", "-", "-", "-"]);
    }
}
