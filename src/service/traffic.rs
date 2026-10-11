//! Internet traffic per client and application (`clients traffic`).

use std::collections::HashMap;

use super::Service;
use super::common::{
    client_id_traffic, client_id_v2, client_name_traffic, client_name_v2, normalize_mac,
};
use crate::error::{Error, Result};
use crate::model::{
    AppRef, AppTraffic, AppTrafficList, ClientAppTraffic, ClientAppTrafficList, ClientTraffic,
    ClientTrafficList, DpiCategoryRef,
};
use crate::unifi::fetch;
use crate::unifi::types::{
    AppUsage, ClientUsage, ClientV2, DpiEntry, TrafficClient, TrafficReport,
};

/// The category and application DPI reports for traffic it could not identify.
const UNIDENTIFIED: (i64, i64) = (255, 65535);

/// Epoch milliseconds, `start_ms <= end_ms`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrafficWindow {
    pub start_ms: i64,
    pub end_ms: i64,
}

/// A traffic view, and why application names were unavailable, if they were
/// (the DPI catalogue could not be read, e.g. without an API key).
#[derive(Debug)]
pub struct TrafficView<T> {
    pub data: T,
    pub names_unavailable: Option<String>,
}

/// The catalogue's application ID for a traffic `(category, application)`.
pub(crate) fn app_id(category: i64, application: i64) -> i64 {
    (category << 16) | application
}

/// DPI names by number, with fallbacks for what the catalogue lacks.
#[derive(Debug, Default)]
pub(crate) struct DpiNames {
    categories: HashMap<i64, String>,
    apps: HashMap<i64, String>,
}

impl DpiNames {
    pub(crate) fn new(categories: &[DpiEntry], apps: &[DpiEntry]) -> Self {
        let by_id = |entries: &[DpiEntry]| entries.iter().map(|e| (e.id, e.name.clone())).collect();
        Self {
            categories: by_id(categories),
            apps: by_id(apps),
        }
    }

    pub(crate) fn app(&self, category: i64, application: i64) -> AppRef {
        let id = app_id(category, application);
        let known = if (category, application) == UNIDENTIFIED {
            None
        } else {
            self.apps.get(&id)
        };
        let named = known.is_some();
        let name = match known {
            Some(name) => name.clone(),
            None if (category, application) == UNIDENTIFIED => "Unidentified".to_string(),
            None => format!("Unknown ({category}:{application})"),
        };
        let category_name = self
            .categories
            .get(&category)
            .cloned()
            .unwrap_or_else(|| format!("Unknown ({category})"));
        AppRef {
            id,
            name,
            category: DpiCategoryRef {
                id: category,
                name: category_name,
            },
            named,
        }
    }
}

impl Service {
    /// Every client with traffic in the window, largest first.
    pub fn clients_traffic(&self, w: TrafficWindow) -> Result<TrafficView<ClientTrafficList>> {
        let t = self.traffic_with_names(w)?;
        let mut items: Vec<ClientTraffic> = t
            .report
            .client_usage_by_app
            .iter()
            .map(|c| client_traffic(c, &t.names, &t.known))
            .collect();
        items.sort_by(|a, b| {
            b.total_bytes
                .cmp(&a.total_bytes)
                .then_with(|| a.name.cmp(&b.name))
        });
        Ok(TrafficView {
            data: ClientTrafficList { items },
            names_unavailable: t.names_unavailable,
        })
    }

