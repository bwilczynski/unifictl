//! ANSI colour for status cells, health metrics, traffic and log messages. Only the basic SGR colours are used, so the
//! terminal's own palette (e.g. a Catppuccin theme) decides the actual shade.

use super::format::format_link_speed;
use super::health::{self, Health};
use crate::model::{AppRef, ClientStatus, DeviceStatus, LinkSpeed, PortState, WanStatus};
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

/// `text` wrapped in `p`'s escape codes when `on`, unchanged otherwise.
pub fn paint(text: &str, p: Paint, on: bool) -> String {
    if on {
        format!("\x1b[{}m{text}\x1b[0m", p.sgr())
    } else {
        text.to_string()
    }
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
