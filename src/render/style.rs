//! ANSI colour for status cells, health metrics, traffic and log messages. Only the basic SGR colours are used, so the
//! terminal's own palette (e.g. a Catppuccin theme) decides the actual shade.

use super::format::format_link_speed;
use super::health::{self, Health};
use crate::model::{AppRef, ClientStatus, DeviceStatus, LinkSpeed, PortState, Verdict, WanStatus};
use crate::setup::check::Status;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Paint {
    Green,
    Red,
    BoldRed,
    Yellow,
    Blue,
    BoldBlue,
    Cyan,
    Magenta,
    Dim,
}

impl Paint {
    const ALL: [Self; 9] = [
        Self::Green,
        Self::Red,
        Self::BoldRed,
        Self::Yellow,
        Self::Blue,
        Self::BoldBlue,
        Self::Cyan,
        Self::Magenta,
        Self::Dim,
    ];

    fn sgr(self) -> &'static str {
        match self {
            Self::Green => "32",
            Self::Red => "31",
            Self::BoldRed => "1;31",
            Self::Yellow => "33",
            Self::Blue => "34",
            Self::BoldBlue => "1;34",
            Self::Cyan => "36",
            Self::Magenta => "35",
            Self::Dim => "2",
        }
    }
}

/// `text`, made [`printable`], wrapped in `p`'s escape codes when `on`.
pub fn paint(text: &str, p: Paint, on: bool) -> String {
    let text = printable(text);
    if on {
        format!("\x1b[{}m{text}\x1b[0m", p.sgr())
    } else {
        text
    }
}

/// `s` with control characters (C0, DEL, C1) replaced by U+FFFD. Names,
/// SSIDs and log messages come from the network (a DHCP hostname, say); an
/// escape sequence or newline in one must not reach the terminal. JSON
/// escapes them itself.
pub fn printable(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { '\u{fffd}' } else { c })
        .collect()
}

/// `s` [`printable`] except for whole spans [`paint`] writes (colour code,
/// printable text, reset): a last line of defence for table cells that hold
/// network text unpainted. A lone colour code is replaced like any other
/// escape, so it cannot colour the rows after it.
pub(crate) fn terminal_safe(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(c) = rest.chars().next() {
        let len = match painted_span(rest) {
            Some(len) => {
                out.push_str(&rest[..len]);
                len
            }
            None => {
                out.push(if c.is_control() { '\u{fffd}' } else { c });
                c.len_utf8()
            }
        };
        rest = &rest[len..];
    }
    out
}

/// The length of the [`paint`] span `s` starts with, if any.
fn painted_span(s: &str) -> Option<usize> {
    const RESET: &str = "\x1b[0m";
    let body = s.strip_prefix("\x1b[")?;
    let (code, rest) = body.split_once('m')?;
    Paint::ALL.iter().find(|p| p.sgr() == code)?;
    let end = rest.find(RESET)?;
    let text = &rest[..end];
    (!text.chars().any(char::is_control)).then_some(2 + code.len() + 1 + end + RESET.len())
}

/// `text` green, yellow or red by `h`.
pub fn graded(text: &str, h: Health, on: bool) -> String {
    let p = match h {
        Health::Good => Paint::Green,
        Health::Fair => Paint::Yellow,
        Health::Poor => Paint::Red,
    };
    paint(text, p, on)
}

/// Downloaded / received bytes: cyan, close to UniFi's light blue.
pub fn down(text: &str, on: bool) -> String {
    paint(text, Paint::Cyan, on)
}

/// Uploaded / transmitted bytes: magenta, close to UniFi's purple.
pub fn up(text: &str, on: bool) -> String {
    paint(text, Paint::Magenta, on)
}

fn dash() -> String {
    "-".to_string()
}

/// `98%`, graded; `-` when the controller gives no score.
pub fn experience(pct: Option<i64>, on: bool) -> String {
    pct.map_or_else(dash, |p| {
        graded(&format!("{p}%"), health::experience(p), on)
    })
}

/// `-69 dBm`, graded; `-` when unknown.
pub fn signal(dbm: Option<i64>, on: bool) -> String {
    dbm.map_or_else(dash, |d| graded(&format!("{d} dBm"), health::signal(d), on))
}

