use std::process::{Command, Output};

fn unifictl(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_unifictl"))
        .args(args)
        .env_remove("UNIFICTL_CONFIG")
        .output()
        .expect("run unifictl")
}

#[test]
fn version_flag_prints_crate_version() {
    let out = unifictl(&["--version"]);
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        format!("unifictl {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn unknown_command_is_usage_error() {
    let out = unifictl(&["bogus"]);
    assert_eq!(out.status.code(), Some(2));
}

#[cfg(feature = "fixtures")]
mod with_fixtures {
    use std::path::PathBuf;
    use std::process::{Command, Output};

    fn fixtures() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    fn run_in(dir: &std::path::Path, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_unifictl"))
            .arg("--fixtures")
            .arg(dir)
            .args(args)
            .output()
            .unwrap()
    }

    fn run(args: &[&str]) -> Output {
        run_in(&fixtures(), args)
    }

    fn stdout(o: &Output) -> String {
        String::from_utf8(o.stdout.clone()).unwrap()
    }

    /// Drops `ESC [ … m` sequences.
    fn strip_ansi(s: &str) -> String {
        let mut out = String::new();
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                for c in chars.by_ref() {
                    if c == 'm' {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    #[test]
    fn color_always_paints_without_moving_columns() {
        for args in [
            &["devices", "list"][..],
            &["devices", "get", "us-8", "--all-ports"],
            &["clients", "list"],
            &["clients", "get", "nas-1-0002"],
            &["clients", "traffic"],
            &["clients", "traffic", "macbook-pro-0001"],
            &["clients", "traffic", "--by-app"],
            &["ports", "list", "--all-ports"],
            &["wans", "list"],
            &["wans", "get", "internet-2"],
            &["wans", "get", "internet-1"],
            &["topology", "--include-clients"],
            &["devices", "get", "cgf-01"],
            &["vlans", "list"],
            &["vlans", "get", "lan-mgmt"],
            &["ssids", "list"],
            &["ssids", "get", "home-wifi"],
            &["devices", "list", "--wide"],
            &["clients", "list", "--wide"],
            &["vlans", "list", "--wide"],
            &["ssids", "list", "--wide"],
            &["wans", "list", "--wide"],
            &["ports", "list", "--wide"],
            &["clients", "get", "macbook-pro-0001"],
        ] {
            let plain = stdout(&run(args));
            let mut colored_args = vec!["--color", "always"];
            colored_args.extend(args);
            let o = run(&colored_args);
            assert!(o.status.success(), "{args:?}");
            let colored = stdout(&o);
            assert!(colored.contains("\x1b["), "{args:?}: not coloured");
            assert_eq!(strip_ansi(&colored), plain, "{args:?}: layout changed");
        }
    }

    #[test]
    fn wide_adds_columns_after_the_default_ones() {
        for cmd in ["devices", "clients", "vlans", "ssids", "wans"] {
            let plain = stdout(&run(&[cmd, "list"]));
            let wide = stdout(&run(&[cmd, "list", "--wide"]));
            let head = |s: &str| {
                s.lines()
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .map(String::from)
                    .collect::<Vec<_>>()
            };
            let (p, w) = (head(&plain), head(&wide));
            assert!(w.len() > p.len(), "{cmd}");
            assert_eq!(w[..p.len()], p[..], "{cmd}: default columns come first");
            assert_eq!(plain.lines().count(), wide.lines().count(), "{cmd}");
        }
    }

    #[test]
    fn mac_only_with_wide() {
        for cmd in ["devices", "clients"] {
            let header = |args: &[&str]| stdout(&run(args)).lines().next().unwrap().to_string();
            assert!(!header(&[cmd, "list"]).contains("MAC"), "{cmd}");
            assert!(header(&[cmd, "list", "--wide"]).contains("MAC"), "{cmd}");
        }
    }

    #[test]
    fn wide_does_not_change_json_or_ids() {
        for cmd in ["devices", "clients", "vlans", "ssids", "wans"] {
            for fmt in ["json", "ids"] {
                let a = stdout(&run(&["-o", fmt, cmd, "list"]));
                let b = stdout(&run(&["-o", fmt, cmd, "list", "--wide"]));
                assert_eq!(a, b, "{cmd} -o {fmt}");
            }
        }
    }

    #[test]
    fn list_items_equal_get_json() {
        for cmd in ["clients", "vlans", "wans"] {
            let list: serde_json::Value =
                serde_json::from_slice(&run(&["-o", "json", cmd, "list"]).stdout).unwrap();
            for item in list["items"].as_array().unwrap() {
                let id = item["id"].as_str().unwrap();
                let mut got: serde_json::Value =
                    serde_json::from_slice(&run(&["-o", "json", cmd, "get", id]).stdout).unwrap();
                // `wans get` adds the outage history on top of the list item.
                if cmd == "wans" {
                    got.as_object_mut().unwrap().remove("outages");
                }
                assert_eq!(&got, item, "{cmd} {id}");
            }
        }
    }

    #[test]
    fn health_colours() {
        let devices = stdout(&run(&["--color", "always", "devices", "list", "--wide"]));
        assert!(devices.contains("\x1b[32m98%\x1b[0m"));
        assert!(devices.contains("\x1b[33m100 Mbps\x1b[0m"));
        assert!(devices.contains("\x1b[31m1 GbE\x1b[0m"));
        let clients = stdout(&run(&["--color", "always", "clients", "list", "--wide"]));
        assert!(clients.contains("\x1b[33m-69 dBm\x1b[0m"));
        assert!(clients.contains("\x1b[31m-80 dBm\x1b[0m"));
        assert!(clients.contains("\x1b[36m512.0 KB/s\x1b[0m"));
        assert!(clients.contains("\x1b[35m10.0 KB/s\x1b[0m"));
        let traffic = stdout(&run(&["--color", "always", "clients", "traffic"]));
        assert!(traffic.contains("\x1b[36m") && traffic.contains("\x1b[35m"));
        let json = stdout(&run(&[
            "--color", "always", "-o", "json", "clients", "list", "--wide",
        ]));
        assert!(!json.contains('\x1b'));
    }

    #[test]
    fn device_json_carries_health_fields() {
        let v: serde_json::Value =
            serde_json::from_slice(&run(&["-o", "json", "devices", "list"]).stdout).unwrap();
        let ap = v["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["id"] == "uap-01")
            .unwrap();
        assert_eq!(ap["experience"], 98);
        assert_eq!(ap["uplink"]["linkSpeed"], "fe");
        assert_eq!(ap["uplink"]["maxLinkSpeed"], "gbe1");
        assert_eq!(ap["uplink"]["fullDuplex"], true);
        assert_eq!(ap["model"], "U7LT");
    }

    #[test]
    fn colours_follow_status() {
        let wans = stdout(&run(&["--color", "always", "wans", "list"]));
        assert!(wans.contains("\x1b[32mconnected\x1b[0m"));
        assert!(wans.contains("\x1b[31mdisconnected\x1b[0m"));
        let clients = stdout(&run(&["--color", "always", "clients", "list"]));
        assert!(clients.contains("\x1b[32monline\x1b[0m"));
        assert!(clients.contains("\x1b[2moffline\x1b[0m"));
        let ports = stdout(&run(&["--color", "always", "ports", "list", "--all-ports"]));
        assert!(ports.contains("\x1b[32mup\x1b[0m"));
        assert!(ports.contains("\x1b[2mdown\x1b[0m"));
        let topo = stdout(&run(&[
            "--color",
            "always",
            "topology",
            "--include-clients",
        ]));
        assert!(topo.contains("(client, wired, \x1b[2moffline\x1b[0m)"));
    }

    #[test]
    fn names_are_blue_and_ids_plain() {
        let blue = |args: &[&str], name: &str| {
            let mut a = vec!["--color", "always"];
            a.extend(args);
            let out = stdout(&run(&a));
            assert!(
                out.contains(&format!("\x1b[34m{name}\x1b[0m")),
                "{args:?}: {name} not blue in\n{out}"
            );
            out
        };
        let devices = blue(&["devices", "list"], "US 8");
        assert!(
            devices.lines().nth(1).unwrap().starts_with("us-8 "),
            "ID plain"
        );
        blue(&["clients", "list"], "nas-1");
        blue(&["vlans", "list"], "LAN-MGMT");
        blue(&["ssids", "list"], "home-wifi");
        blue(&["wans", "list"], "Internet 1");
        blue(&["ports", "list"], "CGF-01");
        let topo = stdout(&run(&[
            "--color",
            "always",
            "topology",
            "--include-clients",
        ]));
        assert!(
            topo.starts_with("\x1b[1;34mCGF-01\x1b[0m (gateway)"),
            "{topo}"
        );
        assert!(
            topo.contains("\x1b[1;34mUS 8 60W\x1b[0m (switch)"),
            "{topo}"
        );
        assert!(topo.contains("\x1b[34mnas-1\x1b[0m (client"), "{topo}");
        blue(&["devices", "get", "us-8"], "US 8");
        blue(&["devices", "get", "cgf-01"], "Internet 1");
        blue(&["clients", "get", "nas-1-0002"], "nas-1");
        blue(&["vlans", "get", "lan-mgmt"], "LAN-MGMT");
        blue(&["wans", "get", "internet-1"], "Internet 1");
        let ssid = blue(&["ssids", "get", "home-wifi"], "UAP-01");
        assert!(ssid.contains("\x1b[34mhome-wifi\x1b[0m"));
        let client = stdout(&run(&["--color", "always", "clients", "get", "nas-1-0002"]));
        assert!(
            client
                .lines()
                .any(|l| l.starts_with("SWITCH") && l.ends_with(" US 8 60W")),
            "names inside other fields stay plain"
        );
    }

    #[test]
    fn piped_json_and_never_are_plain() {
        assert!(!stdout(&run(&["devices", "list"])).contains('\x1b'));
        assert!(!stdout(&run(&["--color", "never", "devices", "list"])).contains('\x1b'));
        let o = run(&["--color", "always", "-o", "json", "devices", "list"]);
        assert!(!stdout(&o).contains('\x1b'));
        serde_json::from_slice::<serde_json::Value>(&o.stdout).expect("valid JSON");
    }

    #[test]
    fn ids_output_is_one_id_per_line() {
        let o = run(&["-o", "ids", "devices", "list"]);
        assert!(o.status.success());
        let out = stdout(&o);
        assert_eq!(out.lines().count(), 7);
        assert_eq!(out.lines().next(), Some("us-8"));
        assert!(out.ends_with('\n'));
        for cmd in ["clients", "vlans", "ssids", "wans"] {
            let o = run(&["-o", "ids", cmd, "list"]);
            assert!(o.status.success(), "{cmd}");
            let out = stdout(&o);
            assert!(!out.is_empty(), "{cmd}");
            assert!(
                out.lines().all(|l| !l.contains(' ') && l != "ID"),
                "{cmd}: {out}"
            );
        }
    }

    #[test]
    fn ids_honour_list_filters() {
        let out = stdout(&run(&[
            "clients", "list", "--status", "offline", "-o", "ids",
        ]));
        assert_eq!(out, "kindle-paperwhite-bb04\nhost-02-bb05\n");
    }

    #[test]
    fn ids_feed_back_into_get() {
        for cmd in ["devices", "clients", "vlans", "ssids", "wans"] {
            let ids = stdout(&run(&["-o", "ids", cmd, "list"]));
            assert!(!ids.is_empty(), "{cmd}: no IDs");
            for id in ids.lines() {
                // Listed but not gettable, as in the Go implementation.
                if id == "sonos-one-sl-l-bb03" {
                    continue;
                }
                let o = run(&[cmd, "get", id]);
                assert!(
                    o.status.success(),
                    "{cmd} get {id}: {}",
                    String::from_utf8_lossy(&o.stderr)
                );
            }
        }
    }

    #[test]
    fn ids_rejected_outside_list_commands() {
        for args in [
            &["ports", "list"][..],
            &["devices", "get", "us-8"],
            &["devices", "get", "nope"],
            &["topology"],
        ] {
            let mut a = vec!["-o", "ids"];
            a.extend(args);
            let o = run(&a);
            assert_eq!(o.status.code(), Some(1), "{args:?}");
            assert_eq!(
                String::from_utf8_lossy(&o.stderr),
                "Error: -o ids is only supported by devices, clients, vlans, ssids and wans list, and clients traffic\n",
                "{args:?}"
            );
            assert!(o.stdout.is_empty(), "{args:?}");
        }
    }

    #[test]
    fn color_flag_works_after_subcommand() {
        let o = run(&["wans", "list", "--color", "always"]);
        assert!(stdout(&o).contains("\x1b[31mdisconnected"));
    }

    #[test]
    fn devices_list_table_and_json() {
        let o = run(&["devices", "list"]);
        assert!(o.status.success());
        assert!(stdout(&o).starts_with("ID"));
        let o = run(&["-o", "json", "devices", "list"]);
        let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
        assert_eq!(v["items"].as_array().unwrap().len(), 7);
        assert!(v["items"][0].get("uri").is_none());
    }

    #[test]
    fn output_flag_works_after_subcommand() {
        let o = run(&["devices", "get", "us-8", "-o", "json"]);
        let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
        assert_eq!(v["type"], "switch");
        assert_eq!(
            v["ports"].as_array().unwrap().len(),
            8,
            "JSON always has every port"
        );
    }

    #[test]
    fn not_found_exits_1_with_message() {
        let o = run(&["devices", "get", "nope"]);
        assert_eq!(o.status.code(), Some(1));
        assert_eq!(
            String::from_utf8_lossy(&o.stderr),
            "Error: Network device not found: nope\n"
        );
    }

    #[test]
    fn last_seen_is_local_time() {
        let last_seen = |tz: &str| -> String {
            let o = Command::new(env!("CARGO_BIN_EXE_unifictl"))
                .arg("--fixtures")
                .arg(fixtures())
                .args(["clients", "get", "macbook-pro-0001"])
                .env("TZ", tz)
                .output()
                .unwrap();
            stdout(&o)
                .lines()
                .find(|l| l.starts_with("LAST SEEN"))
                .unwrap()
                .to_string()
        };
        assert!(last_seen("UTC").ends_with("2026-10-11 10:00"));
        assert!(last_seen("Asia/Tokyo").ends_with("2026-10-11 19:00"));
    }

    #[test]
    fn ports_default_to_up_unless_all_ports() {
        let count = |args: &[&str]| -> usize {
            let mut a = vec!["-o", "json", "ports", "list", "--device", "us-8-60w"];
            a.extend_from_slice(args);
            let v: serde_json::Value = serde_json::from_slice(&run(&a).stdout).unwrap();
            v["items"].as_array().unwrap().len()
        };
        assert_eq!(count(&[]), 7);
        assert_eq!(count(&["--all-ports"]), 12);
        assert_eq!(count(&["--state", "down"]), 5);
    }

    #[test]
    fn closed_stdout_is_not_a_crash() {
        use std::process::Stdio;
        let mut child = Command::new(env!("CARGO_BIN_EXE_unifictl"))
            .arg("--fixtures")
            .arg(fixtures())
            .args(["-o", "json", "topology", "--include-clients"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        drop(child.stdout.take()); // reader goes away before the write
        let out = child.wait_with_output().unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            out.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn all_ports_conflicts_with_state() {
        let o = run(&["ports", "list", "--all-ports", "--state", "up"]);
        assert_eq!(o.status.code(), Some(2));
    }

    #[test]
    fn invalid_enum_value_is_usage_error() {
        assert_eq!(
            run(&["clients", "list", "--status", "away"]).status.code(),
            Some(2)
        );
    }

    #[test]
    fn topology_wireless_implies_clients() {
        let out = stdout(&run(&["topology", "--include-wireless"]));
        assert!(out.contains("MacBook Pro (client, wireless, online)"));
        let v: serde_json::Value =
            serde_json::from_slice(&run(&["-o", "json", "topology"]).stdout).unwrap();
        assert_eq!(v["nodes"].as_array().unwrap().len(), 7);
    }

    #[test]
    fn every_command_runs() {
        for args in [
            &["clients", "list"][..],
            &["clients", "list", "--status", "offline"],
            &["clients", "get", "nas-1-0002"],
            &["vlans", "list"],
            &["vlans", "get", "lan-iot"],
            &["ssids", "list"],
            &["ssids", "get", "home-wifi"],
            &["wans", "list"],
            &["wans", "get", "internet-1"],
            &["ports", "list", "--wide", "--vlan", "10"],
            &["devices", "get", "cgf-01", "--all-ports"],
        ] {
            let o = run(args);
            assert!(
                o.status.success(),
                "{args:?}: {}",
                String::from_utf8_lossy(&o.stderr)
            );
        }
    }

    #[test]
    fn empty_controller_lists_and_topology() {
        let dir = tempfile::tempdir().unwrap();
        for f in [
            "devices",
            "clients",
            "v2-active",
            "v2-history",
            "wlanconf",
            "networkconf",
        ] {
            std::fs::write(
                dir.path().join(format!("unifi-{f}.json")),
                r#"{"data": []}"#,
            )
            .unwrap();
        }
        let o = run_in(dir.path(), &["devices", "list"]);
        assert_eq!(stdout(&o).lines().count(), 1);
        let o = run_in(dir.path(), &["-o", "json", "clients", "list"]);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&o.stdout).unwrap(),
            serde_json::json!({"items": []})
        );
        let o = run_in(dir.path(), &["-o", "ids", "devices", "list"]);
        assert!(o.status.success());
        assert!(o.stdout.is_empty());
        let o = run_in(dir.path(), &["topology"]);
        assert_eq!(o.status.code(), Some(1));
        assert_eq!(
            String::from_utf8_lossy(&o.stderr),
            "Error: no gateway node found in topology\n"
        );
    }

    const ALL_LINES: [&str; 10] = [
        "Oct 08 12:00:00 internet-and-wan: Internet connection WAN1 ExampleISP on port 5 is down.",
        "Oct 08 12:02:00 internet-and-wan: Internet connection WAN1 ExampleISP on port 5 is restored.",
        "Oct 09 12:00:00 security: A network intrusion attempt from 192.168.10.67 to 198.51.100.7 has been detected.",
        "Oct 10 12:00:00 security: MacBook Pro was blocked from accessing 192.168.100.5 by the Block IoT to LAN Firewall Policy.",
        "Oct 11 06:00:00 audit: Network Application made a change to ? in UAP-02 ? settings. Source IP: ?",
        "Oct 11 10:00:00 unifi-devices: UAP-02 moved to channel 11 from 1.",
        "Oct 11 11:00:00 client-devices: MacBook Pro roamed from UAP-02 to UAP-01. Connection Info: Ch. 36 (5 GHz, 80 MHz), -60 dBm.",
        "Oct 11 11:30:00 client-devices: nas-1 connected to LAN-SRV on US 8 60W Port 3. IP: 192.168.100.5",
        "Oct 11 11:50:00 client-devices: Guest Phone disconnected from home-wifi-iot. Last Connected To: UAP-01.",
        "Oct 11 11:50:00 client-devices: MacBook Pro connected to home-wifi on UAP-01.",
    ];

    fn logs_cmd(args: &[&str]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_unifictl"));
        cmd.arg("--fixtures")
            .arg(fixtures())
            .arg("logs")
            .args(args)
            .env("TZ", "UTC")
            .env("UNIFICTL_NOW", "2026-10-11T12:00:00Z")
            .env_remove("NO_COLOR");
        cmd
    }

    fn logs(args: &[&str]) -> Output {
        logs_cmd(args).output().unwrap()
    }

    /// The fixture lines with these 1-based indexes, joined as output.
    fn lines(indexes: &[usize]) -> String {
        indexes
            .iter()
            .map(|i| format!("{}\n", ALL_LINES[i - 1]))
            .collect()
    }

    fn ok(o: &Output) -> String {
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        stdout(o)
    }

    #[test]
    fn logs_default_prints_everything_oldest_first() {
        assert_eq!(ok(&logs(&[])), lines(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]));
    }

    #[test]
    fn logs_lines_and_reverse() {
        assert_eq!(ok(&logs(&["-n", "2"])), lines(&[9, 10]));
        assert_eq!(ok(&logs(&["-r", "-n", "3"])), lines(&[10, 9, 8]));
    }

    #[test]
    fn logs_priority_and_category() {
        assert_eq!(ok(&logs(&["-p", "high"])), lines(&[1, 3]));
        assert_eq!(ok(&logs(&["--category", "security"])), lines(&[3, 4]));
        assert_eq!(
            ok(&logs(&["--category", "security,audit"])),
            lines(&[3, 4, 5])
        );
    }

    #[test]
    fn logs_device_and_client() {
        assert_eq!(ok(&logs(&["--device", "uap-02"])), lines(&[5, 6, 7]));
        assert_eq!(
            ok(&logs(&[
                "--client",
                "macbook-pro-0001",
                "--device",
                "uap-01"
            ])),
            lines(&[7, 10])
        );
        assert_eq!(
            ok(&logs(&["--device", "uap-01", "--device", "uap-02"])),
            lines(&[5, 6, 7, 9, 10])
        );
    }

    #[test]
    fn logs_since_relative_hyphen_value() {
        assert_eq!(ok(&logs(&["-S", "-1h"])), lines(&[7, 8, 9, 10]));
        assert_eq!(ok(&logs(&["--since=-1h"])), lines(&[7, 8, 9, 10]));
    }

    #[test]
    fn logs_absolute_window() {
        assert_eq!(
            ok(&logs(&["-S", "2026-10-11", "-U", "2026-10-11 10:00"])),
            lines(&[5, 6])
        );
        assert_eq!(ok(&logs(&["-U", "2026-10-09"])), lines(&[1, 2]));
    }

    #[test]
    fn logs_json_lines() {
        assert_eq!(
            ok(&logs(&[
                "-o",
                "json",
                "--client",
                "macbook-pro-0001",
                "-n",
                "1"
            ])),
            concat!(
                r#"{"id":"65f00000000000000000000a","time":"2026-10-11T11:50:00.000+00:00","severity":"low","category":"clientDevices","subcategory":"monitoringWifi","event":"clientConnected","title":"WiFi Client Connected","message":"MacBook Pro connected to home-wifi on UAP-01.","devices":[{"kind":"device","id":"uap-01","name":"UAP-01"}],"clients":[{"kind":"client","id":"macbook-pro-0001","name":"MacBook Pro"}],"details":{"wlan":"home-wifi"}}"#,
                "\n"
            )
        );
        let all = ok(&logs(&["-o", "json"]));
        assert_eq!(all.lines().count(), 10);
        assert!(!all.contains('\x1b'));
    }

    #[test]
    fn logs_color_always_paints_messages() {
        let out = ok(&logs(&["-p", "high", "--color", "always"]));
        assert_eq!(
            out,
            format!(
                "Oct 08 12:00:00 internet-and-wan: \x1b[1;31m{}\x1b[0m\nOct 09 12:00:00 security: \x1b[31m{}\x1b[0m\n",
                "Internet connection WAN1 ExampleISP on port 5 is down.",
                "A network intrusion attempt from 192.168.10.67 to 198.51.100.7 has been detected."
            )
        );
        assert_eq!(strip_ansi(&out), lines(&[1, 3]));
    }

    #[test]
    fn logs_rejected_combinations() {
        assert_eq!(logs(&["-f", "-U", "now"]).status.code(), Some(2));
        assert_eq!(logs(&["-f", "-r"]).status.code(), Some(2));
        let o = logs(&["-o", "ids"]);
        assert_eq!(o.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&o.stderr).contains("-o ids is only supported by"));
    }

    #[test]
    fn logs_errors() {
        for (args, want) in [
            (
                &["--device", "nope"][..],
                "Error: Network device not found: nope\n",
            ),
            (
                &["--client", "nope"],
                "Error: Network client not found: nope\n",
            ),
            (
                &["-S", "2026-10-11", "-U", "2026-10-10"],
                "Error: --since is after --until\n",
            ),
        ] {
            let o = logs(args);
            assert_eq!(o.status.code(), Some(1), "{args:?}");
            assert_eq!(String::from_utf8_lossy(&o.stderr), want, "{args:?}");
        }
        let o = logs(&["-S", "soon"]);
        assert_eq!(o.status.code(), Some(1));
        assert!(
            String::from_utf8_lossy(&o.stderr)
                .starts_with("Error: invalid time \"soon\": use now, today")
        );
    }

    #[test]
    fn logs_into_closed_pipe_is_quiet() {
        use std::process::Stdio;
        let mut child = logs_cmd(&[])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        drop(child.stdout.take());
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "{:?}", out.status);
        assert_eq!(String::from_utf8_lossy(&out.stderr), "");
    }

    fn traffic(args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_unifictl"))
            .arg("--fixtures")
            .arg(fixtures())
            .args(["clients", "traffic"])
            .args(args)
            .env("TZ", "UTC")
            .env("UNIFICTL_NOW", "2026-10-11T12:00:00Z")
            .output()
            .unwrap()
    }

    /// Rows of cells, split on runs of 2+ spaces so padding does not matter.
    fn cells(text: &str) -> Vec<Vec<String>> {
        text.lines()
            .map(|l| {
                l.split("  ")
                    .map(str::trim)
                    .filter(|c| !c.is_empty())
                    .map(String::from)
                    .collect()
            })
            .collect()
    }

    fn ok_out(o: &Output) -> String {
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        stdout(o)
    }

    #[test]
    fn traffic_clients_view() {
        let rows = cells(&ok_out(&traffic(&[])));
        assert_eq!(rows[0], ["ID", "NAME", "DOWN", "UP", "TOTAL", "TOP APP"]);
        assert_eq!(
            rows[1],
            [
                "nas-1-0002",
                "nas-1",
                "501.0 MB",
                "3.0 GB",
                "3.5 GB",
                "Example Web"
            ]
        );
        assert_eq!(rows.len(), 4);
    }

    #[test]
    fn traffic_one_client_and_by_app() {
        let rows = cells(&ok_out(&traffic(&["macbook-pro-0001"])));
        assert_eq!(
            rows[0],
            ["APP", "CATEGORY", "DOWN", "UP", "TOTAL", "ACTIVE"]
        );
        assert_eq!(rows[1][0], "Example Video");
        let rows = cells(&ok_out(&traffic(&["--by-app", "--since", "yesterday"])));
        assert_eq!(
            rows[0],
            ["APP", "CATEGORY", "CLIENTS", "DOWN", "UP", "TOTAL"]
        );
        assert_eq!(rows.len(), 7);
    }

    #[test]
    fn traffic_json_and_ids() {
        let out = ok_out(&traffic(&["-o", "json"]));
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["items"][1]["id"], "macbook-pro-0001");
        assert_eq!(v["items"][1]["totalBytes"], 2337275904_i64);
        assert_eq!(v["items"][1]["topApp"]["id"], 262145);
        assert_eq!(
            ok_out(&traffic(&["-o", "ids"])),
            "nas-1-0002\nmacbook-pro-0001\nsonos-one-sl-l-bb03\n"
        );
        let out = ok_out(&traffic(&["--by-app", "-o", "json"]));
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["items"][0]["clientCount"], 1);
    }

    fn wans_get(args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_unifictl"))
            .arg("--fixtures")
            .arg(fixtures())
            .args(["wans", "get"])
            .args(args)
            .env("TZ", "UTC")
            .env("UNIFICTL_NOW", "2026-10-11T12:00:00Z")
            .output()
            .unwrap()
    }

    #[test]
    fn wans_get_shows_outages() {
        let out = ok_out(&wans_get(&["internet-1"]));
        let rows = cells(&out);
        let n = rows.iter().position(|r| r[0] == "OUTAGES").unwrap();
        assert_eq!(rows[n][1], "2 since Oct 05 00:00 (99.95% available)");
        assert_eq!(rows[n + 2], ["START", "END", "DURATION"]);
        assert_eq!(
            rows[n + 3],
            ["Oct 06 03:12:45", "Oct 06 03:14:20", "1m 35s"]
        );
        assert_eq!(rows[n + 4], ["Oct 09 21:40:08", "Oct 09 21:40:49", "41s"]);
    }

    #[test]
    fn wans_get_json_carries_outages() {
        let v: serde_json::Value =
            serde_json::from_str(&ok_out(&wans_get(&["internet-1", "-o", "json"]))).unwrap();
        assert_eq!(v["id"], "internet-1");
        assert_eq!(v["outages"]["since"], "2026-10-05T00:00:00+00:00");
        assert_eq!(v["outages"]["availability"], 99.95);
        let first = &v["outages"]["items"][0];
        assert_eq!(first["start"], "2026-10-06T03:12:45+00:00");
        assert_eq!(first["end"], "2026-10-06T03:14:20+00:00");
        assert_eq!(first["durationSeconds"], 95);
        assert!(first.get("startMs").is_none());
    }

    #[test]
    fn wans_get_without_history_entry_has_no_outages() {
        let out = ok_out(&wans_get(&["internet-2"]));
        assert!(!out.contains("OUTAGES"), "{out}");
        let v: serde_json::Value =
            serde_json::from_str(&ok_out(&wans_get(&["internet-2", "-o", "json"]))).unwrap();
        assert!(v["outages"].is_null());
    }

    #[test]
    fn wans_get_since() {
        ok_out(&wans_get(&["internet-1", "--since", "-30d"]));
        ok_out(&wans_get(&["internet-1", "-S", "2026-10-01"]));
        let o = wans_get(&["internet-1", "--since", "+1h"]);
        assert_eq!(o.status.code(), Some(1));
        assert_eq!(
            String::from_utf8_lossy(&o.stderr),
            "Error: --since must be in the past\n"
        );
    }

    #[test]
    fn wans_list_json_has_no_outages() {
        let out = stdout(&run(&["wans", "list", "-o", "json"]));
        assert!(!out.contains("outages"), "{out}");
    }

    #[test]
    fn traffic_ids_only_for_the_clients_view() {
        for args in [
            &["macbook-pro-0001", "-o", "ids"][..],
            &["--by-app", "-o", "ids"],
        ] {
            let out = traffic(args);
            assert_eq!(out.status.code(), Some(1), "{args:?}");
            assert!(String::from_utf8_lossy(&out.stderr).contains("-o ids"));
        }
    }

    #[test]
    fn traffic_id_conflicts_with_by_app() {
        assert_eq!(
            traffic(&["macbook-pro-0001", "--by-app"]).status.code(),
            Some(2)
        );
    }

    #[test]
    fn traffic_unknown_client_and_bad_window() {
        let out = traffic(&["nobody-00"]);
        assert_eq!(out.status.code(), Some(1));
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("Network client not found: nobody-00")
        );
        let out = traffic(&["--since", "today", "--until", "yesterday"]);
        assert_eq!(out.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&out.stderr).contains("--since is after --until"));
    }

    #[test]
    fn missing_catalogue_prints_note_once() {
        let dir = tempfile::tempdir().unwrap();
        for entry in std::fs::read_dir(fixtures()).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_str().unwrap().to_string();
            if !name.starts_with("unifi-dpi-") {
                std::fs::copy(&path, dir.path().join(&name)).unwrap();
            }
        }
        let out = run_in(dir.path(), &["clients", "traffic", "--by-app"]);
        assert!(out.status.success());
        let err = String::from_utf8_lossy(&out.stderr);
        assert_eq!(
            err.matches("note: application names unavailable (needs an API key)")
                .count(),
            1,
            "{err}"
        );
        assert!(stdout(&out).contains("Unknown (4:1)"));
    }
}

