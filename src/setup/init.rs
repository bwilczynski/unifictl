//! `config init`: gathers host, auth and site from flags, env and prompts,
//! then writes config.toml. The caller runs `config check` afterwards.

use std::path::Path;

use super::prompt::Prompter;
use crate::cli::InitArgs;
use crate::config::{self, Auth, DEFAULT_SITE, FileConfig, Secret};
use crate::error::{Error, Result};
use crate::unifi::client::{TransportError, UnifiClient};

pub fn run(
    args: &InitArgs,
    path: &Path,
    env: &dyn Fn(&str) -> Option<String>,
    p: &mut dyn Prompter,
) -> Result<()> {
    run_with(args, path, env, p, "https")
}

/// `scheme` is `http` only in tests, against a plain-HTTP mock controller.
pub(crate) fn run_with(
    args: &InitArgs,
    path: &Path,
    env: &dyn Fn(&str) -> Option<String>,
    p: &mut dyn Prompter,
    scheme: &str,
) -> Result<()> {
    if path.exists() && !args.force {
        return Err(Error::Config(format!(
            "{} already exists; pass --force to overwrite",
            path.display()
        )));
    }
    let var = |k: &str| env(k).filter(|v| !v.is_empty());
    let host = match &args.host {
        Some(h) => config::normalize_host(h)?,
        None => ask_host(p)?,
    };
    let auth = collect_auth(args, &var, p)?;
    let mut insecure_tls = args.insecure_tls;
    let site = match (&args.site, args.no_check) {
        (Some(site), _) => site.clone(),
        (None, true) => ask_site(p)?,
        (None, false) => {
            if !insecure_tls {
                insecure_tls = offer_insecure(&host, &auth, scheme, p)?;
            }
            choose_site(&host, &auth, insecure_tls, scheme, p)?
        }
    };
    let split = |s: Secret| match s {
        Secret::Literal(v) => (Some(v), None),
        Secret::Command(c) => (None, Some(c)),
    };
    let ((api_key, api_key_command), username, (password, password_command)) = match auth {
        Auth::ApiKey(key) => (split(key), None, (None, None)),
        Auth::Session { username, password } => ((None, None), Some(username), split(password)),
    };
    let file = FileConfig {
        host: Some(host),
        api_key,
        api_key_command,
        username,
        password,
        password_command,
        site: Some(site),
        insecure_tls: insecure_tls.then_some(true),
        ..Default::default()
    };
    config::write(path, &file)?;
    p.note(&format!("wrote {}", path.display()));
    Ok(())
}

/// Errors out instead of prompting when nobody is there to answer.
fn require(p: &dyn Prompter, field: &str, flags: &str) -> Result<()> {
    if p.interactive() {
        Ok(())
    } else {
        Err(Error::Config(format!(
            "{field} is required when not running interactively (pass {flags})"
        )))
    }
}

fn ask_host(p: &mut dyn Prompter) -> Result<String> {
    require(p, "host", "--host")?;
    loop {
        let raw = p.line("Controller host (e.g. 192.168.1.1)", None)?;
        match config::normalize_host(&raw) {
            Ok(host) if !host.is_empty() => return Ok(host),
            Ok(_) => p.note("host is required"),
            Err(e) => p.note(&e.to_string()),
        }
    }
}

fn ask_nonempty(p: &mut dyn Prompter, label: &str) -> Result<String> {
    loop {
        let answer = p.line(label, None)?;
        if !answer.is_empty() {
            return Ok(answer);
        }
    }
}

/// A password manager command, or else the secret itself, asked for as
/// `label` and called `noun` in the command prompt.
fn ask_command_or_secret(
    p: &mut dyn Prompter,
    label: &str,
    noun: &str,
    example: &str,
) -> Result<Secret> {
    let cmd = p.line(
        &format!("{label} command, e.g. `{example}` (empty to type the {noun})"),
        None,
    )?;
    if cmd.is_empty() {
        ask_secret(p, label).map(Secret::Literal)
    } else {
        Ok(Secret::Command(cmd))
    }
}

fn ask_secret(p: &mut dyn Prompter, label: &str) -> Result<String> {
    loop {
        let answer = p.read_secret(&format!("{label}: "))?;
        if !answer.is_empty() {
            return Ok(answer);
        }
    }
}

