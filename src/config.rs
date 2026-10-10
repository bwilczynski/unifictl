//! Config: ~/.config/unifictl/config.toml overlaid with UNIFI_* env vars.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

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

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    host: Option<String>,
    api_key: Option<String>,
    username: Option<String>,
    password: Option<String>,
    site: Option<String>,
    insecure_tls: Option<bool>,
    client_history_days: Option<i64>,
    cache_ttl: Option<u64>,
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

/// Reads `path` (a missing file is fine unless the user named it explicitly),
/// applies env overrides and validates.
pub fn load(path: &Path, explicit: bool, env: &dyn Fn(&str) -> Option<String>) -> Result<Loaded> {
    let mut warnings = Vec::new();
    let file = match std::fs::read_to_string(path) {
        Ok(text) => {
            warnings.extend(permission_warning(path));
            toml::from_str::<FileConfig>(&text)
                .map_err(|e| Error::Config(format!("{}: {e}", path.display())))?
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && !explicit => FileConfig::default(),
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

/// Accepts `host`, `host:port` or `https://host[:port]/`.
fn normalize_host(raw: &str) -> Result<String> {
    let host = raw
        .trim()
        .trim_start_matches("https://")
        .trim_end_matches('/');
    if host.contains("://") || host.contains('/') {
        return Err(Error::Config(format!(
            "host must be a hostname or host:port (the controller is always reached over https), got {raw:?}"
        )));
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

#[cfg(unix)]
fn permission_warning(path: &Path) -> Option<String> {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(path).ok()?.permissions().mode();
    (mode & 0o077 != 0).then(|| {
        format!(
            "{} is readable by other users; run `chmod 600 {}`",
            path.display(),
            path.display()
        )
    })
}

#[cfg(not(unix))]
fn permission_warning(_path: &Path) -> Option<String> {
    None
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

    fn write(dir: &tempfile::TempDir, body: &str) -> PathBuf {
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
        let p = write(&dir, "host = \"192.168.1.1\"\napi_key = \"k\"\n");
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
        let p = write(
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
        let p = write(&dir, "host = \"h\"\napi_key = \"k\"\ncache_ttl = 60\n");
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
        let p = dir.path().join("none.toml");
        let err = load(&p, false, &env_of(&[("UNIFI_API_KEY", "k")]))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("host is required") && err.contains("UNIFI_HOST"),
            "{err}"
        );
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
        let p = write(&dir, "host = \"h\"\napikey = \"k\"\n");
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
        let p = write(&dir, "host = \"h\"\napi_key = \"k\"\n");
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
        let l = load(&p, false, &env_of(&[])).unwrap();
        assert_eq!(l.warnings.len(), 1);
        assert!(l.warnings[0].contains("chmod 600"));
    }
}