    /// One client's traffic by application. A client the controller knows
    /// but with no traffic in the window has no items.
    pub fn client_traffic(
        &self,
        id: &str,
        w: TrafficWindow,
    ) -> Result<TrafficView<ClientAppTrafficList>> {
        let t = self.traffic_with_names(w)?;
        let found = t
            .report
            .client_usage_by_app
            .iter()
            .map(|c| (c, identity(&c.client, &t.known)))
            .find(|(_, (cid, _))| cid == id);
        let data = match found {
            Some((c, (_, name))) => {
                let mut items: Vec<ClientAppTraffic> = c
                    .usage_by_app
                    .iter()
                    .map(|u| ClientAppTraffic {
                        app: t.names.app(u.category, u.application),
                        bytes_received: u.bytes_received,
                        bytes_transmitted: u.bytes_transmitted,
                        total_bytes: u.total_bytes,
                        activity_seconds: u.activity_seconds,
                    })
                    .collect();
                items.sort_by(|a, b| {
                    b.total_bytes
                        .cmp(&a.total_bytes)
                        .then_with(|| a.app.name.cmp(&b.app.name))
                });
                ClientAppTrafficList {
                    id: id.to_string(),
                    name,
                    mac: normalize_mac(&c.client.mac),
                    items,
                }
            }
            None => {
                let known = t
                    .known
                    .iter()
                    .find(|c| client_id_v2(c) == id)
                    .ok_or_else(|| Error::NotFound(format!("Network client not found: {id}")))?;
                ClientAppTrafficList {
                    id: id.to_string(),
                    name: client_name_v2(known),
                    mac: normalize_mac(&known.mac),
                    items: Vec::new(),
                }
            }
        };
        Ok(TrafficView {
            data,
            names_unavailable: t.names_unavailable,
        })
    }

    /// Site-wide traffic per application, largest first.
    pub fn apps_traffic(&self, w: TrafficWindow) -> Result<TrafficView<AppTrafficList>> {
        let t = self.traffic_with_names(w)?;
        let mut items: Vec<AppTraffic> = t
            .report
            .total_usage_by_app
            .iter()
            .map(|u| AppTraffic {
                app: t.names.app(u.category, u.application),
                client_count: u.client_count,
                bytes_received: u.bytes_received,
                bytes_transmitted: u.bytes_transmitted,
                total_bytes: u.total_bytes,
            })
            .collect();
        items.sort_by(|a, b| {
            b.total_bytes
                .cmp(&a.total_bytes)
                .then_with(|| a.app.name.cmp(&b.app.name))
        });
        Ok(TrafficView {
            data: AppTrafficList { items },
            names_unavailable: t.names_unavailable,
        })
    }

    /// The report, the DPI names and the client list, read in parallel. A
    /// catalogue that cannot be read is not an error: names fall back and
    /// the reason is kept.
    fn traffic_with_names(&self, w: TrafficWindow) -> Result<Traffic> {
        fetch!(
            report = self.api.traffic(w.start_ms, w.end_ms),
            categories = Ok::<_, Error>(self.api.dpi_categories()),
            apps = Ok::<_, Error>(self.api.dpi_applications()),
            known = self.api.all_clients(self.history_days),
        );
        let (names, names_unavailable) = match (categories, apps) {
            (Ok(c), Ok(a)) => (DpiNames::new(&c, &a), None),
            (Err(e), _) | (_, Err(e)) => (DpiNames::default(), Some(e.to_string())),
        };
        Ok(Traffic {
            report,
            names,
            names_unavailable,
            known,
        })
    }
}

/// What every traffic view is built from.
struct Traffic {
    report: TrafficReport,
    names: DpiNames,
    names_unavailable: Option<String>,
    /// `clients list`'s clients, for IDs and names that match it.
    known: Vec<ClientV2>,
}

/// A report client's ID and name: those `clients list` gives its MAC, so
/// `clients get` and `clients traffic <id>` accept the ID; else derived from
/// the report (a client gone from the list's history).
fn identity(c: &TrafficClient, known: &[ClientV2]) -> (String, String) {
    let mac = normalize_mac(&c.mac);
    match known.iter().find(|k| normalize_mac(&k.mac) == mac) {
        Some(k) => (client_id_v2(k), client_name_v2(k)),
        None => (client_id_traffic(c), client_name_traffic(c)),
    }
}

