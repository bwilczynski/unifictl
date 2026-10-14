//! Access point radios: `radio_table` (configuration) joined with
//! `radio_table_stats` (live figures) on the radio name.

use super::Service;
use super::common::*;
use crate::error::Result;
use crate::model::*;
use crate::unifi::types::{Device, RadioStats};

/// Query filter for `radios list`; `None` means every access point.
#[derive(Debug, Clone, Default)]
pub struct RadioFilter {
    pub device_id: Option<String>,
}

/// `ng` → `2.4G`, `na` → `5G`, `6e` → `6G`; any other code as is.
fn band(code: &str) -> String {
    match code {
        "ng" => "2.4G",
        "na" => "5G",
        "6e" => "6G",
        other => other,
    }
    .to_string()
}

/// Airtime used by other networks and noise: total minus this radio's own
/// RX and TX, never below 0 (the three are sampled separately).
fn interference(s: &RadioStats) -> Option<i64> {
    let total = s.cu_total?;
    Some((total - s.cu_self_rx.unwrap_or(0) - s.cu_self_tx.unwrap_or(0)).max(0))
}

/// An access point's radios in `radio_table` order. A disconnected AP's
/// stats are stale, so they are dropped like its experience score.
pub(crate) fn build_radios(d: &Device) -> Vec<Radio> {
    let device_id = to_kebab(&d.name);
    let connected = map_device_status(d.state) == DeviceStatus::Connected;
    let bands: Vec<String> = d.radio_table.iter().map(|r| band(&r.radio)).collect();
    d.radio_table
        .iter()
        .zip(&bands)
        .map(|(r, band)| {
            let stats = d
                .radio_table_stats
                .iter()
                .find(|s| connected && s.name == r.name);
            let unique = !band.is_empty() && bands.iter().filter(|b| *b == band).count() == 1;
            let suffix = if unique {
                band.to_lowercase()
            } else {
                r.name.clone()
            };
            Radio {
                id: format!("{device_id}/{suffix}"),
                name: r.name.clone(),
                band: band.clone(),
                channel: stats.and_then(|s| s.channel).or(r.channel),
                width: r.ht,
                tx_power_mode: r.tx_power_mode.clone(),
                tx_power: stats.and_then(|s| s.tx_power),
                min_rssi: r.min_rssi.filter(|_| r.min_rssi_enabled),
                utilisation: stats.and_then(|s| s.cu_total),
                interference: stats.and_then(interference),
                retries: stats.and_then(|s| s.tx_retries_pct),
                clients: stats.and_then(|s| s.num_sta),
                satisfaction: stats.and_then(|s| s.satisfaction).filter(|s| *s >= 0),
            }
        })
        .collect()
}

