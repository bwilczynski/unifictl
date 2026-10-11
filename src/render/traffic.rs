//! `clients traffic` tables: per client, one client by application, and
//! per application across the site.

use super::Table;
use super::format::{format_bytes, format_uptime};
use super::style;
use crate::model::{AppTrafficList, ClientAppTrafficList, ClientTrafficList};

pub fn clients(list: &ClientTrafficList, color: bool) -> String {
    let mut t = Table::new();
    t.row(&["ID", "NAME", "DOWN", "UP", "TOTAL", "TOP APP"]);
    for c in &list.items {
        t.row(&[
            &c.id,
            &style::name(&c.name, color),
            &format_bytes(c.bytes_received),
            &format_bytes(c.bytes_transmitted),
            &format_bytes(c.total_bytes),
            &c.top_app
                .as_ref()
                .map_or_else(String::new, |a| style::app_name(a, color)),
        ]);
    }
    t.finish()
}

pub fn client(list: &ClientAppTrafficList, color: bool) -> String {
    let mut t = Table::new();
    t.row(&["APP", "CATEGORY", "DOWN", "UP", "TOTAL", "ACTIVE"]);
    for a in &list.items {
        t.row(&[
            &style::app_name(&a.app, color),
            &a.app.category.name,
            &format_bytes(a.bytes_received),
            &format_bytes(a.bytes_transmitted),
            &format_bytes(a.total_bytes),
            &format_uptime(a.activity_seconds),
        ]);
    }
    t.finish()
}

pub fn apps(list: &AppTrafficList, color: bool) -> String {
    let mut t = Table::new();
    t.row(&["APP", "CATEGORY", "CLIENTS", "DOWN", "UP", "TOTAL"]);
    for a in &list.items {
        t.row(&[
            &style::app_name(&a.app, color),
            &a.app.category.name,
            &a.client_count.to_string(),
            &format_bytes(a.bytes_received),
            &format_bytes(a.bytes_transmitted),
            &format_bytes(a.total_bytes),
        ]);
    }
    t.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::{Service, TrafficWindow};
    use crate::unifi::fixtures::FixtureApi;

    const W: TrafficWindow = TrafficWindow {
        start_ms: 0,
        end_ms: 1,
    };

    fn svc() -> Service {
        Service::new(
            Box::new(FixtureApi::load(&FixtureApi::default_dir()).unwrap()),
            30,
        )
    }

    #[test]
    fn clients_table() {
        let out = clients(&svc().clients_traffic(W).unwrap().data, false);
        assert_eq!(
            out,
            "\
ID                   NAME              DOWN      UP       TOTAL     TOP APP
nas-1-0002           nas-1             501.0 MB  3.0 GB   3.5 GB    Example Web
macbook-pro-0001     MacBook Pro       2.1 GB    71.0 MB  2.2 GB    Example Video
sonos-one-sl-l-bb03  Sonos One SL (L)  300.0 MB  5.0 MB   305.0 MB  Unknown (4:999)
"
        );
    }

    #[test]
    fn client_table() {
        let out = client(
            &svc().client_traffic("macbook-pro-0001", W).unwrap().data,
            false,
        );
        assert_eq!(
            out,
            "\
APP                 CATEGORY                  DOWN      UP       TOTAL     ACTIVE
Example Video       Media streaming services  2.0 GB    50.0 MB  2.0 GB    1h 30m 0s
Facebook Messenger  Instant messengers        100.0 MB  20.0 MB  120.0 MB  30m 0s
Unidentified        Unknown                   10.0 MB   1.0 MB   11.0 MB   1m 0s
"
        );
    }

    #[test]
    fn apps_table() {
        let out = apps(&svc().apps_traffic(W).unwrap().data, false);
        assert_eq!(
            out,
            "\
APP                 CATEGORY                  CLIENTS  DOWN      UP       TOTAL
Example Web         Web services              1        500.0 MB  3.0 GB   3.5 GB
Example Video       Media streaming services  1        2.0 GB    50.0 MB  2.0 GB
Unknown (4:999)     Media streaming services  1        300.0 MB  5.0 MB   305.0 MB
Facebook Messenger  Instant messengers        1        100.0 MB  20.0 MB  120.0 MB
Unidentified        Unknown                   1        10.0 MB   1.0 MB   11.0 MB
Unknown (9:4242)    Unknown (9)               1        1.0 MB    1.0 MB   2.0 MB
"
        );
    }

    #[test]
    fn empty_client_is_just_the_header() {
        let out = client(
            &svc()
                .client_traffic("kindle-paperwhite-bb04", W)
                .unwrap()
                .data,
            false,
        );
        assert_eq!(out.lines().count(), 1);
        assert!(out.starts_with("APP"));
    }

    #[test]
    fn app_names_blue_and_fallbacks_dim_with_color() {
        let apps_out = apps(&svc().apps_traffic(W).unwrap().data, true);
        assert!(
            apps_out.contains("\x1b[34mExample Web\x1b[0m"),
            "{apps_out}"
        );
        assert!(
            apps_out.contains("\x1b[2mUnknown (4:999)\x1b[0m"),
            "{apps_out}"
        );
        assert!(
            apps_out.contains("\x1b[2mUnidentified\x1b[0m"),
            "{apps_out}"
        );
        let one = client(
            &svc().client_traffic("macbook-pro-0001", W).unwrap().data,
            true,
        );
        assert!(one.contains("\x1b[34mFacebook Messenger\x1b[0m"), "{one}");
        assert!(one.contains("\x1b[2mUnidentified\x1b[0m"), "{one}");
        let list = clients(&svc().clients_traffic(W).unwrap().data, true);
        assert!(list.contains("\x1b[34mExample Video\x1b[0m"), "{list}");
        assert!(list.contains("\x1b[2mUnknown (4:999)\x1b[0m"), "{list}");
    }

    #[test]
    fn names_are_blue_with_color() {
        let out = clients(&svc().clients_traffic(W).unwrap().data, true);
        assert!(out.contains("\x1b[34mMacBook Pro\x1b[0m"), "{out}");
    }
}