#[test]
fn ids_with_config_check_fails_before_checking() {
    let dir = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_unifictl"))
        .args(["-o", "ids", "config", "check"])
        .env_clear()
        .env("HOME", dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "Error: -o ids is only supported by devices, clients, vlans, ssids and wans list, and clients traffic\n"
    );
    assert!(out.stdout.is_empty(), "no check report");
}

#[test]
fn missing_config_points_at_init() {
    let dir = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_unifictl"))
        .args(["devices", "list"])
        .env_clear()
        .env("HOME", dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        err,
        format!(
            "Error: config: no config file at {}; run `unifictl config init`\n",
            dir.path().join(".config/unifictl/config.toml").display()
        )
    );
}

fn unifictl_in_home(home: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_unifictl"))
        .args(args)
        .env_clear()
        .env("HOME", home)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap()
}

#[test]
fn config_check_without_config_fails_with_hint() {
    let dir = tempfile::tempdir().unwrap();
    let out = unifictl_in_home(dir.path(), &["config", "check"]);
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.starts_with("✗  config"), "{stdout}");
    assert!(
        stdout.contains("hint: run `unifictl config init`"),
        "{stdout}"
    );
    assert!(
        out.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let out = unifictl_in_home(dir.path(), &["-o", "json", "config", "check"]);
    assert_eq!(out.status.code(), Some(1));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["ok"], false);
    assert_eq!(v["checks"][0]["step"], "config");
    assert_eq!(v["checks"].as_array().unwrap().len(), 7);
}