impl Service {
    /// Every access point's radios, in controller order.
    pub fn list_radios(&self, filter: &RadioFilter) -> Result<NetworkRadioList> {
        let devices = self.api.devices()?;
        let items = devices
            .iter()
            .filter(|d| d.kind == "uap")
            .filter(|d| {
                filter
                    .device_id
                    .as_deref()
                    .is_none_or(|id| to_kebab(&d.name) == id)
            })
            .flat_map(|d| {
                let device = device_ref(d);
                build_radios(d).into_iter().map(move |radio| NetworkRadio {
                    radio,
                    device: device.clone(),
                })
            })
            .collect();
        Ok(NetworkRadioList { items })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::testutil::{fixtures, svc};
    use crate::unifi::types::RadioEntry;

    fn list(device_id: Option<&str>) -> Vec<NetworkRadio> {
        let filter = RadioFilter {
            device_id: device_id.map(String::from),
        };
        svc(fixtures()).list_radios(&filter).unwrap().items
    }

    fn ap(state: i64, table: Vec<RadioEntry>, stats: Vec<RadioStats>) -> Device {
        Device {
            name: "AP Office".into(),
            kind: "uap".into(),
            state,
            radio_table: table,
            radio_table_stats: stats,
            ..Default::default()
        }
    }

    fn entry(name: &str, radio: &str) -> RadioEntry {
        RadioEntry {
            name: name.into(),
            radio: radio.into(),
            channel: Some(36),
            ht: Some(80),
            ..Default::default()
        }
    }

    fn stats(name: &str) -> RadioStats {
        RadioStats {
            name: name.into(),
            channel: Some(40),
            cu_total: Some(30),
            cu_self_rx: Some(5),
            cu_self_tx: Some(5),
            tx_retries_pct: Some(4.0),
            num_sta: Some(2),
            satisfaction: Some(90),
            tx_power: Some(20),
        }
    }

    #[test]
    fn lists_every_ap_radio_in_controller_order() {
        let ids: Vec<String> = list(None).into_iter().map(|r| r.radio.id).collect();
        assert_eq!(
            ids,
            ["uap-02/2.4g", "uap-02/5g", "uap-01/2.4g", "uap-01/5g"]
        );
    }

    #[test]
    fn joins_config_and_stats() {
        let items = list(Some("uap-02"));
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].device.id, "uap-02");
        assert_eq!(items[0].device.name, "UAP-02");
        assert_eq!(
            items[0].radio,
            Radio {
                id: "uap-02/2.4g".into(),
                name: "wifi0".into(),
                band: "2.4G".into(),
                channel: Some(6),
                width: Some(20),
                tx_power_mode: None,
                tx_power: Some(17),
                min_rssi: None,
                utilisation: Some(15),
                interference: Some(1),
                retries: Some(31.4),
                clients: Some(3),
                satisfaction: Some(100),
            }
        );
        assert_eq!(items[1].radio.band, "5G");
        assert_eq!(items[1].radio.width, Some(80));
    }

    #[test]
    fn unknown_or_non_ap_device_is_empty() {
        assert!(list(Some("nope")).is_empty());
        assert!(list(Some("us-8")).is_empty());
    }

    #[test]
    fn stats_channel_wins_over_config() {
        let r = &build_radios(&ap(1, vec![entry("wifi1", "na")], vec![stats("wifi1")]))[0];
        assert_eq!(r.channel, Some(40));
        assert_eq!(r.utilisation, Some(30));
        assert_eq!(r.interference, Some(20));
    }

    #[test]
    fn offline_ap_drops_stale_stats() {
        let r = &build_radios(&ap(0, vec![entry("wifi1", "na")], vec![stats("wifi1")]))[0];
        assert_eq!(r.channel, Some(36), "configured channel");
        assert_eq!(r.width, Some(80));
        for v in [
            r.utilisation,
            r.interference,
            r.clients,
            r.satisfaction,
            r.tx_power,
        ] {
            assert_eq!(v, None);
        }
        assert_eq!(r.retries, None);
    }

    #[test]
    fn radio_without_stats_has_no_live_figures() {
        let r = &build_radios(&ap(1, vec![entry("wifi0", "ng")], vec![stats("wifi9")]))[0];
        assert_eq!(
            (r.channel, r.utilisation, r.clients),
            (Some(36), None, None)
        );
    }

    #[test]
    fn interference_floors_at_zero() {
        let s = RadioStats {
            cu_total: Some(10),
            cu_self_rx: Some(8),
            cu_self_tx: Some(7),
            ..stats("wifi0")
        };
        let r = &build_radios(&ap(1, vec![entry("wifi0", "ng")], vec![s]))[0];
        assert_eq!(r.interference, Some(0));
        let s = RadioStats {
            cu_self_rx: None,
            cu_self_tx: None,
            ..stats("wifi0")
        };
        let r = &build_radios(&ap(1, vec![entry("wifi0", "ng")], vec![s]))[0];
        assert_eq!(r.interference, Some(30), "missing own airtime counts as 0");
    }

    #[test]
    fn duplicate_band_ids_use_radio_name() {
        let d = ap(
            1,
            vec![
                entry("wifi0", "ng"),
                entry("wifi1", "na"),
                entry("wifi2", "na"),
            ],
            vec![],
        );
        let ids: Vec<String> = build_radios(&d).into_iter().map(|r| r.id).collect();
        assert_eq!(
            ids,
            ["ap-office/2.4g", "ap-office/wifi1", "ap-office/wifi2"]
        );
    }

    #[test]
    fn band_codes() {
        let d = ap(
            1,
            vec![
                entry("wifi0", "ng"),
                entry("wifi1", "na"),
                entry("wifi2", "6e"),
                entry("wifi3", "ad"),
                entry("wifi4", ""),
            ],
            vec![],
        );
        let r = build_radios(&d);
        let bands: Vec<&str> = r.iter().map(|r| r.band.as_str()).collect();
        assert_eq!(bands, ["2.4G", "5G", "6G", "ad", ""]);
        assert_eq!(r[2].id, "ap-office/6g");
        assert_eq!(r[3].id, "ap-office/ad");
        assert_eq!(r[4].id, "ap-office/wifi4", "no band: radio name");
    }

    #[test]
    fn min_rssi_only_when_enabled_and_negative_satisfaction_dropped() {
        let on = RadioEntry {
            min_rssi_enabled: true,
            min_rssi: Some(-75),
            ..entry("wifi0", "ng")
        };
        let off = RadioEntry {
            min_rssi: Some(-80),
            ..entry("wifi1", "na")
        };
        let s = RadioStats {
            satisfaction: Some(-1),
            ..stats("wifi0")
        };
        let r = build_radios(&ap(1, vec![on, off], vec![s]));
        assert_eq!((r[0].min_rssi, r[1].min_rssi), (Some(-75), None));
        assert_eq!(r[0].satisfaction, None);
    }

    #[test]
    fn json_flattens_radio_with_device_and_keeps_nulls() {
        let items = list(Some("uap-02"));
        let v = serde_json::to_value(&items[0]).unwrap();
        assert_eq!(v["id"], "uap-02/2.4g");
        assert_eq!(v["device"]["id"], "uap-02");
        assert_eq!(v["band"], "2.4G");
        assert_eq!(v["utilisation"], 15);
        assert_eq!(v["retries"], 31.4);
        assert!(v["txPowerMode"].is_null());
        assert!(v["minRssi"].is_null());
    }

    #[test]
    fn ids_are_radio_ids() {
        let list = svc(fixtures())
            .list_radios(&RadioFilter::default())
            .unwrap();
        assert_eq!(list.ids()[0], "uap-02/2.4g");
    }
}