/// WAN availability, `99.93%`, graded on the displayed value; `-` when
/// unreported.
pub fn availability(pct: Option<f64>, on: bool) -> String {
    pct.map_or_else(dash, |p| {
        let shown = (p * 100.0).round() / 100.0;
        graded(&format!("{shown:.2}%"), health::availability(shown), on)
    })
}

/// WAN latency, `11 ms`, graded; `-` when unreported.
pub fn latency(ms: Option<i64>, on: bool) -> String {
    ms.map_or_else(dash, |m| graded(&format!("{m} ms"), health::latency(m), on))
}

/// `71%` graded by `grade`; `-` when unreported.
fn graded_pct(pct: Option<i64>, grade: fn(i64) -> Health, on: bool) -> String {
    pct.map_or_else(dash, |p| graded(&format!("{p}%"), grade(p), on))
}

/// A radio's channel utilisation, graded.
pub fn utilisation(pct: Option<i64>, on: bool) -> String {
    graded_pct(pct, health::utilisation, on)
}

/// A radio's interference, graded.
pub fn interference(pct: Option<i64>, on: bool) -> String {
    graded_pct(pct, health::interference, on)
}

/// A radio's TX retry rate, rounded to a whole percent and graded on that.
pub fn retries(pct: Option<f64>, on: bool) -> String {
    graded_pct(pct.map(|p| p.round() as i64), health::retries, on)
}

/// A wired client's link speed, graded on absolute speed.
pub fn wired_speed(speed: Option<LinkSpeed>, on: bool) -> String {
    speed.map_or_else(dash, |s| {
        graded(format_link_speed(s), health::wired_link(s), on)
    })
}

/// A device uplink's speed, graded against what both ports allow.
pub fn uplink_speed(
    speed: Option<LinkSpeed>,
    max: Option<LinkSpeed>,
    full_duplex: Option<bool>,
    on: bool,
) -> String {
    speed.map_or_else(dash, |s| {
        graded(
            format_link_speed(s),
            health::uplink(s, max, full_duplex),
            on,
        )
    })
}

pub fn device_status(s: DeviceStatus, on: bool) -> String {
    let p = match s {
        DeviceStatus::Connected => Paint::Green,
        DeviceStatus::Disconnected => Paint::Red,
    };
    paint(s.as_str(), p, on)
}

pub fn wan_status(s: WanStatus, on: bool) -> String {
    let p = match s {
        WanStatus::Connected => Paint::Green,
        WanStatus::Disconnected => Paint::Red,
    };
    paint(s.as_str(), p, on)
}

/// Offline is dim, not red: recently offline clients are routine.
pub fn client_status(s: ClientStatus, on: bool) -> String {
    let p = match s {
        ClientStatus::Online => Paint::Green,
        ClientStatus::Offline => Paint::Dim,
    };
    paint(s.as_str(), p, on)
}

/// Down is dim, not red: unused ports are routine.
pub fn port_state(s: PortState, on: bool) -> String {
    let p = match s {
        PortState::Up => Paint::Green,
        PortState::Down => Paint::Dim,
        PortState::Disabled => Paint::Yellow,
    };
    paint(s.as_str(), p, on)
}

/// A `status` verdict: idle and unknown recede like offline clients.
pub fn verdict(v: Verdict, on: bool) -> String {
    let p = match v {
        Verdict::Ok => Paint::Green,
        Verdict::Warn => Paint::Yellow,
        Verdict::Error => Paint::Red,
        Verdict::Info => Paint::Blue,
        Verdict::Idle | Verdict::Unknown => Paint::Dim,
    };
    paint(v.as_str(), p, on)
}

/// A name, blue like file names in `eza`.
pub fn name(text: &str, on: bool) -> String {
    paint(text, Paint::Blue, on)
}

/// An application name: blue like other names, dim when it is a fallback
/// (`Unknown (…)`, `Unidentified`) so unnamed traffic recedes.
pub fn app_name(app: &AppRef, on: bool) -> String {
    paint(
        &app.name,
        if app.named { Paint::Blue } else { Paint::Dim },
        on,
    )
}

