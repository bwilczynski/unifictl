//! `radios list` and the RADIOS section of `devices get <ap>`.

use super::Table;
use super::style;
use crate::model::{NetworkRadio, NetworkRadioList, Radio};

/// Table order of bands: 2.4G, 5G, 6G, then anything else.
pub(crate) fn band_rank(band: &str) -> u8 {
    match band {
        "2.4G" => 0,
        "5G" => 1,
        "6G" => 2,
        _ => 3,
    }
}

/// Column titles for one radio, without the AP column.
pub(crate) fn header(wide: bool) -> Vec<&'static str> {
    let mut h = vec!["BAND", "CH", "WIDTH", "TX"];
    if wide {
        h.extend(["TX POWER", "MIN RSSI"]);
    }
    h.extend(["UTIL", "INTERF", "RETRY", "CLIENTS"]);
    if wide {
        h.push("EXPERIENCE");
    }
    h
}

fn num(v: Option<i64>) -> String {
    v.map_or_else(|| "-".to_string(), |v| v.to_string())
}

fn dbm(v: Option<i64>) -> String {
    v.map_or_else(|| "-".to_string(), |v| format!("{v} dBm"))
}

/// Cells for `header(wide)`; absent values are `-`.
pub(crate) fn cells(r: &Radio, wide: bool, color: bool) -> Vec<String> {
    let mut row = vec![
        r.band.clone(),
        num(r.channel),
        num(r.width),
        r.tx_power_mode.clone().unwrap_or_else(|| "-".to_string()),
    ];
    if wide {
        row.extend([dbm(r.tx_power), dbm(r.min_rssi)]);
    }
    row.extend([
        style::utilisation(r.utilisation, color),
        style::interference(r.interference, color),
        style::retries(r.retries, color),
        num(r.clients),
    ]);
    if wide {
        row.push(style::experience(r.satisfaction, color));
    }
    row
}

/// Rows sorted by AP name, then band, then radio name.
pub fn list_table(list: &NetworkRadioList, wide: bool, color: bool) -> String {
    let mut t = Table::new();
    let mut head = vec!["AP"];
    head.extend(header(wide));
    t.row(&head);
    let mut items: Vec<&NetworkRadio> = list.items.iter().collect();
    items.sort_by(|a, b| {
        a.device
            .name
            .cmp(&b.device.name)
            .then(band_rank(&a.radio.band).cmp(&band_rank(&b.radio.band)))
            .then(a.radio.band.cmp(&b.radio.band))
            .then(a.radio.name.cmp(&b.radio.name))
    });
    for nr in items {
        let mut row = vec![style::name(&nr.device.name, color)];
        row.extend(cells(&nr.radio, wide, color));
        t.row(&row.iter().map(String::as_str).collect::<Vec<_>>());
    }
    t.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::DeviceRef;

    fn radio(band: &str, name: &str) -> Radio {
        Radio {
            id: format!("x/{}", band.to_lowercase()),
            name: name.into(),
            band: band.into(),
            channel: Some(6),
            width: Some(20),
            tx_power_mode: Some("auto".into()),
            tx_power: Some(17),
            min_rssi: None,
            utilisation: Some(71),
            interference: Some(38),
            retries: Some(14.2),
            clients: Some(9),
            satisfaction: Some(88),
        }
    }

    fn nr(ap: &str, band: &str, name: &str) -> NetworkRadio {
        NetworkRadio {
            radio: radio(band, name),
            device: DeviceRef::new(ap.to_lowercase(), ap.into()),
        }
    }

    fn one(r: Radio) -> NetworkRadioList {
        NetworkRadioList {
            items: vec![NetworkRadio {
                radio: r,
                device: DeviceRef::new("ap-office".into(), "AP-Office".into()),
            }],
        }
    }

    fn tokens(line: &str) -> Vec<&str> {
        line.split_whitespace().collect()
    }

    #[test]
    fn narrow_header_and_row() {
        let out = list_table(&one(radio("2.4G", "wifi0")), false, false);
        let mut lines = out.lines();
        assert_eq!(
            tokens(lines.next().unwrap()),
            [
                "AP", "BAND", "CH", "WIDTH", "TX", "UTIL", "INTERF", "RETRY", "CLIENTS"
            ]
        );
        assert_eq!(
            tokens(lines.next().unwrap()),
            [
                "AP-Office",
                "2.4G",
                "6",
                "20",
                "auto",
                "71%",
                "38%",
                "14%",
                "9"
            ]
        );
    }

    #[test]
    fn wide_header_and_row() {
        let out = list_table(&one(radio("5G", "wifi1")), true, false);
        let mut lines = out.lines();
        let head = lines.next().unwrap();
        assert!(head.starts_with("AP  "), "{head}");
        assert!(head.contains("TX POWER  MIN RSSI  UTIL"), "{head}");
        assert!(head.ends_with("EXPERIENCE"), "{head}");
        assert_eq!(
            tokens(lines.next().unwrap()),
            [
                "AP-Office",
                "5G",
                "6",
                "20",
                "auto",
                "17",
                "dBm",
                "-",
                "71%",
                "38%",
                "14%",
                "9",
                "88%"
            ]
        );
    }

    #[test]
    fn absent_values_are_dashes() {
        let r = Radio {
            channel: None,
            width: None,
            tx_power_mode: None,
            tx_power: None,
            utilisation: None,
            interference: None,
            retries: None,
            clients: None,
            satisfaction: None,
            ..radio("2.4G", "wifi0")
        };
        let out = list_table(&one(r), true, false);
        let row = tokens(out.lines().nth(1).unwrap());
        assert_eq!(row.len(), 12, "{row:?}");
        assert!(row[2..].iter().all(|c| *c == "-"), "{row:?}");
    }

    #[test]
    fn rows_sorted_by_ap_then_band_then_radio() {
        let list = NetworkRadioList {
            items: vec![
                nr("AP-Office", "6G", "wifi2"),
                nr("AP-Office", "5G", "wifi1"),
                nr("AP-Living-Room", "2.4G", "wifi0"),
                nr("AP-Office", "ad", "wifi3"),
                nr("AP-Office", "2.4G", "wifi0"),
            ],
        };
        let order: Vec<String> = list_table(&list, false, false)
            .lines()
            .skip(1)
            .map(|l| tokens(l)[..2].join(" "))
            .collect();
        assert_eq!(
            order,
            [
                "AP-Living-Room 2.4G",
                "AP-Office 2.4G",
                "AP-Office 5G",
                "AP-Office 6G",
                "AP-Office ad"
            ]
        );
    }

    #[test]
    fn health_columns_coloured_only_when_on() {
        let list = one(radio("2.4G", "wifi0"));
        let out = list_table(&list, false, true);
        assert!(out.contains("\x1b[33m71%\x1b[0m"), "{out}");
        assert!(out.contains("\x1b[31m38%\x1b[0m"), "{out}");
        assert!(out.contains("\x1b[33m14%\x1b[0m"), "{out}");
        assert!(out.contains("\x1b[34mAP-Office\x1b[0m"), "{out}");
        assert!(!list_table(&list, false, false).contains('\x1b'));
    }

    #[test]
    fn empty_list_is_header_only() {
        let out = list_table(&NetworkRadioList { items: vec![] }, false, false);
        assert_eq!(out.lines().count(), 1);
    }
}
