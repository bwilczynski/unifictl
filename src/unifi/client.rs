//! HTTP access to a UniFi controller, ported from
//! homelab-api/internal/adapters/unifi.go.

use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use reqwest::StatusCode;
use reqwest::blocking::{Client, Response};
use serde::de::DeserializeOwned;
use serde_json::json;

use super::UnifiApi;
use super::types::{ClientV2, Device, Envelope, NetworkConf, Sta, WlanConf};
use crate::cache::Cache;
use crate::config::{Auth, Config};
use crate::error::{Error, Result};

const TIMEOUT: Duration = Duration::from_secs(30);

pub struct UnifiClient {
    base_url: String,
    auth: Auth,
    site: String,
    http: Client,
    /// Session mode only: whether the cookie jar holds a live session. Locked
    /// for the duration of a login, so parallel reads share one login.
    logged_in: Mutex<bool>,
    cache: Option<Cache>,
    timeout: Duration,
}

impl UnifiClient {
    pub fn new(cfg: &Config) -> Result<Self> {
        Self::with_base_url(
            format!("https://{}", cfg.host),
            cfg.auth.clone(),
            cfg.site.clone(),
            cfg.insecure_tls,
        )
    }

    pub fn with_base_url(
        base_url: String,
        auth: Auth,
        site: String,
        insecure_tls: bool,
    ) -> Result<Self> {
        let http = Client::builder()
            // Never follow redirects: reqwest would forward X-API-KEY to the new location.
            .redirect(reqwest::redirect::Policy::none())
            .cookie_store(true)
            .danger_accept_invalid_certs(insecure_tls)
            .build()
            .map_err(|e| Error::Unifi(format!("unifi client: {}", describe(&e))))?;
        Ok(Self {
            base_url,
            auth,
            site,
            http,
            logged_in: Mutex::new(false),
            cache: None,
            timeout: TIMEOUT,
        })
    }

    /// Serves fresh responses from `cache` and stores new ones in it.
    pub fn with_cache(mut self, cache: Cache) -> Self {
        self.cache = Some(cache);
        self
    }

    /// Gives up on each request after `timeout` instead of the default 30 s.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// UniFi OS proxies the Network app at /proxy/network; legacy controllers
    /// expose it at the root.
    fn prefix(&self) -> &'static str {
        match self.auth {
            Auth::ApiKey(_) => "/proxy/network",
            Auth::Session { .. } => "",
        }
    }

    /// Logs in unless a session is live, or always with `force` (after a 401).
    fn ensure_login(&self, force: bool) -> Result<()> {
        let mut live = self
            .logged_in
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if force || !*live {
            *live = false;
            self.login()?;
            *live = true;
        }
        Ok(())
    }

    fn login(&self) -> Result<()> {
        let Auth::Session { username, password } = &self.auth else {
            return Ok(());
        };
        let resp = self
            .http
            .post(format!("{}/api/login", self.base_url))
            .json(&json!({ "username": username, "password": password }))
            .timeout(self.timeout)
            .send()
            .map_err(|e| Error::Unifi(format!("unifi login: {}", describe(&e))))?;
        if resp.status() != StatusCode::OK {
            return Err(Error::Unifi(format!(
                "unifi login: status {}",
                resp.status().as_u16()
            )));
        }
        Ok(())
    }

    fn send(&self, url: &str) -> Result<Response> {
        let mut req = self.http.get(url).timeout(self.timeout);
        if let Auth::ApiKey(key) = &self.auth {
            req = req.header("X-API-KEY", key);
        }
        req.send()
            .map_err(|e| Error::Unifi(format!("unifi request: {}", describe(&e))))
    }

    /// GET `{prefix}{path}`, from the cache when fresh. Only responses that
    /// parse are cached.
    fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}{}{}", self.base_url, self.prefix(), path);
        let cached = self.cache.as_ref().and_then(|c| c.get(&url));
        if let Some(value) = cached.and_then(|body| serde_json::from_str(&body).ok()) {
            return Ok(value);
        }
        let body = self.fetch(&url)?;
        let value = serde_json::from_str(&body)
            .map_err(|e| Error::Unifi(format!("unifi parse response: {e}")))?;
        if let Some(cache) = &self.cache {
            cache.put(&url, &body);
        }
        Ok(value)
    }

    /// GET `url` and return the body; in session mode re-authenticates once on 401.
    fn fetch(&self, url: &str) -> Result<String> {
        let session = matches!(self.auth, Auth::Session { .. });
        if session {
            self.ensure_login(false)?;
        }
        let mut resp = self.send(url)?;
        if resp.status() == StatusCode::UNAUTHORIZED {
            if !session {
                return Err(Error::Unifi("unifi request: invalid API key (401)".into()));
            }
            self.ensure_login(true)
                .map_err(|e| Error::Unifi(format!("unifi re-auth: {e}")))?;
            resp = self.send(url)?;
            if resp.status() == StatusCode::UNAUTHORIZED {
                return Err(Error::Unifi(
                    "unifi request: unauthorized after re-auth".into(),
                ));
            }
        }
        if !resp.status().is_success() {
            return Err(Error::Unifi(format!(
                "unifi request: status {}",
                resp.status().as_u16()
            )));
        }
        resp.text()
            .map_err(|e| Error::Unifi(format!("unifi read body: {}", describe(&e))))
    }

    /// Bare-array v2 endpoint; a `null` body is an empty list (as in Go).
    fn get_list<T: DeserializeOwned>(&self, path: &str) -> Result<Vec<T>> {
        Ok(self.get::<Option<Vec<T>>>(path)?.unwrap_or_default())
    }

    fn get_data<T: DeserializeOwned + Default>(&self, endpoint: &str) -> Result<T> {
        let env: Envelope<T> = self.get(&format!("/api/s/{}/{endpoint}", self.site))?;
        Ok(env.data)
    }
}

