use std::path::Path;
use std::process::{Command, Output};

/// Asks for bash completions of the last word in `words`, as the script from
/// `completions bash` would on TAB.
fn complete(words: &[&str], env: &[(&str, &Path)]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_unifictl"))
        .env_clear()
        .env("COMPLETE", "bash")
        .env("_CLAP_IFS", "\n")
        .env("_CLAP_COMPLETE_INDEX", words.len().to_string())
        .envs(env.iter().copied())
        .arg("--")
        .arg("unifictl")
        .args(words)
        .output()
        .expect("run unifictl")
}

fn candidates(o: &Output) -> Vec<String> {
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout)
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn script_registers_unifictl_for_each_shell() {
    for shell in ["bash", "zsh", "fish", "powershell", "elvish"] {
        let o = Command::new(env!("CARGO_BIN_EXE_unifictl"))
            .args(["completions", shell])
            .output()
            .unwrap();
        assert!(o.status.success(), "{shell}");
        let script = String::from_utf8_lossy(&o.stdout);
        assert!(script.contains("COMPLETE"), "{shell}: {script}");
        assert!(script.contains("unifictl"), "{shell}: {script}");
    }
}

#[test]
fn subcommands_complete_without_a_controller() {
    let o = complete(&["dev"], &[]);
    assert_eq!(candidates(&o), ["devices"]);
}

#[test]
fn ids_complete_to_nothing_when_config_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    let o = complete(
        &["devices", "get", "u"],
        &[
            ("HOME", dir.path()),
            ("UNIFICTL_CONFIG", &dir.path().join("nope.toml")),
        ],
    );
    assert!(candidates(&o).is_empty());
    assert!(
        o.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
}

#[cfg(feature = "fixtures")]
mod with_fixtures {
    use std::path::PathBuf;

    use super::{candidates, complete};

    fn ids(words: &[&str]) -> Vec<String> {
        let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let o = complete(words, &[("UNIFICTL_FIXTURES", &fixtures)]);
        candidates(&o)
            .into_iter()
            .filter(|c| !c.starts_with('-'))
            .collect()
    }

    #[test]
    fn device_ids_filter_by_prefix() {
        assert_eq!(
            ids(&["devices", "get", "us"]),
            ["us-8", "usw-flex-2-5g-8", "us-8-60w"]
        );
    }

    #[test]
    fn ports_device_offers_only_switches_and_gateways() {
        let got = ids(&["ports", "list", "--device", ""]);
        assert!(got.contains(&"cgf-01".to_owned()), "{got:?}");
        assert!(got.contains(&"us-8".to_owned()), "{got:?}");
        assert!(!got.iter().any(|id| id.starts_with("uap-")), "{got:?}");
    }

    #[test]
    fn client_ids_complete() {
        assert_eq!(ids(&["clients", "get", "nas"]), ["nas-1-0002"]);
    }

    #[test]
    fn lookup_ids_complete_per_kind() {
        assert_eq!(
            ids(&["vlans", "get", ""]),
            ["lan-mgmt", "lan-iot", "lan-int", "lan-srv"]
        );
        assert_eq!(ids(&["ssids", "get", ""]), ["home-wifi-iot", "home-wifi"]);
        assert_eq!(ids(&["wans", "get", ""]), ["internet-1", "internet-2"]);
    }

    #[test]
    fn logs_device_and_client_complete() {
        assert_eq!(ids(&["logs", "--device", "uap"]), ["uap-02", "uap-01"]);
        assert_eq!(ids(&["logs", "--client", "mac"]), ["macbook-pro-0001"]);
    }
}