/// `--api-key`/`--api-key-command`/$UNIFI_API_KEY, else `--username` with
/// `--password`/`--password-command`/$UNIFI_PASSWORD, else ask.
fn collect_auth(
    args: &InitArgs,
    var: &dyn Fn(&str) -> Option<String>,
    p: &mut dyn Prompter,
) -> Result<Auth> {
    if let Some(key) = &args.api_key {
        return Ok(Auth::ApiKey(Secret::Literal(key.clone())));
    }
    if let Some(cmd) = &args.api_key_command {
        return Ok(Auth::ApiKey(Secret::Command(cmd.clone())));
    }
    // A typed --username wins over a key exported for something else.
    if args.username.is_none()
        && let Some(key) = var("UNIFI_API_KEY")
    {
        p.note("using UNIFI_API_KEY from the environment");
        return Ok(Auth::ApiKey(Secret::Literal(key)));
    }
    let username = match &args.username {
        Some(u) => u.clone(),
        None => {
            require(p, "auth", "--api-key, --api-key-command or --username")?;
            loop {
                let choice = p.line(
                    "Auth method: [1] API key (UniFi OS: UCG, UDM, …)  [2] username/password",
                    Some("1"),
                )?;
                match choice.as_str() {
                    "1" => {
                        let key =
                            ask_command_or_secret(p, "API key", "key", "pass show unifi/api-key")?;
                        return Ok(Auth::ApiKey(key));
                    }
                    "2" => break ask_nonempty(p, "Username")?,
                    _ => p.note("enter 1 or 2"),
                }
            }
        }
    };
    let password = match (&args.password_command, &args.password) {
        (Some(cmd), _) => Secret::Command(cmd.clone()),
        (None, Some(pw)) => Secret::Literal(pw.clone()),
        (None, None) => match var("UNIFI_PASSWORD") {
            Some(pw) => Secret::Literal(pw),
            None => {
                require(p, "password", "--password or --password-command")?;
                ask_command_or_secret(p, "Password", "password", "pass show unifi/admin")?
            }
        },
    };
    Ok(Auth::Session { username, password })
}

fn probe_client(host: &str, auth: &Auth, insecure_tls: bool, scheme: &str) -> Result<UnifiClient> {
    Ok(UnifiClient::with_base_url(
        format!("{scheme}://{host}"),
        auth.clone(),
        DEFAULT_SITE.to_string(),
        insecure_tls,
    )?
    .with_timeout(std::time::Duration::from_secs(10)))
}

/// Asks about `insecure_tls` only once the certificate has actually failed.
/// Other connection failures are left for the final check to explain.
fn offer_insecure(host: &str, auth: &Auth, scheme: &str, p: &mut dyn Prompter) -> Result<bool> {
    let Err(TransportError::Certificate(reason)) =
        probe_client(host, auth, false, scheme)?.handshake()
    else {
        return Ok(false);
    };
    if !p.interactive() {
        return Ok(false);
    }
    p.confirm(
        &format!(
            "Controller certificate is not trusted ({reason}). Accept it anyway (insecure_tls)?"
        ),
        false,
    )
}

fn ask_site(p: &mut dyn Prompter) -> Result<String> {
    if !p.interactive() {
        return Ok(DEFAULT_SITE.to_string());
    }
    p.line("Site", Some(DEFAULT_SITE))
}