impl UnifiApi for UnifiClient {
    fn devices(&self) -> Result<Vec<Device>> {
        self.get_data("stat/device")
    }

    fn clients(&self) -> Result<Vec<Sta>> {
        self.get_data("stat/sta")
    }

    fn active_clients(&self) -> Result<Vec<ClientV2>> {
        self.get_list(&format!(
            "/v2/api/site/{}/clients/active?includeTrafficUsage=false&includeUnifiDevices=false",
            self.site
        ))
    }

    fn offline_clients(&self, history_days: i64) -> Result<Vec<ClientV2>> {
        self.get_list(&format!(
            "/v2/api/site/{}/clients/history?onlyNonBlocked=true&withinHours={}",
            self.site,
            history_days * 24
        ))
    }

    fn wlan_conf(&self) -> Result<Vec<WlanConf>> {
        self.get_data("rest/wlanconf")
    }

    fn network_conf(&self) -> Result<Vec<NetworkConf>> {
        self.get_data("rest/networkconf")
    }
}

/// reqwest's Display omits the cause ("error sending request"); append the
/// source chain so TLS and DNS failures are visible.
fn describe(e: &reqwest::Error) -> String {
    let mut msg = e.to_string();
    let mut source = std::error::Error::source(e);
    while let Some(s) = source {
        msg.push_str(": ");
        msg.push_str(&s.to_string());
        source = s.source();
    }
    with_tls_hint(msg)
}

/// UniFi gateways ship self-signed certificates; point users at the fix.
pub(crate) fn with_tls_hint(msg: String) -> String {
    if msg.to_lowercase().contains("certificate") {
        format!(
            "{msg} (set insecure_tls = true or UNIFI_INSECURE_TLS=1 for a self-signed controller certificate)"
        )
    } else {
        msg
    }
}

#[cfg(test)]
mod tests {
    use httpmock::prelude::*;
    use serde_json::json;

    use super::*;

    fn api_key_client(server: &MockServer) -> UnifiClient {
        UnifiClient::with_base_url(
            server.base_url(),
            Auth::ApiKey("k".into()),
            "default".into(),
            false,
        )
        .unwrap()
    }

    fn session_client(server: &MockServer) -> UnifiClient {
        let auth = Auth::Session {
            username: "admin".into(),
            password: "pw".into(),
        };
        UnifiClient::with_base_url(server.base_url(), auth, "lab".into(), false).unwrap()
    }

