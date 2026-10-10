use std::process::{Command, Output};

fn unifictl(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_unifictl"))
        .args(args)
        .env_remove("UNIFICTL_CONFIG")
        .output()
        .expect("run unifictl")
}

#[test]
fn version_prints_crate_version() {
    let out = unifictl(&["version"]);
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
            &["ports", "list", "--all-ports"],
            &["wans", "list"],
            &["wans", "get", "internet-2"],
            &["topology", "--include-clients"],
            &["devices", "get", "cgf-01"],
            &["vlans", "list"],
            &["vlans", "get", "lan-mgmt"],
            &["ssids", "list"],
            &["ssids", "get", "home-wifi"],
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
                // Listed but not gettable in the Go implementation too
                // (tests/golden/clients-get-sonos-one-sl-l-bb03.err).
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
                "Error: -o ids is only supported by devices, clients, vlans, ssids and wans list\n",
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
        "Error: -o ids is only supported by devices, clients, vlans, ssids and wans list\n"
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
