//! `config check`: walks config → permissions → dns → tcp → tls → auth → site
//! and reports the step where setup breaks.

use std::path::Path;

use serde::Serialize;

use super::probe;
use crate::config::{self, Auth, Config};
use crate::unifi::client::{AccessError, TransportError, UnifiClient};
use crate::unifi::types::Site;

const STEPS: [&str; 7] = ["config", "permissions", "dns", "tcp", "tls", "auth", "site"];
const TLS_HINT: &str = "set insecure_tls = true (UniFi gateways ship self-signed certificates)";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Pass,
    Warn,
    Fail,
    Skip,
}

#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub step: &'static str,
    pub status: Status,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

impl Check {
    pub fn new(step: &'static str, status: Status, detail: impl Into<String>) -> Self {
        Self {
            step,
            status,
            detail: detail.into(),
            hint: None,
        }
    }

    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub ok: bool,
    pub checks: Vec<Check>,
}

impl Report {
    pub fn new(checks: Vec<Check>) -> Self {
        let ok = checks.iter().all(|c| c.status != Status::Fail);
        Self { ok, checks }
    }
}

pub fn run(path: &Path, explicit: bool, env: &dyn Fn(&str) -> Option<String>) -> Report {
    run_with(path, explicit, env, "https")
}

/// `scheme` is `http` only in tests, against a plain-HTTP mock controller.
pub(crate) fn run_with(
    path: &Path,
    explicit: bool,
    env: &dyn Fn(&str) -> Option<String>,
    scheme: &str,
) -> Report {
    let mut checks = Vec::new();
    steps(&mut checks, path, explicit, env, scheme);
    if let Some(failed) = checks
        .iter()
        .find(|c| c.status == Status::Fail)
        .map(|c| c.step)
    {
        for &step in &STEPS[checks.len()..] {
            checks.push(Check::new(
                step,
                Status::Skip,
                format!("skipped: {failed} failed"),
            ));
        }
    }
    Report::new(checks)
}

/// Pushes one check per step, stopping after the first failure.
fn steps(
    checks: &mut Vec<Check>,
    path: &Path,
    explicit: bool,
    env: &dyn Fn(&str) -> Option<String>,
    scheme: &str,
) -> Option<()> {
    let exists = path.is_file();
    let (cfg, secret_in_file) = match config::load(path, explicit, env) {
        Ok(loaded) => {
            checks.push(Check::new(
                "config",
                Status::Pass,
                config_detail(path, exists, env),
            ));
            (loaded.config, loaded.secret_in_file)
        }
        Err(e) => {
            let mut check = Check::new("config", Status::Fail, e.to_string());
            if !exists {
                check = check.hint("run `unifictl config init`");
            }
            checks.push(check);
            return None;
        }
    };
    checks.push(permissions(path, exists, secret_in_file));

    let (host, port) = probe::split_host_port(&cfg.host);
    let addrs = gate(
        checks,
        match probe::resolve(&host, port) {
            Ok(addrs) => {
                let ips: Vec<String> = addrs.iter().map(|a| a.ip().to_string()).collect();
                Ok((
                    Check::new("dns", Status::Pass, format!("{host} → {}", ips.join(", "))),
                    addrs,
                ))
            }
            Err(e) => Err(Check::new(
                "dns",
                Status::Fail,
                format!("cannot resolve {host}: {e}"),
            )),
        },
    )?;
    gate(checks, match probe::connect(&addrs) {
        Ok(addr) => Ok((Check::new("tcp", Status::Pass, format!("connected to {addr}")), ())),
        Err(e) => Err(Check::new("tcp", Status::Fail, format!("cannot connect to port {port}: {e}"))
            .hint(format!(
                "is the controller listening on port {port}? set host = \"{host}:<port>\" if it uses another"
            ))),
    })?;
    gate(checks, tls(&cfg, scheme))?;
    let sites = gate(checks, auth(&cfg, scheme))?;
    checks.push(site(&cfg.site, &sites));
    Some(())
}

/// Records the step's check and hands its value on, or stops the run.
fn gate<T>(checks: &mut Vec<Check>, result: Result<(Check, T), Check>) -> Option<T> {
    match result {
        Ok((check, value)) => {
            checks.push(check);
            Some(value)
        }
        Err(check) => {
            checks.push(check);
            None
        }
    }
}

fn config_detail(path: &Path, exists: bool, env: &dyn Fn(&str) -> Option<String>) -> String {
    let mut detail = if exists {
        path.display().to_string()
    } else {
        "no file, environment only".to_string()
    };
    let overrides = config::env_overrides(env);
    if !overrides.is_empty() {
        detail.push_str("; overrides: ");
        detail.push_str(&overrides.join(", "));
    }
    detail
}

