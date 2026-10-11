//! ANSI colour for status cells and log messages. Only the basic SGR colours are used, so the
//! terminal's own palette (e.g. a Catppuccin theme) decides the actual shade.

use crate::model::{AppRef, ClientStatus, DeviceStatus, PortState, WanStatus};
use crate::setup::check::Status;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Paint {
    Green,
    Red,
    BoldRed,
    Yellow,
    Blue,
    BoldBlue,
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
}