/// A client's totals summed over its applications, and its largest one
/// (ties broken by name).
fn client_traffic(c: &ClientUsage, names: &DpiNames, known: &[ClientV2]) -> ClientTraffic {
    let sum = |field: fn(&AppUsage) -> i64| c.usage_by_app.iter().map(field).sum();
    let top_app = c
        .usage_by_app
        .iter()
        .map(|u| (u.total_bytes, names.app(u.category, u.application)))
        .max_by(|(ta, a), (tb, b)| ta.cmp(tb).then_with(|| b.name.cmp(&a.name)))
        .map(|(_, app)| app);
    let (id, name) = identity(&c.client, known);
    ClientTraffic {
        id,
        name,
        mac: normalize_mac(&c.client.mac),
        bytes_received: sum(|u| u.bytes_received),
        bytes_transmitted: sum(|u| u.bytes_transmitted),
        total_bytes: sum(|u| u.total_bytes),
        top_app,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use crate::service::testutil::{fixtures, svc};
    use crate::unifi::fixtures::FixtureApi;
    use crate::unifi::types::{ClientUsage, TrafficClient};

    const W: TrafficWindow = TrafficWindow {
        start_ms: 0,
        end_ms: 1,
    };

    fn names() -> DpiNames {
        let api = fixtures();
        DpiNames::new(&api.dpi_categories.unwrap(), &api.dpi_apps.unwrap())
    }

    #[test]
    fn app_id_packs_category_and_application() {
        assert_eq!(app_id(1, 2), 65538);
        assert_eq!(app_id(0, 127), 127);
        assert_eq!(app_id(255, 65535), 16777215);
    }

    #[test]
    fn names_resolve_with_fallbacks() {
        let n = names();
        let known = n.app(4, 1);
        assert_eq!(known.id, 262145);
        assert_eq!(known.name, "Example Video");
        assert_eq!(known.category.name, "Media streaming services");
        let unknown_app = n.app(4, 999);
        assert_eq!(unknown_app.name, "Unknown (4:999)");
        assert_eq!(unknown_app.category.name, "Media streaming services");
        let unknown_both = n.app(9, 4242);
        assert_eq!(unknown_both.name, "Unknown (9:4242)");
        assert_eq!(unknown_both.category.name, "Unknown (9)");
        let unidentified = n.app(255, 65535);
        assert_eq!(unidentified.name, "Unidentified");
        assert_eq!(unidentified.category.name, "Unknown");
        assert_eq!(DpiNames::default().app(0, 127).name, "Unknown (0:127)");
        assert!(known.named);
        assert!(!unknown_app.named && !unknown_both.named && !unidentified.named);
    }

    #[test]
    fn clients_sorted_by_total_with_top_app() {
        let v = svc(fixtures()).clients_traffic(W).unwrap();
        assert_eq!(v.names_unavailable, None);
        let ids: Vec<_> = v.data.items.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(
            ids,
            ["nas-1-0002", "macbook-pro-0001", "sonos-one-sl-l-bb03"]
        );
        let mb = &v.data.items[1];
        assert_eq!(mb.name, "MacBook Pro");
        assert_eq!(mb.mac, "3c:ee:ee:00:00:01");
        assert_eq!(mb.bytes_received, 2262827008);
        assert_eq!(mb.bytes_transmitted, 74448896);
        assert_eq!(mb.total_bytes, 2337275904);
        assert_eq!(mb.top_app.as_ref().unwrap().name, "Example Video");
        assert_eq!(v.data.items[0].name, "nas-1");
        assert_eq!(
            v.data.items[0].top_app.as_ref().unwrap().name,
            "Example Web"
        );
    }

    #[test]
    fn client_without_name_falls_back_to_mac() {
        let mut api = fixtures();
        api.traffic.client_usage_by_app = vec![ClientUsage {
            client: TrafficClient {
                mac: "AA:BB:CC:00:12:34".into(),
                name: Some(String::new()),
                hostname: None,
                is_wired: true,
            },
            usage_by_app: vec![],
        }];
        let v = svc(api).clients_traffic(W).unwrap();
        let c = &v.data.items[0];
        assert_eq!(c.name, "AA:BB:CC:00:12:34");
        assert_eq!(c.id, "aa-bb-cc-00-12-34-1234");
        assert_eq!(c.total_bytes, 0);
        assert_eq!(c.top_app, None);
    }

    #[test]
    fn one_client_by_app() {
        let v = svc(fixtures())
            .client_traffic("macbook-pro-0001", W)
            .unwrap();
        assert_eq!(v.data.name, "MacBook Pro");
        let apps: Vec<_> = v.data.items.iter().map(|a| a.app.name.as_str()).collect();
        assert_eq!(
            apps,
            ["Example Video", "Facebook Messenger", "Unidentified"]
        );
        assert_eq!(v.data.items[0].activity_seconds, 5400);
        assert_eq!(v.data.items[1].app.id, 127);
    }

    #[test]
    fn known_client_without_traffic_is_empty() {
        let v = svc(fixtures())
            .client_traffic("kindle-paperwhite-bb04", W)
            .unwrap();
        assert_eq!(v.data.name, "Kindle Paperwhite");
        assert_eq!(v.data.mac, "e0:f7:28:aa:bb:04");
        assert!(v.data.items.is_empty());
    }

    #[test]
    fn unknown_client_is_not_found() {
        let err = svc(fixtures()).client_traffic("nobody-00", W).unwrap_err();
        assert!(matches!(err, Error::NotFound(_)));
        assert_eq!(err.to_string(), "Network client not found: nobody-00");
    }

    #[test]
    fn apps_sorted_by_total() {
        let v = svc(fixtures()).apps_traffic(W).unwrap();
        let apps: Vec<_> = v.data.items.iter().map(|a| a.app.name.as_str()).collect();
        assert_eq!(
            apps,
            [
                "Example Web",
                "Example Video",
                "Unknown (4:999)",
                "Facebook Messenger",
                "Unidentified",
                "Unknown (9:4242)"
            ]
        );
        assert_eq!(v.data.items[0].client_count, 1);
    }

    #[test]
    fn missing_catalogue_falls_back_and_says_so() {
        let api = FixtureApi {
            dpi_categories: None,
            dpi_apps: None,
            ..fixtures()
        };
        let v = svc(api).apps_traffic(W).unwrap();
        assert_eq!(v.names_unavailable.as_deref(), Some("needs an API key"));
        assert_eq!(v.data.items[1].app.name, "Unknown (4:1)");
        assert_eq!(v.data.items[1].app.category.name, "Unknown (4)");
    }

    #[test]
    fn ids_follow_the_client_list_when_the_report_names_differ() {
        let mut api = fixtures();
        // The report has no alias for the MacBook; `clients list` does.
        api.traffic.client_usage_by_app[0].client.name = Some(String::new());
        let s = svc(api);
        let v = s.clients_traffic(W).unwrap();
        let mb = v
            .data
            .items
            .iter()
            .find(|c| c.mac == "3c:ee:ee:00:00:01")
            .unwrap();
        assert_eq!(mb.id, "macbook-pro-0001");
        assert_eq!(mb.name, "MacBook Pro");
        let one = s.client_traffic("macbook-pro-0001", W).unwrap();
        assert_eq!(one.data.items.len(), 3);
    }

    #[test]
    fn traffic_failure_is_an_error() {
        let api = FixtureApi {
            fail: true,
            ..fixtures()
        };
        assert!(svc(api).clients_traffic(W).is_err());
    }

    #[test]
    fn json_is_camel_case_with_flattened_app() {
        let v = svc(fixtures())
            .client_traffic("macbook-pro-0001", W)
            .unwrap();
        let j = serde_json::to_value(&v.data.items[1]).unwrap();
        assert_eq!(
            j,
            serde_json::json!({
                "id": 127, "name": "Facebook Messenger",
                "category": {"id": 0, "name": "Instant messengers"},
                "bytesReceived": 104857600, "bytesTransmitted": 20971520,
                "totalBytes": 125829120, "activitySeconds": 1800
            })
        );
    }
}