fn permissions(path: &Path, exists: bool, secret_in_file: bool) -> Check {
    if !exists {
        return Check::new("permissions", Status::Skip, "no config file");
    }
    match config::file_mode(path) {
        None => Check::new("permissions", Status::Skip, "not checked on this platform"),
        Some(mode) if mode & 0o077 == 0 => {
            Check::new("permissions", Status::Pass, format!("{mode:04o}"))
        }
        Some(mode) if !secret_in_file => Check::new(
            "permissions",
            Status::Pass,
            format!("{mode:04o}; no secret in the file"),
        ),
        Some(mode) => Check::new(
            "permissions",
            Status::Warn,
            format!("{mode:04o}: readable by other users"),
        )
        .hint(format!("chmod 600 {}", path.display())),
    }
}

/// A check should not hang for the 30 s a normal command tolerates.
const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

fn client(cfg: &Config, scheme: &str) -> crate::error::Result<UnifiClient> {
    Ok(UnifiClient::with_base_url(
        format!("{scheme}://{}", cfg.host),
        cfg.auth.clone(),
        cfg.site.clone(),
        cfg.insecure_tls,
    )?
    .with_timeout(PROBE_TIMEOUT))
}

fn tls(cfg: &Config, scheme: &str) -> Result<(Check, ()), Check> {
    let fail = |detail: String| Check::new("tls", Status::Fail, detail);
    let client = client(cfg, scheme).map_err(|e| fail(e.to_string()))?;
    match client.handshake() {
        Ok(()) if cfg.insecure_tls => Ok((
            Check::new(
                "tls",
                Status::Warn,
                "certificate not verified (insecure_tls = true)",
            ),
            (),
        )),
        Ok(()) => Ok((Check::new("tls", Status::Pass, "certificate valid"), ())),
        Err(e @ TransportError::Certificate(_)) => Err(fail(e.to_string()).hint(TLS_HINT)),
        Err(e) => Err(fail(e.to_string())),
    }
}

fn auth(cfg: &Config, scheme: &str) -> Result<(Check, Vec<Site>), Check> {
    let fail = |detail: &str| Check::new("auth", Status::Fail, detail);
    let client = client(cfg, scheme).map_err(|e| fail(&e.to_string()))?;
    let api_key = matches!(cfg.auth, Auth::ApiKey(_));
    match client.sites() {
        Ok(sites) => {
            let detail = match &cfg.auth {
                Auth::ApiKey(_) => "API key accepted".to_string(),
                Auth::Session { username, .. } => format!("logged in as {username}"),
            };
            Ok((Check::new("auth", Status::Pass, detail), sites))
        }
        Err(AccessError::Status(401 | 403)) if api_key => Err(fail("API key rejected")
            .hint("create a key under Settings → Control Plane → Integrations")),
        Err(AccessError::Status(404)) if api_key => Err(fail(
            "no UniFi OS API at /proxy/network (status 404)",
        )
        .hint("a self-hosted Network application needs username and password instead of api_key")),
        Err(e @ AccessError::Login(_)) => Err(fail(&e.to_string())
            .hint("check username and password; UniFi OS consoles also accept an api_key")),
        Err(e @ AccessError::Secret(_)) => {
            let what = if api_key { "API key" } else { "password" };
            Err(fail(&e.to_string()).hint(format!(
                "run the command yourself: it must print the {what} on its first line"
            )))
        }
        Err(e) => Err(fail(&e.to_string())),
    }
}