/// A device name in `topology`, bold so devices stand out from clients.
pub fn device_name(text: &str, on: bool) -> String {
    paint(text, Paint::BoldBlue, on)
}

/// The `config check` mark for `s`.
pub fn check_mark(s: Status, on: bool) -> String {
    let (mark, p) = match s {
        Status::Pass => ("✓", Paint::Green),
        Status::Warn => ("!", Paint::Yellow),
        Status::Fail => ("✗", Paint::Red),
        Status::Skip => ("–", Paint::Dim),
    };
    paint(mark, p, on)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paint_wraps_only_when_on() {
        assert_eq!(paint("up", Paint::Green, true), "\x1b[32mup\x1b[0m");
        assert_eq!(paint("up", Paint::Green, false), "up");
    }

    #[test]
    fn paint_replaces_control_characters_first() {
        assert_eq!(
            paint("a\x1b]0;x\x07\nb", Paint::Blue, true),
            "\x1b[34ma\u{fffd}]0;x\u{fffd}\u{fffd}b\x1b[0m"
        );
        assert_eq!(paint("a\u{9b}2Jb", Paint::Blue, false), "a\u{fffd}2Jb");
    }

    #[test]
    fn terminal_safe_keeps_only_whole_painted_spans() {
        let painted = format!(
            "{} and {}",
            paint("up", Paint::BoldRed, true),
            paint("x", Paint::Dim, true)
        );
        assert_eq!(terminal_safe(&painted), painted);
        assert_eq!(
            terminal_safe("\x1b[8mx\x1b[2J\x1b]8;;u\x07\t\x1b[3"),
            "\u{fffd}[8mx\u{fffd}[2J\u{fffd}]8;;u\u{fffd}\u{fffd}\u{fffd}[3"
        );
        // One of paint's own codes, but unbalanced or wrapping an escape.
        assert_eq!(terminal_safe("\x1b[31m192.0.2.1"), "\u{fffd}[31m192.0.2.1");
        assert_eq!(terminal_safe("\x1b[0mx"), "\u{fffd}[0mx");
        assert_eq!(
            terminal_safe("\x1b[31m\x1b[2J\x1b[0m"),
            "\u{fffd}[31m\u{fffd}[2J\u{fffd}[0m"
        );
        assert_eq!(terminal_safe("café ✓"), "café ✓");
    }

    #[test]
    fn radio_metrics_are_graded_percentages() {
        assert_eq!(utilisation(Some(71), true), "\x1b[33m71%\x1b[0m");
        assert_eq!(utilisation(Some(22), true), "\x1b[32m22%\x1b[0m");
        assert_eq!(interference(Some(38), true), "\x1b[31m38%\x1b[0m");
        assert_eq!(retries(Some(14.4), true), "\x1b[33m14%\x1b[0m");
        assert_eq!(
            retries(Some(9.5), true),
            "\x1b[33m10%\x1b[0m",
            "graded on the rounded value"
        );
        assert_eq!(utilisation(Some(71), false), "71%");
        assert_eq!(utilisation(None, true), "-");
        assert_eq!(interference(None, true), "-");
        assert_eq!(retries(None, true), "-");
    }

    #[test]
    fn verdicts_map_to_colours() {
        assert_eq!(verdict(Verdict::Ok, true), "\x1b[32mok\x1b[0m");
        assert_eq!(verdict(Verdict::Warn, true), "\x1b[33mwarn\x1b[0m");
        assert_eq!(verdict(Verdict::Error, true), "\x1b[31merror\x1b[0m");
        assert_eq!(verdict(Verdict::Info, true), "\x1b[34minfo\x1b[0m");
        assert_eq!(verdict(Verdict::Idle, true), "\x1b[2midle\x1b[0m");
        assert_eq!(verdict(Verdict::Unknown, true), "\x1b[2munknown\x1b[0m");
        assert_eq!(verdict(Verdict::Warn, false), "warn");
    }

    #[test]
    fn statuses_map_to_colours() {
        assert_eq!(
            device_status(DeviceStatus::Connected, true),
            "\x1b[32mconnected\x1b[0m"
        );
        assert_eq!(
            device_status(DeviceStatus::Disconnected, true),
            "\x1b[31mdisconnected\x1b[0m"
        );
        assert_eq!(
            wan_status(WanStatus::Connected, true),
            "\x1b[32mconnected\x1b[0m"
        );
        assert_eq!(
            wan_status(WanStatus::Disconnected, true),
            "\x1b[31mdisconnected\x1b[0m"
        );
        assert_eq!(
            client_status(ClientStatus::Online, true),
            "\x1b[32monline\x1b[0m"
        );
        assert_eq!(
            client_status(ClientStatus::Offline, true),
            "\x1b[2moffline\x1b[0m"
        );
        assert_eq!(port_state(PortState::Up, true), "\x1b[32mup\x1b[0m");
        assert_eq!(port_state(PortState::Down, true), "\x1b[2mdown\x1b[0m");
        assert_eq!(
            port_state(PortState::Disabled, true),
            "\x1b[33mdisabled\x1b[0m"
        );
    }

    #[test]
    fn check_marks_map_to_colours() {
        assert_eq!(check_mark(Status::Pass, true), "\x1b[32m✓\x1b[0m");
        assert_eq!(check_mark(Status::Warn, true), "\x1b[33m!\x1b[0m");
        assert_eq!(check_mark(Status::Fail, true), "\x1b[31m✗\x1b[0m");
        assert_eq!(check_mark(Status::Skip, true), "\x1b[2m–\x1b[0m");
    }

    #[test]
    fn names_are_plain_blue() {
        assert_eq!(name("US 8", true), "\x1b[34mUS 8\x1b[0m");
        assert_eq!(name("US 8", false), "US 8");
    }

    #[test]
    fn device_names_are_bold_blue() {
        assert_eq!(device_name("CGF-01", true), "\x1b[1;34mCGF-01\x1b[0m");
        assert_eq!(device_name("CGF-01", false), "CGF-01");
    }

    #[test]
    fn off_returns_plain_words() {
        assert_eq!(
            device_status(DeviceStatus::Disconnected, false),
            "disconnected"
        );
        assert_eq!(port_state(PortState::Disabled, false), "disabled");
        assert_eq!(check_mark(Status::Fail, false), "✗");
    }

    #[test]
    fn bold_red_sgr() {
        assert_eq!(paint("x", Paint::BoldRed, true), "\x1b[1;31mx\x1b[0m");
    }

    #[test]
    fn graded_maps_to_traffic_light() {
        use crate::render::health::Health;
        assert_eq!(graded("x", Health::Good, true), "\x1b[32mx\x1b[0m");
        assert_eq!(graded("x", Health::Fair, true), "\x1b[33mx\x1b[0m");
        assert_eq!(graded("x", Health::Poor, true), "\x1b[31mx\x1b[0m");
        assert_eq!(graded("x", Health::Poor, false), "x");
    }

    #[test]
    fn down_is_cyan_up_is_magenta() {
        assert_eq!(down("1 KB", true), "\x1b[36m1 KB\x1b[0m");
        assert_eq!(up("1 KB", true), "\x1b[35m1 KB\x1b[0m");
        assert_eq!(down("1 KB", false), "1 KB");
    }

    #[test]
    fn health_cells() {
        use crate::model::LinkSpeed;
        assert_eq!(experience(Some(98), true), "\x1b[32m98%\x1b[0m");
        assert_eq!(experience(Some(45), false), "45%");
        assert_eq!(experience(None, true), "-");
        assert_eq!(signal(Some(-69), true), "\x1b[33m-69 dBm\x1b[0m");
        assert_eq!(signal(None, true), "-");
        assert_eq!(
            wired_speed(Some(LinkSpeed::Fe), true),
            "\x1b[33m100 Mbps\x1b[0m"
        );
        assert_eq!(wired_speed(None, true), "-");
        assert_eq!(
            uplink_speed(
                Some(LinkSpeed::Gbe2p5),
                Some(LinkSpeed::Gbe2p5),
                Some(true),
                true
            ),
            "\x1b[32m2.5 GbE\x1b[0m"
        );
        assert_eq!(uplink_speed(None, Some(LinkSpeed::Gbe1), None, true), "-");
    }
}
