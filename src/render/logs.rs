//! `logs` output: journalctl `short` lines and JSON Lines.

use jiff::Timestamp;
use jiff::tz::TimeZone;

use super::style::{Paint, paint, printable};
use crate::error::{Error, Result};
use crate::model::{LogEntry, LogSeverity};

/// `Oct 11 10:43:15 client-devices: message`, the message coloured by
/// severity as journalctl colours by priority.
pub fn line(e: &LogEntry, tz: &TimeZone, color: bool) -> String {
    let time = Timestamp::from_millisecond(e.timestamp_ms)
        .unwrap_or(Timestamp::UNIX_EPOCH)
        .to_zoned(tz.clone())
        .strftime("%b %d %H:%M:%S");
    let category = if e.category.is_empty() {
        "unknown".to_string()
    } else {
        printable(&camel_to_kebab(&e.category))
    };
    let message = printable(&e.message);
    let message = match severity_paint(e.severity) {
        Some(p) => paint(&message, p, color),
        None => message,
    };
    format!("{time} {category}: {message}\n")
}

/// One compact JSON object and a newline.
pub fn json_line(e: &LogEntry) -> Result<String> {
    let mut s =
        serde_json::to_string(e).map_err(|err| Error::Other(format!("encode JSON: {err}")))?;
    s.push('\n');
    Ok(s)
}

fn severity_paint(s: LogSeverity) -> Option<Paint> {
    match s {
        LogSeverity::VeryHigh => Some(Paint::BoldRed),
        LogSeverity::High => Some(Paint::Red),
        LogSeverity::Medium | LogSeverity::Warning => Some(Paint::Yellow),
        LogSeverity::Low | LogSeverity::Info | LogSeverity::Unknown => None,
    }
}

/// `clientDevices` → `client-devices`, the spelling of `--category` values.
fn camel_to_kebab(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for c in s.chars() {
        if c.is_ascii_uppercase() {
            out.push('-');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use jiff::tz::TimeZone;

    use super::*;
    use crate::model::{LogEntry, LogSeverity};

    fn entry(severity: LogSeverity, category: &str, message: &str) -> LogEntry {
        LogEntry {
            id: "65f000000000000000000001".into(),
            time: String::new(),
            severity,
            category: category.into(),
            subcategory: String::new(),
            event: String::new(),
            title: String::new(),
            message: message.into(),
            devices: Vec::new(),
            clients: Vec::new(),
            details: BTreeMap::new(),
            timestamp_ms: 1791460800000,
        }
    }

    #[test]
    fn line_is_journalctl_short_without_host() {
        let e = entry(
            LogSeverity::Low,
            "internetAndWan",
            "Internet connection WAN1 is down.",
        );
        assert_eq!(
            line(&e, &TimeZone::UTC, false),
            "Oct 08 12:00:00 internet-and-wan: Internet connection WAN1 is down.\n"
        );
        let warsaw = TimeZone::get("Europe/Warsaw").unwrap();
        assert!(line(&e, &warsaw, false).starts_with("Oct 08 14:00:00 "));
    }

    #[test]
    fn message_is_coloured_by_severity() {
        let cases = [
            (LogSeverity::VeryHigh, "\x1b[1;31mm\x1b[0m"),
            (LogSeverity::High, "\x1b[31mm\x1b[0m"),
            (LogSeverity::Medium, "\x1b[33mm\x1b[0m"),
            (LogSeverity::Warning, "\x1b[33mm\x1b[0m"),
            (LogSeverity::Low, "m"),
            (LogSeverity::Info, "m"),
        ];
        for (sev, msg) in cases {
            let got = line(&entry(sev, "security", "m"), &TimeZone::UTC, true);
            assert_eq!(got, format!("Oct 08 12:00:00 security: {msg}\n"), "{sev}");
        }
        let plain = line(
            &entry(LogSeverity::VeryHigh, "security", "m"),
            &TimeZone::UTC,
            false,
        );
        assert!(!plain.contains('\x1b'));
    }

    #[test]
    fn unknown_severity_is_plain_and_empty_category_reads_unknown() {
        let got = line(&entry(LogSeverity::Unknown, "", "m"), &TimeZone::UTC, true);
        assert_eq!(got, "Oct 08 12:00:00 unknown: m\n");
    }

    #[test]
    fn control_characters_cannot_reach_the_terminal() {
        // A hostname the network controls, carrying an escape sequence,
        // a bell and a line break.
        let e = entry(
            LogSeverity::High,
            "secu\x1brity",
            "evil\x1b]0;pwned\x07 host\nfake line\u{9b}31m",
        );
        let plain = line(&e, &TimeZone::UTC, false);
        assert_eq!(
            plain,
            "Oct 08 12:00:00 secu\u{fffd}rity: evil\u{fffd}]0;pwned\u{fffd} host\u{fffd}fake line\u{fffd}31m\n"
        );
        let painted = line(&e, &TimeZone::UTC, true);
        assert_eq!(painted.matches('\x1b').count(), 2, "{painted:?}");
    }

    #[test]
    fn json_line_is_one_compact_line() {
        let s = json_line(&entry(LogSeverity::VeryHigh, "internetAndWan", "m")).unwrap();
        assert!(s.ends_with("}\n") && s.matches('\n').count() == 1);
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["severity"], "veryHigh");
        assert!(v.get("timestampMs").is_none());
    }

    #[test]
    fn camel_to_kebab_cases() {
        assert_eq!(camel_to_kebab("clientDevices"), "client-devices");
        assert_eq!(camel_to_kebab("unifiEthernetPorts"), "unifi-ethernet-ports");
        assert_eq!(camel_to_kebab("security"), "security");
    }
}
