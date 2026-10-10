//! Equivalence with the Go implementation: for every case recorded by
//! tools/golden/generate.sh, unifictl must produce the same JSON data and the
//! same table rows as hlctl + homelab-api (modulo the documented ID/uri
//! differences, which the generator already normalized).
#![cfg(feature = "fixtures")]

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

use serde_json::Value;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_unifictl"))
        .arg("--fixtures")
        .arg(root().join("tests/fixtures"))
        .args(args)
        .output()
        .expect("run unifictl")
}

/// Numbers compared as f64 so `8` == `8.0`; objects compare order-insensitively.
fn normalize(v: Value) -> Value {
    match v {
        Value::Number(n) => Value::from(n.as_f64().expect("finite number")),
        Value::Array(a) => Value::Array(a.into_iter().map(normalize).collect()),
        Value::Object(o) => Value::Object(o.into_iter().map(|(k, v)| (k, normalize(v))).collect()),
        other => other,
    }
}

/// Cells split on runs of 2+ spaces, so column padding does not matter.
fn rows(text: &str) -> Vec<Vec<String>> {
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

#[test]
fn matches_go_implementation() {
    let dir = root().join("tests/golden");
    let manifest = fs::read_to_string(dir.join("cases.txt"))
        .expect("tests/golden missing: run `mise run golden`");
    let mut failures = Vec::new();
    let mut count = 0;

    for line in manifest.lines().filter(|l| !l.trim().is_empty()) {
        count += 1;
        let (name, args) = line.split_once('\t').expect("cases.txt: name<TAB>args");
        let args: Vec<&str> = args.split(' ').collect();

        if dir.join(format!("{name}.err")).exists() {
            let out = run(&args);
            if out.status.code() != Some(1) {
                failures.push(format!(
                    "{name}: hlctl failed, expected unifictl to exit 1, got {:?}",
                    out.status.code()
                ));
            }
            continue;
        }

        let mut json_args = vec!["-o", "json"];
        json_args.extend(&args);
        let out = run(&json_args);
        if !out.status.success() {
            failures.push(format!(
                "{name}: failed: {}",
                String::from_utf8_lossy(&out.stderr)
            ));
            continue;
        }
        let got: Value = serde_json::from_slice(&out.stdout).expect("unifictl JSON");
        let want: Value =
            serde_json::from_str(&fs::read_to_string(dir.join(format!("{name}.json"))).unwrap())
                .unwrap();
        if normalize(got.clone()) != normalize(want.clone()) {
            failures.push(format!(
                "{name}: JSON differs\n  want: {want}\n  got:  {got}"
            ));
        }

        let got = String::from_utf8(run(&args).stdout).unwrap();
        let want = fs::read_to_string(dir.join(format!("{name}.txt"))).unwrap();
        if rows(&got) != rows(&want) {
            failures.push(format!(
                "{name}: table differs\n--- want\n{want}--- got\n{got}"
            ));
        }
    }

    assert!(count > 40, "suspiciously few golden cases: {count}");
    assert!(
        failures.is_empty(),
        "{} of {count} golden cases failed:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}