/// One site: take it. Several: pick from a list. None or an error: ask.
fn choose_site(
    host: &str,
    auth: &Auth,
    insecure_tls: bool,
    scheme: &str,
    p: &mut dyn Prompter,
) -> Result<String> {
    let sites = match probe_client(host, auth, insecure_tls, scheme)?.sites() {
        Ok(sites) if !sites.is_empty() => sites,
        Ok(_) => return ask_site(p),
        Err(e) => {
            p.note(&format!("could not list sites: {e}"));
            return ask_site(p);
        }
    };
    if let [only] = sites.as_slice() {
        p.note(&format!("using site {} ({})", only.name, only.desc));
        return Ok(only.name.clone());
    }
    let default = sites
        .iter()
        .position(|s| s.name == DEFAULT_SITE)
        .unwrap_or(0);
    if !p.interactive() {
        return Ok(sites[default].name.clone());
    }
    for (i, s) in sites.iter().enumerate() {
        p.note(&format!("  [{}] {} ({})", i + 1, s.name, s.desc));
    }
    loop {
        let answer = p.line("Site", Some(&(default + 1).to_string()))?;
        match answer.parse::<usize>() {
            Ok(n) if (1..=sites.len()).contains(&n) => return Ok(sites[n - 1].name.clone()),
            _ => p.note(&format!("enter a number from 1 to {}", sites.len())),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::path::PathBuf;

    use httpmock::prelude::*;
    use serde_json::json;

    use super::*;
    use crate::config::Auth;

    /// Answers prompts from a list; records what was asked and noted.
    struct Script {
        interactive: bool,
        answers: VecDeque<&'static str>,
        prompts: Vec<String>,
        notes: Vec<String>,
    }

    impl Script {
        fn new(interactive: bool, answers: &[&'static str]) -> Self {
            Self {
                interactive,
                answers: answers.iter().copied().collect(),
                prompts: Vec::new(),
                notes: Vec::new(),
            }
        }
    }

    impl Prompter for Script {
        fn interactive(&self) -> bool {
            self.interactive
        }
        fn read_line(&mut self, prompt: &str) -> Result<String> {
            self.prompts.push(prompt.to_string());
            self.answers
                .pop_front()
                .map(String::from)
                .ok_or_else(|| Error::Other("config init: aborted, nothing written".into()))
        }
        fn read_secret(&mut self, prompt: &str) -> Result<String> {
            self.read_line(prompt)
        }
        fn note(&mut self, msg: &str) {
            self.notes.push(msg.to_string());
        }
    }

    fn no_env(_: &str) -> Option<String> {
        None
    }

    fn target(dir: &tempfile::TempDir) -> PathBuf {
        dir.path().join("unifictl/config.toml")
    }

    fn loaded(path: &Path) -> crate::config::Config {
        crate::config::load(path, true, &no_env).unwrap().config
    }

    fn mock_sites(server: &MockServer, status: u16, data: serde_json::Value) {
        server.mock(|when, then| {
            when.method(GET).path("/proxy/network/api/self/sites");
            then.status(status).json_body(json!({ "data": data }));
        });
    }

    #[test]
    fn flags_only_without_check() {
        let dir = tempfile::tempdir().unwrap();
        let args = InitArgs {
            host: Some("https://192.168.1.1/".into()),
            api_key: Some("k".into()),
            site: Some("lab".into()),
            insecure_tls: true,
            no_check: true,
            ..Default::default()
        };
        let mut p = Script::new(false, &[]);
        run(&args, &target(&dir), &no_env, &mut p).unwrap();
        let c = loaded(&target(&dir));
        assert_eq!(
            (c.host.as_str(), c.auth, c.site.as_str(), c.insecure_tls),
            (
                "192.168.1.1",
                Auth::ApiKey(Secret::Literal("k".into())),
                "lab",
                true
            )
        );
        assert!(p.prompts.is_empty());
        assert_eq!(p.notes, vec![format!("wrote {}", target(&dir).display())]);
    }

    #[test]
    fn refuses_to_overwrite_without_force() {
        let dir = tempfile::tempdir().unwrap();
        let args = InitArgs {
            host: Some("h".into()),
            api_key: Some("k".into()),
            no_check: true,
            ..Default::default()
        };
        run(&args, &target(&dir), &no_env, &mut Script::new(false, &[])).unwrap();
        let again = InitArgs {
            host: Some("h2".into()),
            ..args
        };
        let err = run(&again, &target(&dir), &no_env, &mut Script::new(false, &[]))
            .unwrap_err()
            .to_string();
        assert!(err.contains("already exists; pass --force"), "{err}");
        assert_eq!(loaded(&target(&dir)).host, "h");

        let forced = InitArgs {
            force: true,
            ..again
        };
        run(
            &forced,
            &target(&dir),
            &no_env,
            &mut Script::new(false, &[]),
        )
        .unwrap();
        assert_eq!(loaded(&target(&dir)).host, "h2");
    }

    #[test]
    fn password_from_env_when_not_interactive() {
        let dir = tempfile::tempdir().unwrap();
        let args = InitArgs {
            host: Some("h".into()),
            username: Some("admin".into()),
            no_check: true,
            ..Default::default()
        };
        let env = |k: &str| (k == "UNIFI_PASSWORD").then(|| "pw".to_string());
        run(&args, &target(&dir), &env, &mut Script::new(false, &[])).unwrap();
        let c = loaded(&target(&dir));
        assert_eq!(
            c.auth,
            Auth::Session {
                username: "admin".into(),
                password: Secret::Literal("pw".into())
            }
        );
        assert_eq!(c.site, "default");
    }

    #[test]
    fn missing_value_when_not_interactive_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let args = InitArgs {
            api_key: Some("k".into()),
            no_check: true,
            ..Default::default()
        };
        let err = run(&args, &target(&dir), &no_env, &mut Script::new(false, &[]))
            .unwrap_err()
            .to_string();
        assert!(err.contains("pass --host"), "{err}");
        assert!(!target(&dir).exists());

        let args = InitArgs {
            host: Some("h".into()),
            no_check: true,
            ..Default::default()
        };
        let err = run(&args, &target(&dir), &no_env, &mut Script::new(false, &[]))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("pass --api-key, --api-key-command or --username"),
            "{err}"
        );
    }

    #[test]
    fn interactive_prompts_and_site_pick() {
        let server = MockServer::start();
        mock_sites(
            &server,
            200,
            json!([{"name": "default", "desc": "Default"}, {"name": "ab12", "desc": "Lab"}]),
        );
        let host: &'static str = Box::leak(server.address().to_string().into_boxed_str());
        let dir = tempfile::tempdir().unwrap();
        let mut p = Script::new(true, &["", host, "x", "1", "", "", "k", "9", "2"]);
        run_with(&InitArgs::default(), &target(&dir), &no_env, &mut p, "http").unwrap();
        let c = loaded(&target(&dir));
        assert_eq!(
            (c.auth, c.site.as_str()),
            (Auth::ApiKey(Secret::Literal("k".into())), "ab12")
        );
        assert!(!c.insecure_tls);
        // empty host re-prompts; bad auth choice re-prompts; no command asks
        // for the key; empty key re-prompts;
        // out-of-range site re-prompts; default site offered is [1] (default).
        assert_eq!(
            p.prompts
                .iter()
                .filter(|q| q.starts_with("Controller host"))
                .count(),
            2
        );
        assert!(
            p.prompts.iter().any(|q| q == "Site [1]: "),
            "{:?}",
            p.prompts
        );
        assert!(
            p.notes.iter().any(|n| n.contains("[2] ab12 (Lab)")),
            "{:?}",
            p.notes
        );
    }

    #[test]
    fn single_site_is_picked_without_asking() {
        let server = MockServer::start();
        mock_sites(
            &server,
            200,
            json!([{"name": "default", "desc": "Default"}]),
        );
        let dir = tempfile::tempdir().unwrap();
        let args = InitArgs {
            host: Some(server.address().to_string()),
            api_key: Some("k".into()),
            ..Default::default()
        };
        let mut p = Script::new(true, &[]);
        run_with(&args, &target(&dir), &no_env, &mut p, "http").unwrap();
        assert_eq!(loaded(&target(&dir)).site, "default");
        assert!(
            p.notes.iter().any(|n| n == "using site default (Default)"),
            "{:?}",
            p.notes
        );
    }

    #[test]
    fn site_lookup_failure_falls_back_to_prompt() {
        let server = MockServer::start();
        mock_sites(&server, 401, json!([]));
        let dir = tempfile::tempdir().unwrap();
        let args = InitArgs {
            host: Some(server.address().to_string()),
            api_key: Some("bad".into()),
            ..Default::default()
        };
        let mut p = Script::new(true, &[""]);
        run_with(&args, &target(&dir), &no_env, &mut p, "http").unwrap();
        assert_eq!(loaded(&target(&dir)).site, "default");
        assert!(
            p.notes
                .iter()
                .any(|n| n == "could not list sites: status 401"),
            "{:?}",
            p.notes
        );
    }

    #[test]
    fn empty_site_list_falls_back_to_prompt() {
        let server = MockServer::start();
        mock_sites(&server, 200, json!([]));
        let dir = tempfile::tempdir().unwrap();
        let args = InitArgs {
            host: Some(server.address().to_string()),
            api_key: Some("k".into()),
            ..Default::default()
        };
        let mut p = Script::new(true, &["mysite"]);
        run_with(&args, &target(&dir), &no_env, &mut p, "http").unwrap();
        assert_eq!(loaded(&target(&dir)).site, "mysite");
    }

    #[test]
    fn eof_mid_prompt_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = Script::new(true, &["h"]);
        let err = run(
            &InitArgs {
                no_check: true,
                ..Default::default()
            },
            &target(&dir),
            &no_env,
            &mut p,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("aborted"), "{err}");
        assert!(!target(&dir).exists());
    }

    #[test]
    fn username_flag_beats_env_api_key() {
        let dir = tempfile::tempdir().unwrap();
        let args = InitArgs {
            host: Some("h".into()),
            username: Some("admin".into()),
            password: Some("pw".into()),
            no_check: true,
            ..Default::default()
        };
        let env = |k: &str| (k == "UNIFI_API_KEY").then(|| "envkey123456".to_string());
        run(&args, &target(&dir), &env, &mut Script::new(false, &[])).unwrap();
        assert_eq!(
            loaded(&target(&dir)).auth,
            Auth::Session {
                username: "admin".into(),
                password: Secret::Literal("pw".into())
            }
        );
    }

    #[test]
    fn env_api_key_use_is_announced() {
        let dir = tempfile::tempdir().unwrap();
        let args = InitArgs {
            host: Some("h".into()),
            no_check: true,
            ..Default::default()
        };
        let env = |k: &str| (k == "UNIFI_API_KEY").then(|| "envkey123456".to_string());
        let mut p = Script::new(true, &[""]);
        run(&args, &target(&dir), &env, &mut p).unwrap();
        assert_eq!(
            loaded(&target(&dir)).auth,
            Auth::ApiKey(Secret::Literal("envkey123456".into()))
        );
        assert!(
            p.notes
                .iter()
                .any(|n| n == "using UNIFI_API_KEY from the environment"),
            "{:?}",
            p.notes
        );
    }

    #[test]
    fn secret_commands_are_written_instead_of_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let args = InitArgs {
            host: Some("h".into()),
            api_key_command: Some("pass show unifi/api-key".into()),
            site: Some("default".into()),
            no_check: true,
            ..Default::default()
        };
        let env = |k: &str| (k == "UNIFI_API_KEY").then(|| "envkey123456".to_string());
        run(&args, &target(&dir), &env, &mut Script::new(false, &[])).unwrap();
        assert_eq!(
            std::fs::read_to_string(target(&dir)).unwrap(),
            "host = \"h\"\napi_key_command = \"pass show unifi/api-key\"\nsite = \"default\"\n"
        );

        let dir = tempfile::tempdir().unwrap();
        let args = InitArgs {
            host: Some("h".into()),
            username: Some("admin".into()),
            password_command: Some("pass show unifi/admin".into()),
            no_check: true,
            ..Default::default()
        };
        let env = |k: &str| (k == "UNIFI_PASSWORD").then(|| "pw".to_string());
        run(&args, &target(&dir), &env, &mut Script::new(false, &[])).unwrap();
        assert_eq!(
            loaded(&target(&dir)).auth,
            Auth::Session {
                username: "admin".into(),
                password: Secret::Command("pass show unifi/admin".into())
            }
        );
    }

    #[test]
    fn interactive_asks_for_a_command_first() {
        let dir = tempfile::tempdir().unwrap();
        let args = InitArgs {
            host: Some("h".into()),
            no_check: true,
            ..Default::default()
        };
        let mut p = Script::new(true, &["1", "pass show unifi/api-key", ""]);
        run(&args, &target(&dir), &no_env, &mut p).unwrap();
        assert_eq!(
            loaded(&target(&dir)).auth,
            Auth::ApiKey(Secret::Command("pass show unifi/api-key".into()))
        );
        assert_eq!(
            p.prompts[1],
            "API key command, e.g. `pass show unifi/api-key` (empty to type the key): "
        );

        let dir = tempfile::tempdir().unwrap();
        let mut p = Script::new(true, &["2", "admin", "", "pw", ""]);
        run(&args, &target(&dir), &no_env, &mut p).unwrap();
        assert_eq!(
            loaded(&target(&dir)).auth,
            Auth::Session {
                username: "admin".into(),
                password: Secret::Literal("pw".into())
            }
        );
        assert_eq!(
            p.prompts[2..4],
            [
                "Password command, e.g. `pass show unifi/admin` (empty to type the password): ",
                "Password: "
            ]
        );
    }
}
