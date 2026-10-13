//! Ported from homelab-cli/internal/output/output.go.

use jiff::{Timestamp, tz::TimeZone};

use crate::model::{LinkSpeed, WifiBand};

/// Seconds → "1d 0h 0m 0s"; leading zero segments skipped, seconds always shown.
pub fn format_uptime(seconds: i64) -> String {
    let (d, rem) = (seconds / 86400, seconds % 86400);
    let (h, rem) = (rem / 3600, rem % 3600);
    let (m, s) = (rem / 60, rem % 60);
    let mut parts: Vec<String> = Vec::new();
    for (val, unit) in [(d, "d"), (h, "h"), (m, "m"), (s, "s")] {
        if !parts.is_empty() || val > 0 || unit == "s" {
            parts.push(format!("{val}{unit}"));
        }
    }
    parts.join(" ")
}

fn binary_units(n: i64, suffix: &str) -> String {
    const UNIT: i64 = 1024;
    if n < UNIT {
        return format!("{n} B{suffix}");
    }
    let (mut div, mut exp, mut q) = (UNIT, 0usize, n / UNIT);
    while q >= UNIT {
        div *= UNIT;
        exp += 1;
        q /= UNIT;
    }
    const UNITS: [&str; 6] = ["K", "M", "G", "T", "P", "E"];
    format!("{:.1} {}B{suffix}", n as f64 / div as f64, UNITS[exp])
}

pub fn format_bytes(n: i64) -> String {
    binary_units(n, "")
}

pub fn format_bytes_per_sec(n: i64) -> String {
    binary_units(n, "/s")
}

pub fn format_link_speed(s: LinkSpeed) -> &'static str {
    match s {
        LinkSpeed::E => "10 Mbps",
        LinkSpeed::Fe => "100 Mbps",
        LinkSpeed::Gbe1 => "1 GbE",
        LinkSpeed::Gbe2p5 => "2.5 GbE",
        LinkSpeed::Gbe5 => "5 GbE",
        LinkSpeed::Gbe10 => "10 GbE",
    }
}

/// Epoch seconds → "YYYY-MM-DD HH:MM" in `tz` (absolute, so output is stable).
pub fn format_last_seen(epoch: i64, tz: &TimeZone) -> String {
    Timestamp::from_second(epoch)
        .map(|t| {
            t.to_zoned(tz.clone())
                .strftime("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|_| "-".to_string())
}

pub fn format_bands(bands: &[WifiBand]) -> String {
    bands
        .iter()
        .map(|b| match b {
            WifiBand::Band2g => "2.4 GHz",
            WifiBand::Band5g => "5 GHz",
            WifiBand::Band6g => "6 GHz",
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uptime() {
        assert_eq!(format_uptime(86400), "1d 0h 0m 0s");
        assert_eq!(format_uptime(7200), "2h 0m 0s");
        assert_eq!(format_uptime(45), "45s");
        assert_eq!(format_uptime(0), "0s");
        assert_eq!(format_uptime(3_651_480), "42d 6h 18m 0s");
    }

    #[test]
    fn bytes() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes_per_sec(0), "0 B/s");
        assert_eq!(format_bytes_per_sec(18 * 1024 * 1024), "18.0 MB/s");
        assert_eq!(format_bytes(i64::MAX), "8.0 EB");
    }

    #[test]
    fn link_speed_and_bands() {
        assert_eq!(format_link_speed(LinkSpeed::Gbe2p5), "2.5 GbE");
        assert_eq!(
            format_bands(&[WifiBand::Band2g, WifiBand::Band5g]),
            "2.4 GHz, 5 GHz"
        );
        assert_eq!(format_bands(&[]), "");
    }

    #[test]
    fn last_seen_is_minutes_in_the_given_zone() {
        assert_eq!(
            format_last_seen(1791712800, &TimeZone::UTC),
            "2026-10-11 10:00"
        );
        assert_eq!(format_last_seen(0, &TimeZone::UTC), "1970-01-01 00:00");
        let tokyo = TimeZone::get("Asia/Tokyo").unwrap();
        assert_eq!(format_last_seen(1791712800, &tokyo), "2026-10-11 19:00");
    }

    #[test]
    fn link_speed_mbps() {
        assert_eq!(LinkSpeed::E.mbps(), 10);
        assert_eq!(LinkSpeed::Gbe2p5.mbps(), 2500);
        assert_eq!(LinkSpeed::Gbe10.mbps(), 10000);
    }
}