fn site(name: &str, sites: &[Site]) -> Check {
    if let Some(s) = sites.iter().find(|s| s.name == name) {
        return Check::new(
            "site",
            Status::Pass,
            format!("site \"{name}\" found ({})", s.desc),
        );
    }
    let hint = if sites.is_empty() {
        "the controller reported no sites; is the API key for the right console?".to_string()
    } else {
        let available: Vec<String> = sites
            .iter()
            .map(|s| format!("{} ({})", s.name, s.desc))
            .collect();
        format!("available: {}", available.join(", "))
    };
    Check::new("site", Status::Fail, format!("site \"{name}\" not found")).hint(hint)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use httpmock::prelude::*;
    use serde_json::json;

    use super::Status::*;
    use super::*;

    fn no_env(_: &str) -> Option<String> {
        None
    }

    fn write_cfg(dir: &tempfile::TempDir, body: &str) -> PathBuf {
        let p = dir.path().join("config.toml");
        crate::private_fs::replace(&p, body).unwrap();
        p
    }

    fn statuses(r: &Report) -> Vec<(&'static str, Status)> {
        r.checks.iter().map(|c| (c.step, c.status)).collect()
    }

    fn check<'a>(r: &'a Report, step: &str) -> &'a Check {
        r.checks.iter().find(|c| c.step == step).unwrap()
    }

    fn mock_sites(server: &MockServer, path: &str, status: u16, data: serde_json::Value) {
        server.mock(|when, then| {
            when.method(GET).path(path);
            then.status(status).json_body(json!({ "data": data }));
        });
    }

    fn api_key_cfg(dir: &tempfile::TempDir, server: &MockServer, extra: &str) -> PathBuf {
        write_cfg(
            dir,
            &format!("host = \"{}\"\napi_key = \"k\"\n{extra}", server.address()),
        )
    }

    #[test]
    fn all_steps_pass() {
        let server = MockServer::start();
        mock_sites(
            &server,
            "/proxy/network/api/self/sites",
            200,
            json!([{"name": "default", "desc": "Default"}]),
        );
        let dir = tempfile::tempdir().unwrap();
        let r = run_with(&api_key_cfg(&dir, &server, ""), false, &no_env, "http");
        assert!(r.ok, "{r:?}");
        assert_eq!(
            statuses(&r),
            vec![
                ("config", Pass),
                ("permissions", Pass),
                ("dns", Pass),
                ("tcp", Pass),
                ("tls", Pass),
                ("auth", Pass),
                ("site", Pass),
            ]
        );
        assert_eq!(check(&r, "auth").detail, "API key accepted");
        assert_eq!(check(&r, "site").detail, "site \"default\" found (Default)");
    }

    #[test]
    fn rejected_key_fails_auth_and_skips_site() {
        let server = MockServer::start();
        mock_sites(&server, "/proxy/network/api/self/sites", 401, json!([]));
        let dir = tempfile::tempdir().unwrap();
        let r = run_with(&api_key_cfg(&dir, &server, ""), false, &no_env, "http");
        assert!(!r.ok);
        assert_eq!(check(&r, "auth").status, Fail);
        assert_eq!(check(&r, "auth").detail, "API key rejected");
        assert_eq!(check(&r, "site").status, Skip);
        assert_eq!(check(&r, "site").detail, "skipped: auth failed");
    }

    #[cfg(unix)]
    #[test]
    fn failed_secret_command_fails_auth_with_a_hint() {
        let server = MockServer::start();
        let dir = tempfile::tempdir().unwrap();
        let p = write_cfg(
            &dir,
            &format!(
                "host = \"{}\"\napi_key_command = \"exit 2\"\n",
                server.address()
            ),
        );
        let r = run_with(&p, false, &no_env, "http");
        let auth = check(&r, "auth");
        assert_eq!(auth.status, Fail);
        assert_eq!(
            auth.detail,
            "api_key_command `exit 2`: exited with status 2"
        );
        assert_eq!(
            auth.hint.as_deref(),
            Some("run the command yourself: it must print the API key on its first line")
        );
        assert_eq!(check(&r, "site").status, Skip);
    }

    #[test]
    fn api_key_against_legacy_controller_hints_session_auth() {
        let server = MockServer::start();
        let dir = tempfile::tempdir().unwrap();
        let r = run_with(&api_key_cfg(&dir, &server, ""), false, &no_env, "http");
        let auth = check(&r, "auth");
        assert_eq!(auth.status, Fail);
        assert!(
            auth.hint
                .as_deref()
                .unwrap()
                .contains("username and password"),
            "{auth:?}"
        );
    }

    #[test]
    fn session_login_failure() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/api/login");
            then.status(400);
        });
        let dir = tempfile::tempdir().unwrap();
        let p = write_cfg(
            &dir,
            &format!(
                "host = \"{}\"\nusername = \"admin\"\npassword = \"pw\"\n",
                server.address()
            ),
        );
        let r = run_with(&p, false, &no_env, "http");
        assert_eq!(check(&r, "auth").detail, "login failed (status 400)");
    }

    #[test]
    fn missing_site_lists_available_ones() {
        let server = MockServer::start();
        mock_sites(
            &server,
            "/proxy/network/api/self/sites",
            200,
            json!([{"name": "default", "desc": "Default"}, {"name": "ab12", "desc": "Lab"}]),
        );
        let dir = tempfile::tempdir().unwrap();
        let r = run_with(
            &api_key_cfg(&dir, &server, "site = \"nope\"\n"),
            false,
            &no_env,
            "http",
        );
        let site = check(&r, "site");
        assert_eq!(site.status, Fail);
        assert_eq!(site.detail, "site \"nope\" not found");
        assert_eq!(
            site.hint.as_deref(),
            Some("available: default (Default), ab12 (Lab)")
        );
    }

    #[test]
    fn controller_without_sites() {
        let server = MockServer::start();
        mock_sites(&server, "/proxy/network/api/self/sites", 200, json!([]));
        let dir = tempfile::tempdir().unwrap();
        let r = run_with(&api_key_cfg(&dir, &server, ""), false, &no_env, "http");
        assert_eq!(
            check(&r, "site").hint.as_deref(),
            Some("the controller reported no sites; is the API key for the right console?")
        );
    }

    #[test]
    fn refused_tcp_skips_the_rest() {
        let port = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let dir = tempfile::tempdir().unwrap();
        let p = write_cfg(
            &dir,
            &format!("host = \"127.0.0.1:{port}\"\napi_key = \"k\"\n"),
        );
        let r = run_with(&p, false, &no_env, "http");
        assert_eq!(
            statuses(&r)[2..],
            [
                ("dns", Pass),
                ("tcp", Fail),
                ("tls", Skip),
                ("auth", Skip),
                ("site", Skip)
            ]
        );
        assert_eq!(check(&r, "tls").detail, "skipped: tcp failed");
    }

    #[test]
    fn unresolvable_host() {
        let dir = tempfile::tempdir().unwrap();
        let p = write_cfg(&dir, "host = \"unifictl-test.invalid\"\napi_key = \"k\"\n");
        let r = run_with(&p, false, &no_env, "http");
        assert_eq!(check(&r, "dns").status, Fail);
        assert_eq!(check(&r, "tcp").status, Skip);
    }

    #[test]
    fn missing_config_fails_first_step() {
        let dir = tempfile::tempdir().unwrap();
        let r = run_with(&dir.path().join("none.toml"), false, &no_env, "http");
        assert_eq!(r.checks.len(), 7);
        let config = check(&r, "config");
        assert_eq!(config.status, Fail);
        assert_eq!(config.hint.as_deref(), Some("run `unifictl config init`"));
        assert!(r.checks[1..].iter().all(|c| c.status == Skip));
    }

    #[test]
    fn env_only_config_skips_permissions() {
        let server = MockServer::start();
        mock_sites(
            &server,
            "/proxy/network/api/self/sites",
            200,
            json!([{"name": "default", "desc": "Default"}]),
        );
        let host = server.address().to_string();
        let env = move |k: &str| match k {
            "UNIFI_HOST" => Some(host.clone()),
            "UNIFI_API_KEY" => Some("abcdefghijkl".to_string()),
            _ => None,
        };
        let dir = tempfile::tempdir().unwrap();
        let r = run_with(&dir.path().join("none.toml"), false, &env, "http");
        assert!(r.ok, "{r:?}");
        assert_eq!(
            check(&r, "config").detail,
            format!(
                "no file, environment only; overrides: UNIFI_HOST={}, UNIFI_API_KEY=abcd…",
                server.address()
            )
        );
        assert_eq!(check(&r, "permissions").status, Skip);
    }

    #[cfg(unix)]
    #[test]
    fn world_readable_file_warns_but_passes() {
        use std::os::unix::fs::PermissionsExt;
        let server = MockServer::start();
        mock_sites(
            &server,
            "/proxy/network/api/self/sites",
            200,
            json!([{"name": "default", "desc": "Default"}]),
        );
        let dir = tempfile::tempdir().unwrap();
        let p = api_key_cfg(&dir, &server, "");
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
        let r = run_with(&p, false, &no_env, "http");
        assert!(r.ok);
        let perms = check(&r, "permissions");
        assert_eq!(perms.status, Warn);
        assert_eq!(perms.hint, Some(format!("chmod 600 {}", p.display())));
    }

    #[cfg(unix)]
    #[test]
    fn readable_file_without_a_secret_passes_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let p = write_cfg(
            &dir,
            "host = \"127.0.0.1:9\"\napi_key_command = \"pass show unifi/api-key\"\n",
        );
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
        let r = run_with(&p, false, &no_env, "http");
        let perms = check(&r, "permissions");
        assert_eq!(perms.status, Pass);
        assert_eq!(perms.detail, "0644; no secret in the file");
        assert_eq!(perms.hint, None);
    }

    #[test]
    fn insecure_tls_is_a_warning() {
        let server = MockServer::start();
        mock_sites(
            &server,
            "/proxy/network/api/self/sites",
            200,
            json!([{"name": "default", "desc": "Default"}]),
        );
        let dir = tempfile::tempdir().unwrap();
        let r = run_with(
            &api_key_cfg(&dir, &server, "insecure_tls = true\n"),
            false,
            &no_env,
            "http",
        );
        assert!(r.ok);
        assert_eq!(check(&r, "tls").status, Warn);
        assert_eq!(
            check(&r, "tls").detail,
            "certificate not verified (insecure_tls = true)"
        );
    }

    #[test]
    fn json_shape() {
        let r = Report::new(vec![
            Check::new("dns", Pass, "ok"),
            Check::new("tcp", Fail, "refused").hint("h"),
        ]);
        assert_eq!(
            serde_json::to_value(&r).unwrap(),
            json!({"ok": false, "checks": [
                {"step": "dns", "status": "pass", "detail": "ok"},
                {"step": "tcp", "status": "fail", "detail": "refused", "hint": "h"}
            ]})
        );
    }
}
