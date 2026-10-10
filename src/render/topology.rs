//! topology.tmpl + hlctl topology.go `buildTopologyTree`.

use std::collections::{HashMap, HashSet};

use super::format::format_link_speed;
use super::style;
use crate::error::{Error, Result};
use crate::model::{DeviceType, NetworkTopology, TopologyEdge, TopologyNode};

struct Child<'a> {
    id: &'a str,
    edge: String,
}

/// Tree rooted at the gateway. Wireless edges are drawn only when
/// `include_wireless`; JSON output is unaffected (handled by the caller).
pub fn tree(topo: &NetworkTopology, include_wireless: bool, color: bool) -> Result<String> {
    let mut display: HashMap<&str, String> = HashMap::new();
    let mut gateway: Option<&str> = None;
    for node in &topo.nodes {
        match node {
            TopologyNode::Device(d) => {
                let mut s = format!("{} ({})", style::device_name(&d.name, color), d.device_type);
                if let Some(n) = d.num_clients.filter(|&n| n > 0) {
                    s.push_str(&format!(" [{n} clients]"));
                }
                display.insert(&d.id, s);
                if d.device_type == DeviceType::Gateway {
                    gateway = Some(&d.id);
                }
            }
            TopologyNode::Client(c) => {
                display.insert(
                    &c.id,
                    format!(
                        "{} (client, {}, {})",
                        style::name(&c.name, color),
                        c.connection_type,
                        style::client_status(c.status, color)
                    ),
                );
            }
        }
    }
    let gateway =
        gateway.ok_or_else(|| Error::Other("no gateway node found in topology".into()))?;

    let mut children: HashMap<&str, Vec<Child>> = HashMap::new();
    for edge in &topo.edges {
        match edge {
            TopologyEdge::Wired(e) => {
                let label = match (e.port, e.link_speed) {
                    (Some(p), Some(ls)) => format!("[port {p}, {}]", format_link_speed(ls)),
                    (Some(p), None) => format!("[port {p}]"),
                    _ => String::new(),
                };
                children.entry(&e.target.id).or_default().push(Child {
                    id: e.source.id(),
                    edge: label,
                });
            }
            TopologyEdge::Wireless(e) if include_wireless => {
                let label = match e.signal_strength {
                    Some(s) => format!("[{}, {s} dBm]", e.ssid),
                    None => format!("[{}]", e.ssid),
                };
                children.entry(&e.target.id).or_default().push(Child {
                    id: &e.source.id,
                    edge: label,
                });
            }
            TopologyEdge::Wireless(_) => {}
        }
    }

    let mut out = display.get(gateway).cloned().unwrap_or_default();
    out.push('\n');
    let mut visited = HashSet::from([gateway]);
    write_subtree(&mut out, gateway, "", &children, &display, &mut visited);
    Ok(out)
}

/// Depth-first; a node already drawn is listed again but not descended into,
/// so an uplink loop cannot recurse forever.
fn write_subtree<'a>(
    out: &mut String,
    parent: &str,
    prefix: &str,
    children: &HashMap<&'a str, Vec<Child<'a>>>,
    display: &HashMap<&str, String>,
    visited: &mut HashSet<&'a str>,
) {
    let Some(kids) = children.get(parent) else {
        return;
    };
    for (i, kid) in kids.iter().enumerate() {
        let last = i + 1 == kids.len();
        out.push_str(prefix);
        out.push_str(if last { "└── " } else { "├── " });
        out.push_str(display.get(kid.id).map(String::as_str).unwrap_or(""));
        if !kid.edge.is_empty() {
            out.push(' ');
            out.push_str(&kid.edge);
        }
        out.push('\n');
        if visited.insert(kid.id) {
            let child_prefix = format!("{prefix}{}", if last { "    " } else { "│   " });
            write_subtree(out, kid.id, &child_prefix, children, display, visited);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::Service;
    use crate::unifi::fixtures::FixtureApi;
    use crate::unifi::types::{Device, Uplink};

    fn topo(include_clients: bool) -> NetworkTopology {
        Service::new(
            Box::new(FixtureApi::load(&FixtureApi::default_dir()).unwrap()),
            30,
        )
        .get_topology(include_clients)
        .unwrap()
    }

    #[test]
    fn devices_tree_rooted_at_gateway() {
        let out = tree(&topo(false), false, false).unwrap();
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "CGF-01 (gateway)");
        assert_eq!(lines.len(), 7, "gateway + 6 devices");
        assert!(out.ends_with('\n'));
        assert!(
            lines
                .iter()
                .any(|l| l.contains("UAP-01 (accessPoint) [6 clients] [port 5, 100 Mbps]"))
        );
        assert!(lines[1..].iter().all(|l| l.contains("── ")));
    }

    #[test]
    fn wireless_edges_only_with_flag() {
        let t = topo(true);
        let wired_only = tree(&t, false, false).unwrap();
        assert!(wired_only.contains("nas-1 (client, wired, online) [port 3, 1 GbE]"));
        assert!(!wired_only.contains("MacBook Pro"));
        let with_wireless = tree(&t, true, false).unwrap();
        assert!(
            with_wireless.contains("MacBook Pro (client, wireless, online) [homelab, -69 dBm]")
        );
        assert!(with_wireless.contains("Kindle Paperwhite (client, wireless, offline) [homelab]"));
    }

    #[test]
    fn topology_without_gateway_is_error() {
        let t = NetworkTopology {
            nodes: vec![],
            edges: vec![],
        };
        assert_eq!(
            tree(&t, false, false).unwrap_err().to_string(),
            "no gateway node found in topology"
        );
    }

    #[test]
    fn topology_tree_survives_uplink_cycle() {
        let dev = |name: &str, mac: &str, kind: &str, up: &str| Device {
            name: name.into(),
            mac: mac.into(),
            kind: kind.into(),
            state: 1,
            uplink: (!up.is_empty()).then(|| Uplink {
                uplink_mac: up.into(),
                ..Default::default()
            }),
            ..Default::default()
        };
        let api = FixtureApi {
            devices: vec![
                dev("GW", "aa:00:00:00:00:01", "udm", ""),
                dev("A", "aa:00:00:00:00:02", "usw", "aa:00:00:00:00:03"),
                dev("B", "aa:00:00:00:00:03", "usw", "aa:00:00:00:00:02"),
                dev("C", "aa:00:00:00:00:04", "usw", "aa:00:00:00:00:01"),
            ],
            ..Default::default()
        };
        let t = Service::new(Box::new(api), 30).get_topology(false).unwrap();
        let out = tree(&t, false, false).unwrap();
        assert_eq!(out, "GW (gateway)\n└── C (switch)\n");
        // A and B only reach each other; render them as their own subtree to prove termination.
        let mut kids: HashMap<&str, Vec<Child>> = HashMap::new();
        kids.insert(
            "a",
            vec![Child {
                id: "b",
                edge: String::new(),
            }],
        );
        kids.insert(
            "b",
            vec![Child {
                id: "a",
                edge: String::new(),
            }],
        );
        let display = HashMap::from([("a", "A".to_string()), ("b", "B".to_string())]);
        let mut out = String::new();
        let mut visited = HashSet::from(["a"]);
        write_subtree(&mut out, "a", "", &kids, &display, &mut visited);
        assert_eq!(out, "└── B\n    └── A\n");
    }
}