    #[test]
    fn api_key_sends_header_and_uses_proxy_prefix() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(GET)
                .path("/proxy/network/api/s/default/stat/device")
                .header("X-API-KEY", "k");
            then.status(200).json_body(
                json!({"meta": {"rc": "ok"}, "data": [{"name": "US 8", "type": "usw"}]}),
            );
        });
        let devices = api_key_client(&server).devices().unwrap();
        m.assert();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].name, "US 8");
    }

    #[test]
    fn v2_endpoints_decode_bare_arrays_with_query() {
        let server = MockServer::start();
        let active = server.mock(|when, then| {
            when.method(GET)
                .path("/proxy/network/v2/api/site/default/clients/active")
                .query_param("includeTrafficUsage", "false")
                .query_param("includeUnifiDevices", "false");
            then.status(200)
                .json_body(json!([{"mac": "aa:bb:cc:00:00:01", "status": "online"}]));
        });
        let history = server.mock(|when, then| {
            when.method(GET)
                .path("/proxy/network/v2/api/site/default/clients/history")
                .query_param("onlyNonBlocked", "true")
                .query_param("withinHours", "168");
            then.status(200).json_body(json!([]));
        });
        let all = api_key_client(&server).all_clients(7).unwrap();
        active.assert();
        history.assert();
        assert_eq!(all.len(), 1);
    }

    #[test]
    fn v2_null_body_is_empty_list() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/proxy/network/v2/api/site/default/clients/active");
            then.status(200)
                .json_body(json!([{"mac": "aa:bb:cc:00:00:01", "status": "online"}]));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/proxy/network/v2/api/site/default/clients/history");
            then.status(200).body("null");
        });
        assert_eq!(api_key_client(&server).all_clients(30).unwrap().len(), 1);
    }

    #[test]
    fn redirects_are_not_followed() {
        let server = MockServer::start();
        let target = server.mock(|when, then| {
            when.method(GET).path("/elsewhere");
            then.status(200).json_body(json!({"data": []}));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/proxy/network/api/s/default/stat/device");
            then.status(302)
                .header("Location", server.url("/elsewhere"));
        });
        let err = api_key_client(&server).devices().unwrap_err();
        assert_eq!(err.to_string(), "unifi request: status 302");
        target.assert_calls(0);
    }

    #[test]
    fn invalid_api_key_message() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/proxy/network/api/s/default/rest/wlanconf");
            then.status(401);
        });
        let err = api_key_client(&server).wlan_conf().unwrap_err();
        assert_eq!(err.to_string(), "unifi request: invalid API key (401)");
    }

    #[test]
    fn session_logs_in_once_and_sends_cookie() {
        let server = MockServer::start();
        let login = server.mock(|when, then| {
            when.method(POST)
                .path("/api/login")
                .json_body(json!({"username": "admin", "password": "pw"}));
            then.status(200)
                .header("Set-Cookie", "unifises=abc; Path=/")
                .json_body(json!({}));
        });
        let conf = server.mock(|when, then| {
            when.method(GET)
                .path("/api/s/lab/rest/networkconf")
                .header("cookie", "unifises=abc");
            then.status(200).json_body(json!({"data": []}));
        });
        let c = session_client(&server);
        c.network_conf().unwrap();
        c.network_conf().unwrap();
        login.assert_calls(1);
        conf.assert_calls(2);
    }

    #[test]
    fn parallel_session_reads_share_one_login() {
        let server = MockServer::start();
        let login = server.mock(|when, then| {
            when.method(POST).path("/api/login");
            then.status(200)
                .header("Set-Cookie", "unifises=abc; Path=/")
                .json_body(json!({}));
        });
        server.mock(|when, then| {
            when.method(GET).header("cookie", "unifises=abc");
            then.status(200).json_body(json!({"data": []}));
        });
        let c = session_client(&server);
        std::thread::scope(|s| {
            let reads: Vec<_> = (0..4).map(|_| s.spawn(|| c.devices())).collect();
            for r in reads {
                r.join().unwrap().unwrap();
            }
        });
        login.assert_calls(1);
    }

    #[test]
    fn service_reads_endpoints_in_parallel() {
        let delay = Duration::from_millis(400);
        let server = MockServer::start();
        for endpoint in ["stat/device", "stat/sta", "rest/networkconf"] {
            server.mock(|when, then| {
                when.method(GET)
                    .path(format!("/proxy/network/api/s/default/{endpoint}"));
                then.status(200).delay(delay).json_body(json!({"data": []}));
            });
        }
        let svc = crate::service::Service::new(Box::new(api_key_client(&server)), 30);
        let start = std::time::Instant::now();
        svc.list_ports(&Default::default()).unwrap();
        let elapsed = start.elapsed();
        assert!(elapsed < delay * 2, "three reads took {elapsed:?}");
    }

    #[test]
    fn session_relogs_once_on_401_then_gives_up() {
        let server = MockServer::start();
        let login = server.mock(|when, then| {
            when.method(POST).path("/api/login");
            then.status(200).json_body(json!({}));
        });
        let sta = server.mock(|when, then| {
            when.method(GET).path("/api/s/lab/stat/sta");
            then.status(401);
        });
        let err = session_client(&server).clients().unwrap_err();
        assert_eq!(err.to_string(), "unifi request: unauthorized after re-auth");
        login.assert_calls(2);
        sta.assert_calls(2);
    }

    #[test]
    fn login_failure_and_http_errors() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/api/login");
            then.status(400);
        });
        assert_eq!(
            session_client(&server).devices().unwrap_err().to_string(),
            "unifi login: status 400"
        );

        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/proxy/network/api/s/default/stat/device");
            then.status(500);
        });
        assert_eq!(
            api_key_client(&server).devices().unwrap_err().to_string(),
            "unifi request: status 500"
        );
    }

    #[test]
    fn malformed_body_is_parse_error() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/proxy/network/api/s/default/stat/device");
            then.status(200).body("<html>");
        });
        let err = api_key_client(&server).devices().unwrap_err().to_string();
        assert!(err.starts_with("unifi parse response:"), "{err}");
    }

    #[test]
    fn unreachable_controller_is_request_error() {
        let c = UnifiClient::with_base_url(
            "http://127.0.0.1:1".into(),
            Auth::ApiKey("k".into()),
            "default".into(),
            false,
        )
        .unwrap();
        let err = c.devices().unwrap_err().to_string();
        assert!(err.starts_with("unifi request:"), "{err}");
    }

    fn cache_in(dir: &tempfile::TempDir, read: bool) -> Cache {
        Cache::new(dir.path().to_path_buf(), Duration::from_secs(60), read)
    }

    #[test]
    fn fresh_responses_come_from_cache() {
        let dir = tempfile::tempdir().unwrap();
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(GET)
                .path("/proxy/network/api/s/default/stat/device");
            then.status(200)
                .json_body(json!({"data": [{"name": "US 8", "type": "usw"}]}));
        });
        for _ in 0..2 {
            let c = api_key_client(&server).with_cache(cache_in(&dir, true));
            assert_eq!(c.devices().unwrap()[0].name, "US 8");
        }
        m.assert_calls(1);

        let c = api_key_client(&server).with_cache(cache_in(&dir, false));
        c.devices().unwrap();
        m.assert_calls(2);
    }

    #[test]
    fn errors_and_unparseable_bodies_are_not_cached() {
        let dir = tempfile::tempdir().unwrap();
        let server = MockServer::start();
        let mut m = server.mock(|when, then| {
            when.method(GET)
                .path("/proxy/network/api/s/default/stat/device");
            then.status(200).body("<html>");
        });
        let c = api_key_client(&server).with_cache(cache_in(&dir, true));
        assert!(c.devices().is_err());
        m.delete();
        let m = server.mock(|when, then| {
            when.method(GET)
                .path("/proxy/network/api/s/default/stat/device");
            then.status(500);
        });
        assert!(c.devices().is_err());
        assert!(c.devices().is_err());
        m.assert_calls(2);
    }

    #[test]
    fn session_cache_hit_skips_login() {
        let dir = tempfile::tempdir().unwrap();
        let server = MockServer::start();
        let login = server.mock(|when, then| {
            when.method(POST).path("/api/login");
            then.status(200).json_body(json!({}));
        });
        server.mock(|when, then| {
            when.method(GET).path("/api/s/lab/rest/networkconf");
            then.status(200).json_body(json!({"data": []}));
        });
        session_client(&server)
            .with_cache(cache_in(&dir, true))
            .network_conf()
            .unwrap();
        session_client(&server)
            .with_cache(cache_in(&dir, true))
            .network_conf()
            .unwrap();
        login.assert_calls(1);
    }

    #[test]
    fn tls_errors_get_insecure_hint() {
        let msg =
            with_tls_hint("error sending request: invalid peer certificate: UnknownIssuer".into());
        assert!(msg.contains("insecure_tls = true"), "{msg}");
        assert_eq!(
            with_tls_hint("connection refused".into()),
            "connection refused"
        );
    }
}