fn init_in(xdg: &std::path::Path, args: &[&str]) -> Output {
    let mut all = vec!["config", "init"];
    all.extend_from_slice(args);
    Command::new(env!("CARGO_BIN_EXE_unifictl"))
        .args(&all)
        .env_clear()
        .env("XDG_CONFIG_HOME", xdg)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap()
}

#[test]
fn config_init_writes_private_file_once() {
    let dir = tempfile::tempdir().unwrap();
    let xdg = dir.path().join("nested/xdg");
    let out = init_in(
        &xdg,
        &["--no-check", "--host", "192.168.1.1", "--api-key", "k"],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stdout.is_empty());
    let path = xdg.join("unifictl/config.toml");
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        format!("wrote {}\n", path.display())
    );
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "host = \"192.168.1.1\"\napi_key = \"k\"\nsite = \"default\"\n"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    let out = init_in(&xdg, &["--no-check", "--host", "other", "--api-key", "k"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("already exists; pass --force"), "{err}");
}

#[test]
fn config_init_without_tty_or_flags_fails() {
    let dir = tempfile::tempdir().unwrap();
    let out = init_in(dir.path(), &["--no-check"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("pass --host"), "{err}");
}

#[test]
fn config_init_api_key_conflicts_with_username() {
    let dir = tempfile::tempdir().unwrap();
    let out = init_in(dir.path(), &["--api-key", "k", "--username", "u"]);
    assert_eq!(out.status.code(), Some(2));
}
