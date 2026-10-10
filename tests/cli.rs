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
        let o = run_in(dir.path(), &["topology"]);
        assert_eq!(o.status.code(), Some(1));
        assert_eq!(
            String::from_utf8_lossy(&o.stderr),
            "Error: no gateway node found in topology\n"
        );
    }
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
