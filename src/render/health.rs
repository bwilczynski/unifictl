//! Green/yellow/red grading of link health. Thresholds only; `style` turns a
//! grade into colour.

use crate::model::LinkSpeed;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    Good,
    Fair,
    Poor,
}

/// UniFi experience score, 0–100.
pub fn experience(pct: i64) -> Health {
    match pct {
        80.. => Health::Good,
        50..=79 => Health::Fair,
        _ => Health::Poor,
    }
}

/// Wi-Fi signal in dBm.
pub fn signal(dbm: i64) -> Health {
    match dbm {
        -67.. => Health::Good,
        -75..=-68 => Health::Fair,
        _ => Health::Poor,
    }
}

/// WAN availability in percent, as displayed (two decimals).
pub fn availability(pct: f64) -> Health {
    if pct >= 99.9 {
        Health::Good
    } else if pct >= 99.0 {
        Health::Fair
    } else {
        Health::Poor
    }
}

/// WAN latency in milliseconds.
pub fn latency(ms: i64) -> Health {
    match ms {
        ..50 => Health::Good,
        50..=99 => Health::Fair,
        _ => Health::Poor,
    }
}

/// A wired link whose maximum is unknown: graded on absolute speed.
pub fn wired_link(speed: LinkSpeed) -> Health {
    match speed.mbps() {
        1000.. => Health::Good,
        100..=999 => Health::Fair,
        _ => Health::Poor,
    }
}

/// A device uplink: below what both ports allow (`max`) is fair, 10 Mbps or
/// half duplex is poor; without a known `max`, the absolute rule applies.
pub fn uplink(speed: LinkSpeed, max: Option<LinkSpeed>, full_duplex: Option<bool>) -> Health {
    if speed == LinkSpeed::E || full_duplex == Some(false) {
        return Health::Poor;
    }
    match max {
        Some(m) if speed.mbps() < m.mbps() => Health::Fair,
        Some(_) => Health::Good,
        None => wired_link(speed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::LinkSpeed;

    #[test]
    fn experience_boundaries() {
        assert_eq!(experience(100), Health::Good);
        assert_eq!(experience(80), Health::Good);
        assert_eq!(experience(79), Health::Fair);
        assert_eq!(experience(50), Health::Fair);
        assert_eq!(experience(49), Health::Poor);
        assert_eq!(experience(0), Health::Poor);
    }

    #[test]
    fn signal_boundaries() {
        assert_eq!(signal(-40), Health::Good);
        assert_eq!(signal(-67), Health::Good);
        assert_eq!(signal(-68), Health::Fair);
        assert_eq!(signal(-75), Health::Fair);
        assert_eq!(signal(-76), Health::Poor);
    }

    #[test]
    fn availability_boundaries() {
        assert_eq!(availability(100.0), Health::Good);
        assert_eq!(availability(99.9), Health::Good);
        assert_eq!(availability(99.89), Health::Fair);
        assert_eq!(availability(99.0), Health::Fair);
        assert_eq!(availability(98.99), Health::Poor);
    }

    #[test]
    fn latency_boundaries() {
        assert_eq!(latency(11), Health::Good);
        assert_eq!(latency(49), Health::Good);
        assert_eq!(latency(50), Health::Fair);
        assert_eq!(latency(99), Health::Fair);
        assert_eq!(latency(100), Health::Poor);
    }

    #[test]
    fn wired_link_is_absolute() {
        assert_eq!(wired_link(LinkSpeed::Gbe10), Health::Good);
        assert_eq!(wired_link(LinkSpeed::Gbe1), Health::Good);
        assert_eq!(wired_link(LinkSpeed::Fe), Health::Fair);
        assert_eq!(wired_link(LinkSpeed::E), Health::Poor);
    }

    #[test]
    fn uplink_is_relative_to_max() {
        use LinkSpeed::*;
        assert_eq!(uplink(Gbe1, Some(Gbe1), Some(true)), Health::Good);
        assert_eq!(uplink(Gbe2p5, Some(Gbe2p5), Some(true)), Health::Good);
        assert_eq!(uplink(Fe, Some(Gbe1), Some(true)), Health::Fair);
        assert_eq!(uplink(Gbe1, Some(Gbe10), None), Health::Fair);
        assert_eq!(
            uplink(E, Some(E), Some(true)),
            Health::Poor,
            "10 Mbps is always poor"
        );
        assert_eq!(
            uplink(Gbe1, Some(Gbe1), Some(false)),
            Health::Poor,
            "half duplex"
        );
    }

    #[test]
    fn uplink_without_max_uses_absolute() {
        use LinkSpeed::*;
        assert_eq!(uplink(Gbe1, None, Some(true)), Health::Good);
        assert_eq!(uplink(Fe, None, None), Health::Fair);
    }
}
