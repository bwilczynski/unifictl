//! Config: ~/.config/unifictl/config.toml overlaid with UNIFI_* env vars.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

pub const DEFAULT_SITE: &str = "default";
const DEFAULT_HISTORY_DAYS: i64 = 30;
const DEFAULT_CACHE_TTL_SECS: u64 = 15;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Auth {
    /// UniFi OS: X-API-KEY header, /proxy/network prefix.
    ApiKey(String),
    /// Legacy controller: cookie session via /api/login.
    Session { username: String, password: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// host[:port]; always reached over https.
    pub host: String,
    pub auth: Auth,
    pub site: String,
    pub insecure_tls: bool,
    pub client_history_days: i64,
    /// How long controller responses are reused; zero disables the cache.
    pub cache_ttl: Duration,
}

#[derive(Debug)]
pub struct Loaded {
    pub config: Config,
    /// Non-fatal problems to print on stderr.
    pub warnings: Vec<String>,
}

/// The keys of config.toml. `config init` writes only the ones it sets.
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FileConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub insecure_tls: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_history_days: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_ttl: Option<u64>,
}

/// `--config` > `UNIFICTL_CONFIG` > `$XDG_CONFIG_HOME/unifictl/config.toml` >
/// `$HOME/.config/unifictl/config.toml`.
pub fn resolve_path(flag: Option<&Path>, env: &dyn Fn(&str) -> Option<String>) -> PathBuf {
    if let Some(p) = flag {
        return p.to_path_buf();
    }
    let var = |k: &str| env(k).filter(|v| !v.is_empty());
    if let Some(p) = var("UNIFICTL_CONFIG") {
        return PathBuf::from(p);
    }
    let base = var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| var("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from(".config"));
    base.join("unifictl").join("config.toml")
}

/// `$XDG_CACHE_HOME/unifictl` > `$HOME/.cache/unifictl`.
pub fn resolve_cache_dir(env: &dyn Fn(&str) -> Option<String>) -> PathBuf {
    let var = |k: &str| env(k).filter(|v| !v.is_empty());
    let base = var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| var("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(|| PathBuf::from(".cache"));
    base.join("unifictl")
}

/// Writes `file` to `path` owner-only, creating its directory.
pub fn write(path: &Path, file: &FileConfig) -> Result<()> {
    let body = toml::to_string(file).map_err(|e| Error::Config(format!("encode config: {e}")))?;
    let fail = |e: std::io::Error| Error::Config(format!("cannot write {}: {e}", path.display()));
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        crate::private_fs::create_dir(dir).map_err(fail)?;
    }
    crate::private_fs::replace(path, &body).map_err(fail)
}

/// Reads `path` (a missing file is fine unless the user named it explicitly),
/// applies env overrides and validates.
pub fn load(path: &Path, explicit: bool, env: &dyn Fn(&str) -> Option<String>) -> Result<Loaded> {
    let mut warnings = Vec::new();
    let mut no_file = false;
    let file = match std::fs::read_to_string(path) {
        Ok(text) => {
            warnings.extend(permission_warning(path));
            toml::from_str::<FileConfig>(&text).map_err(|e| parse_error(path, &text, &e))?
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && !explicit => {
            no_file = true;
            FileConfig::default()
        }
        Err(e) => {
            return Err(Error::Config(format!(
                "cannot read {}: {e}",
                path.display()
            )));
        }
    };
    let var = |k: &str| env(k).filter(|v| !v.is_empty());
    let pick =
        |key: &str, from_file: Option<String>| var(key).or(from_file).filter(|v| !v.is_empty());

    let host = normalize_host(&pick("UNIFI_HOST", file.host).unwrap_or_default())?;
    if host.is_empty() {
        if no_file {
            return Err(Error::Config(format!(
                "no config file at {}; run `unifictl config init`",
                path.display()
            )));
        }
        return Err(missing("host", "UNIFI_HOST", path));
    }

    let auth = match pick("UNIFI_API_KEY", file.api_key) {
        Some(key) => Auth::ApiKey(key),
        None => match (
            pick("UNIFI_USERNAME", file.username),
            pick("UNIFI_PASSWORD", file.password),
        ) {
            (Some(username), Some(password)) => Auth::Session { username, password },
            (None, None) => {
                return Err(Error::Config(format!(
                    "either api_key or username/password is required (set api_key in {} or UNIFI_API_KEY)",
                    path.display()
                )));
            }
            (None, Some(_)) => return Err(missing("username", "UNIFI_USERNAME", path)),
            (Some(_), None) => return Err(missing("password", "UNIFI_PASSWORD", path)),
        },
    };

    let site = pick("UNIFI_SITE", file.site).unwrap_or_else(|| DEFAULT_SITE.to_string());
    let insecure_tls = match var("UNIFI_INSECURE_TLS") {
        Some(v) => parse_bool("UNIFI_INSECURE_TLS", &v)?,
        None => file.insecure_tls.unwrap_or(false),
    };
    let days = match var("UNIFI_CLIENT_HISTORY_DAYS") {
        Some(v) => v.parse().map_err(|_| {
            Error::Config(format!(
                "UNIFI_CLIENT_HISTORY_DAYS: expected an integer, got {v:?}"
            ))
        })?,
        None => file.client_history_days.unwrap_or(0),
    };
    let client_history_days = if days <= 0 {
        DEFAULT_HISTORY_DAYS
    } else {
        days
    };

    let cache_ttl = match var("UNIFI_CACHE_TTL") {
        Some(v) => v.parse().map_err(|_| {
            Error::Config(format!(
                "UNIFI_CACHE_TTL: expected a number of seconds, got {v:?}"
            ))
        })?,
        None => file.cache_ttl.unwrap_or(DEFAULT_CACHE_TTL_SECS),
    };

    Ok(Loaded {
        config: Config {
            host,
            auth,
            site,
            insecure_tls,
            client_history_days,
            cache_ttl: Duration::from_secs(cache_ttl),
        },
        warnings,
    })
}

fn missing(key: &str, env: &str, path: &Path) -> Error {
    Error::Config(format!(
        "{key} is required (set {key} in {} or {env})",
        path.display()
    ))
}

/// One line naming the position, never toml's snippet of the file: the
/// snippet can show a secret, and `config check` output gets pasted around.
fn parse_error(path: &Path, text: &str, e: &toml::de::Error) -> Error {
    let at = e.span().map_or(String::new(), |span| {
        let before = &text[..span.start.min(text.len())];
        let line = before.matches('\n').count() + 1;
        let column = before.len() - before.rfind('\n').map_or(0, |i| i + 1) + 1;
        format!(" at line {line}, column {column}")
    });
    Error::Config(format!("{}: {}{at}", path.display(), e.message()))
}

/// Accepts `host`, `host:port`, `[v6]:port`, a bare v6 literal (bracketed
/// here, as URLs need) or `https://host[:port]/`.
pub fn normalize_host(raw: &str) -> Result<String> {
    let host = raw
        .trim()
        .trim_start_matches("https://")
        .trim_end_matches('/');
    if host.contains("://") || host.contains('/') {
        return Err(Error::Config(format!(
            "host must be a hostname or host:port (the controller is always reached over https), got {raw:?}"
        )));
    }
    if host.parse::<std::net::Ipv6Addr>().is_ok() {
        return Ok(format!("[{host}]"));
    }
    Ok(host.to_string())
}

fn parse_bool(name: &str, v: &str) -> Result<bool> {
    match v.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(Error::Config(format!(
            "{name}: expected true or false, got {v:?}"
        ))),
    }
}

/// The env vars that override config keys, in the order `config check` lists them.
const ENV_VARS: [&str; 8] = [
    "UNIFI_HOST",
    "UNIFI_API_KEY",
    "UNIFI_USERNAME",
    "UNIFI_PASSWORD",
    "UNIFI_SITE",
    "UNIFI_INSECURE_TLS",
    "UNIFI_CLIENT_HISTORY_DAYS",
    "UNIFI_CACHE_TTL",
];

/// `NAME=value` for every non-empty override, secrets masked.
pub fn env_overrides(env: &dyn Fn(&str) -> Option<String>) -> Vec<String> {
    ENV_VARS
        .iter()
        .filter_map(|&name| {
            let value = env(name).filter(|v| !v.is_empty())?;
            let shown = match name {
                "UNIFI_API_KEY" => mask_key(&value),
                "UNIFI_PASSWORD" => "****".to_string(),
                _ => value,
            };
            Some(format!("{name}={shown}"))
        })
        .collect()
}

/// Enough of an API key to tell two apart, never enough to use it.
pub fn mask_key(key: &str) -> String {
    if key.chars().count() <= 8 {
        "…".to_string()
    } else {
        format!("{}…", key.chars().take(4).collect::<String>())
    }
}

/// Permission bits of `path`; `None` off Unix or when it cannot be read.
#[cfg(unix)]
pub fn file_mode(path: &Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    Some(std::fs::metadata(path).ok()?.permissions().mode() & 0o777)
}

#[cfg(not(unix))]
pub fn file_mode(_path: &Path) -> Option<u32> {
    None
}

fn permission_warning(path: &Path) -> Option<String> {
    let mode = file_mode(path)?;
    (mode & 0o077 != 0).then(|| {
        format!(
            "{} is readable by other users; run `chmod 600 {}`",
            path.display(),
            path.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let m: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |k: &str| m.get(k).cloned()
    }

    fn write_file(dir: &tempfile::TempDir, body: &str) -> PathBuf {
        let p = dir.path().join("config.toml");
        std::fs::write(&p, body).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        p
    }

    #[test]
    fn resolve_path_precedence() {
        let flag = Path::new("/x/flag.toml");
        let env = env_of(&[
            ("UNIFICTL_CONFIG", "/x/env.toml"),
            ("XDG_CONFIG_HOME", "/xdg"),
            ("HOME", "/home/u"),
        ]);
        assert_eq!(
            resolve_path(Some(flag), &env),
            PathBuf::from("/x/flag.toml")
        );
        assert_eq!(resolve_path(None, &env), PathBuf::from("/x/env.toml"));
        let env = env_of(&[("XDG_CONFIG_HOME", "/xdg"), ("HOME", "/home/u")]);
        assert_eq!(
            resolve_path(None, &env),
            PathBuf::from("/xdg/unifictl/config.toml")
        );
        let env = env_of(&[("HOME", "/home/u")]);
        assert_eq!(
            resolve_path(None, &env),
            PathBuf::from("/home/u/.config/unifictl/config.toml")
        );
    }

    #[test]
    fn file_with_api_key_and_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let p = write_file(&dir, "host = \"192.168.1.1\"\napi_key = \"k\"\n");
        let l = load(&p, false, &env_of(&[])).unwrap();
        assert_eq!(
            l.config,
            Config {
                host: "192.168.1.1".into(),
                auth: Auth::ApiKey("k".into()),
                site: "default".into(),
                insecure_tls: false,
                client_history_days: 30,
                cache_ttl: Duration::from_secs(15),
            }
        );
        assert!(l.warnings.is_empty());
    }

    #[test]
    fn env_overrides_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = write_file(
            &dir,
            "host = \"a\"\napi_key = \"k\"\nsite = \"s1\"\nclient_history_days = 7\n",
        );
        let env = env_of(&[
            ("UNIFI_HOST", "b"),
            ("UNIFI_SITE", "s2"),
            ("UNIFI_INSECURE_TLS", "true"),
            ("UNIFI_CLIENT_HISTORY_DAYS", "3"),
        ]);
        let c = load(&p, false, &env).unwrap().config;
        assert_eq!(
            (
                c.host.as_str(),
                c.site.as_str(),
                c.insecure_tls,
                c.client_history_days
            ),
            ("b", "s2", true, 3)
        );
    }

    #[test]
    fn cache_ttl_from_file_env_and_zero() {
        let dir = tempfile::tempdir().unwrap();
        let p = write_file(&dir, "host = \"h\"\napi_key = \"k\"\ncache_ttl = 60\n");
        let ttl = |env: &[(&str, &str)]| load(&p, false, &env_of(env)).map(|l| l.config.cache_ttl);
        assert_eq!(ttl(&[]).unwrap(), Duration::from_secs(60));
        assert_eq!(ttl(&[("UNIFI_CACHE_TTL", "0")]).unwrap(), Duration::ZERO);
        let err = ttl(&[("UNIFI_CACHE_TTL", "-1")]).unwrap_err().to_string();
        assert!(err.contains("UNIFI_CACHE_TTL"), "{err}");
    }

    #[test]
    fn resolve_cache_dir_precedence() {
        let env = env_of(&[("XDG_CACHE_HOME", "/xdg"), ("HOME", "/home/u")]);
        assert_eq!(resolve_cache_dir(&env), PathBuf::from("/xdg/unifictl"));
        let env = env_of(&[("HOME", "/home/u")]);
        assert_eq!(
            resolve_cache_dir(&env),
            PathBuf::from("/home/u/.cache/unifictl")
        );
    }

    #[test]
    fn missing_default_file_uses_env_only() {
        let dir = tempfile::tempdir().unwrap();
        let env = env_of(&[
            ("UNIFI_HOST", "h"),
            ("UNIFI_USERNAME", "u"),
            ("UNIFI_PASSWORD", "p"),
        ]);
        let c = load(&dir.path().join("none.toml"), false, &env)
            .unwrap()
            .config;
        assert_eq!(
            c.auth,
            Auth::Session {
                username: "u".into(),
                password: "p".into()
            }
        );
    }

    #[test]
    fn explicit_missing_file_is_error() {
        let dir = tempfile::tempdir().unwrap();
        let env = env_of(&[("UNIFI_HOST", "h"), ("UNIFI_API_KEY", "k")]);
        let err = load(&dir.path().join("typo.toml"), true, &env).unwrap_err();
        assert!(err.to_string().starts_with("config: cannot read"), "{err}");
    }

    #[test]
    fn validation_messages_name_key_and_env() {
        let dir = tempfile::tempdir().unwrap();
        let empty = write_file(&dir, "");
        let err = load(&empty, false, &env_of(&[("UNIFI_API_KEY", "k")]))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("host is required") && err.contains("UNIFI_HOST"),
            "{err}"
        );
        let p = dir.path().join("none.toml");
        let err = load(&p, false, &env_of(&[("UNIFI_HOST", "h")]))
            .unwrap_err()
            .to_string();
        assert!(err.contains("either api_key or username/password"), "{err}");
        let err = load(
            &p,
            false,
            &env_of(&[("UNIFI_HOST", "h"), ("UNIFI_USERNAME", "u")]),
        )
        .unwrap_err()
        .to_string();
        assert!(
            err.contains("password is required") && err.contains("UNIFI_PASSWORD"),
            "{err}"
        );
    }

    #[test]
    fn https_url_host_is_normalized() {
        let dir = tempfile::tempdir().unwrap();
        let env = env_of(&[
            ("UNIFI_HOST", "https://192.168.1.1/"),
            ("UNIFI_API_KEY", "k"),
        ]);
        assert_eq!(
            load(&dir.path().join("n"), false, &env)
                .unwrap()
                .config
                .host,
            "192.168.1.1"
        );
    }

    #[test]
    fn http_url_host_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let env = env_of(&[("UNIFI_HOST", "http://192.168.1.1"), ("UNIFI_API_KEY", "k")]);
        let err = load(&dir.path().join("n"), false, &env)
            .unwrap_err()
            .to_string();
        assert!(err.contains("host must be"), "{err}");
    }

    #[test]
    fn bad_bool_env_is_error() {
        let dir = tempfile::tempdir().unwrap();
        let env = env_of(&[
            ("UNIFI_HOST", "h"),
            ("UNIFI_API_KEY", "k"),
            ("UNIFI_INSECURE_TLS", "maybe"),
        ]);
        let err = load(&dir.path().join("n"), false, &env)
            .unwrap_err()
            .to_string();
        assert!(err.contains("UNIFI_INSECURE_TLS"), "{err}");
    }

    #[test]
    fn unknown_keys_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let p = write_file(&dir, "host = \"h\"\napikey = \"k\"\n");
        assert!(
            load(&p, false, &env_of(&[]))
                .unwrap_err()
                .to_string()
                .contains("apikey")
        );
    }

    #[cfg(unix)]
    #[test]
    fn world_readable_file_warns() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let p = write_file(&dir, "host = \"h\"\napi_key = \"k\"\n");
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
        let l = load(&p, false, &env_of(&[])).unwrap();
        assert_eq!(l.warnings.len(), 1);
        assert!(l.warnings[0].contains("chmod 600"));
    }

    #[cfg(unix)]
    fn mode(p: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn write_creates_dirs_and_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("xdg/unifictl/config.toml");
        let file = FileConfig {
            host: Some("192.168.1.1".into()),
            api_key: Some("k".into()),
            site: Some("lab".into()),
            ..Default::default()
        };
        write(&p, &file).unwrap();
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "host = \"192.168.1.1\"\napi_key = \"k\"\nsite = \"lab\"\n"
        );
        let l = load(&p, true, &env_of(&[])).unwrap();
        assert_eq!(l.config.site, "lab");
        assert!(l.warnings.is_empty(), "{:?}", l.warnings);
        #[cfg(unix)]
        {
            assert_eq!(mode(&p), 0o600);
            assert_eq!(mode(p.parent().unwrap()), 0o700);
        }
    }

    #[cfg(unix)]
    #[test]
    fn write_over_world_readable_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let p = write_file(&dir, "host = \"old\"\napi_key = \"k\"\n");
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
        let file = FileConfig {
            host: Some("new".into()),
            username: Some("u".into()),
            password: Some("p".into()),
            insecure_tls: Some(true),
            ..Default::default()
        };
        write(&p, &file).unwrap();
        assert_eq!(mode(&p), 0o600);
        let c = load(&p, true, &env_of(&[])).unwrap().config;
        assert_eq!(c.host, "new");
        assert!(c.insecure_tls);
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            1,
            "temp file left behind"
        );
    }

    #[test]
    fn missing_default_file_points_at_init() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("none.toml");
        let err = load(&p, false, &env_of(&[])).unwrap_err().to_string();
        assert_eq!(
            err,
            format!(
                "config: no config file at {}; run `unifictl config init`",
                p.display()
            )
        );
        // An existing file without host keeps the precise message.
        let p = write_file(&dir, "api_key = \"k\"\n");
        let err = load(&p, false, &env_of(&[])).unwrap_err().to_string();
        assert!(err.contains("host is required"), "{err}");
    }

    #[test]
    fn env_overrides_are_listed_and_masked() {
        let env = env_of(&[
            ("UNIFI_SITE", "lab"),
            ("UNIFI_API_KEY", "abcdefghijkl"),
            ("UNIFI_PASSWORD", "hunter2"),
            ("UNIFI_HOST", ""),
            ("HOME", "/home/u"),
        ]);
        assert_eq!(
            env_overrides(&env),
            vec![
                "UNIFI_API_KEY=abcd…",
                "UNIFI_PASSWORD=****",
                "UNIFI_SITE=lab"
            ]
        );
    }

    #[test]
    fn short_keys_are_fully_masked() {
        assert_eq!(mask_key("12345678"), "…");
        assert_eq!(mask_key("123456789"), "1234…");
    }

    #[cfg(unix)]
    #[test]
    fn file_mode_reports_permission_bits() {
        let dir = tempfile::tempdir().unwrap();
        let p = write_file(&dir, "");
        assert_eq!(file_mode(&p), Some(0o600));
        assert_eq!(file_mode(&dir.path().join("missing")), None);
    }

    #[test]
    fn bare_ipv6_host_is_bracketed() {
        assert_eq!(normalize_host("fd00::1").unwrap(), "[fd00::1]");
        assert_eq!(normalize_host("::1").unwrap(), "[::1]");
        assert_eq!(normalize_host("[fd00::1]:8443").unwrap(), "[fd00::1]:8443");
        assert_eq!(normalize_host("unifi.lan:8443").unwrap(), "unifi.lan:8443");
    }

    #[test]
    fn parse_errors_do_not_echo_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = write_file(&dir, "host = \"h\"\napi_key = \"supersecretkey123\n");
        let err = load(&p, false, &env_of(&[])).unwrap_err().to_string();
        assert!(!err.contains("supersecret"), "{err}");
        assert!(!err.contains('\n'), "{err}");
        assert!(err.contains("line 2"), "{err}");
    }
}
