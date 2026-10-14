//! HTTP access to a UniFi controller, ported from
//! homelab-api/internal/adapters/unifi.go.

use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use reqwest::StatusCode;
use reqwest::blocking::{Client, Response};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::json;

use super::UnifiApi;
use super::types::{
    ClientV2, Device, DpiEntry, DpiPage, Envelope, HealthSubsystem, LogQuery, NetworkConf, Site,
    Sta, SystemLogPage, TrafficReport, WanHistory, WlanConf,
};
use crate::cache::Cache;
use crate::config::{Auth, Config};
use crate::error::{Error, Result};

const TIMEOUT: Duration = Duration::from_secs(30);
/// Entries per Integration API page; the controller accepts 200.
const DPI_PAGE: i64 = 200;
/// Upper bound on catalogue pages (2112 applications need 11), against a
/// nonsensical `totalCount`.
const MAX_DPI_PAGES: usize = 50;
/// The DPI catalogue only changes with controller updates.
const CATALOGUE_TTL: Duration = Duration::from_secs(24 * 60 * 60);

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

/// Why a request never got an HTTP response, for `config check`/`config init`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    Dns(String),
    Connect(String),
    Timeout,
    /// The controller's certificate was rejected; holds the reason (e.g. `UnknownIssuer`).
    Certificate(String),
    Other(String),
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Dns(m) => write!(f, "DNS lookup failed: {m}"),
            Self::Connect(m) => write!(f, "connection failed: {m}"),
            Self::Timeout => write!(f, "timed out"),
            Self::Certificate(r) => write!(f, "certificate not trusted: {r}"),
            Self::Other(m) => write!(f, "{m}"),
        }
    }
}

/// Why listing sites failed: the request, the login, or the response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessError {
    Transport(TransportError),
    Login(u16),
    Status(u16),
    Parse(String),
}

impl std::fmt::Display for AccessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(e) => write!(f, "{e}"),
            Self::Login(code) => write!(f, "login failed (status {code})"),
            Self::Status(code) => write!(f, "status {code}"),
            Self::Parse(m) => write!(f, "unexpected response: {m}"),
        }
    }
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

    /// Completes one unauthenticated request to the controller root; any
    /// HTTP response means DNS, TCP and TLS all worked.
    pub fn handshake(&self) -> std::result::Result<(), TransportError> {
        self.http
            .get(format!("{}/", self.base_url))
            .timeout(self.timeout)
            .send()
            .map(drop)
            .map_err(|e| classify(&e))
    }

    /// Lists the controller's sites, logging in first in session mode.
    /// Uncached, and every failure is typed so callers can explain it.
    pub fn sites(&self) -> std::result::Result<Vec<Site>, AccessError> {
        let transport = |e: reqwest::Error| AccessError::Transport(classify(&e));
        if let Auth::Session { username, password } = &self.auth {
            let resp = self.post_login(username, password).map_err(transport)?;
            if resp.status() != StatusCode::OK {
                return Err(AccessError::Login(resp.status().as_u16()));
            }
        }
        let url = format!("{}{}/api/self/sites", self.base_url, self.prefix());
        let resp = self.request(&url).map_err(transport)?;
        if !resp.status().is_success() {
            return Err(AccessError::Status(resp.status().as_u16()));
        }
        let body = resp.text().map_err(transport)?;
        serde_json::from_str::<Envelope<Vec<Site>>>(&body)
            .map(|env| env.data)
            .map_err(|e| AccessError::Parse(e.to_string()))
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

    fn post_login(&self, username: &str, password: &str) -> reqwest::Result<Response> {
        self.http
            .post(format!("{}/api/login", self.base_url))
            .json(&json!({ "username": username, "password": password }))
            .timeout(self.timeout)
            .send()
    }

    fn request(&self, url: &str) -> reqwest::Result<Response> {
        let mut req = self.http.get(url).timeout(self.timeout);
        if let Auth::ApiKey(key) = &self.auth {
            req = req.header("X-API-KEY", key);
        }
        req.send()
    }

    fn post_request(&self, url: &str, body: &impl Serialize) -> reqwest::Result<Response> {
        let mut req = self.http.post(url).json(body).timeout(self.timeout);
        if let Auth::ApiKey(key) = &self.auth {
            req = req.header("X-API-KEY", key);
        }
        req.send()
    }

    fn login(&self) -> Result<()> {
        let Auth::Session { username, password } = &self.auth else {
            return Ok(());
        };
        let resp = self
            .post_login(username, password)
            .map_err(|e| Error::Unifi(format!("unifi login: {}", describe(&e))))?;
        let status = resp.status();
        if status != StatusCode::OK {
            let msg = format!("unifi login: status {}", status.as_u16());
            // Only a rejection is final; 429 and 5xx may pass on a retry.
            return Err(match status {
                StatusCode::BAD_REQUEST | StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                    Error::Auth(msg)
                }
                _ => Error::Unifi(msg),
            });
        }
        Ok(())
    }

    /// GET `{prefix}{path}`, from the cache when fresh. Only responses that
    /// parse are cached.
    fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}{}{}", self.base_url, self.prefix(), path);
        self.get_url(&url, None)
    }

    /// GET `url`, from the cache when younger than `max_age` (default: the
    /// cache TTL).
    fn get_url<T: DeserializeOwned>(&self, url: &str, max_age: Option<Duration>) -> Result<T> {
        let cached = self.cache.as_ref().and_then(|c| match max_age {
            Some(age) => c.get_within(url, age),
            None => c.get(url),
        });
        if let Some(value) = cached.and_then(|body| serde_json::from_str(&body).ok()) {
            return Ok(value);
        }
        let body = self.fetch(url)?;
        let value = serde_json::from_str(&body)
            .map_err(|e| Error::Unifi(format!("unifi parse response: {e}")))?;
        if let Some(cache) = &self.cache {
            cache.put(url, &body);
        }
        Ok(value)
    }

    /// Every entry of an Integration API list under `/v1/{path}`: the first
    /// page gives the total, the rest are read in parallel. API key only.
    fn dpi_list(&self, path: &str) -> Result<Vec<DpiEntry>> {
        if !matches!(self.auth, Auth::ApiKey(_)) {
            return Err(Error::Unifi("needs an API key".into()));
        }
        let page = |offset: i64| -> Result<DpiPage> {
            let url = format!(
                "{}/proxy/network/integration/v1/{path}?offset={offset}&limit={DPI_PAGE}",
                self.base_url
            );
            self.get_url(&url, Some(CATALOGUE_TTL))
        };
        let page = &page;
        let first = page(0)?;
        let offsets: Vec<i64> = (1..MAX_DPI_PAGES as i64)
            .map(|n| n * DPI_PAGE)
            .take_while(|&o| o < first.total_count)
            .collect();
        let rest = std::thread::scope(|s| {
            let handles: Vec<_> = offsets.iter().map(|&o| s.spawn(move || page(o))).collect();
            handles
                .into_iter()
                .map(super::joined)
                .collect::<Result<Vec<_>>>()
        })?;
        let mut all = first.data;
        all.extend(rest.into_iter().flat_map(|p| p.data));
        Ok(all)
    }

    /// GET `url` and return the body; see [`Self::fetch_with`].
    fn fetch(&self, url: &str) -> Result<String> {
        self.fetch_with(|| self.request(url))
    }

    /// Sends a request with `send` and returns the body; in session mode
    /// re-authenticates once on 401. Authentication failures are `Error::Auth`.
    fn fetch_with(&self, send: impl Fn() -> reqwest::Result<Response>) -> Result<String> {
        let send = || send().map_err(|e| Error::Unifi(format!("unifi request: {}", describe(&e))));
        let session = matches!(self.auth, Auth::Session { .. });
        if session {
            self.ensure_login(false)?;
        }
        let mut resp = send()?;
        if resp.status() == StatusCode::UNAUTHORIZED {
            if !session {
                return Err(Error::Auth("unifi request: invalid API key (401)".into()));
            }
            self.ensure_login(true).map_err(reauth_error)?;
            resp = send()?;
            if resp.status() == StatusCode::UNAUTHORIZED {
                return Err(Error::Auth(
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

    fn health(&self) -> Result<Vec<HealthSubsystem>> {
        self.get_data("stat/health")
    }

    fn system_log(&self, query: &LogQuery) -> Result<SystemLogPage> {
        let url = format!(
            "{}{}/v2/api/site/{}/system-log/all",
            self.base_url,
            self.prefix(),
            self.site
        );
        let body = self.fetch_with(|| self.post_request(&url, query))?;
        serde_json::from_str(&body).map_err(|e| Error::Unifi(format!("unifi parse response: {e}")))
    }

    /// Never cached: every window is its own URL, so entries would pile up.
    fn traffic(&self, start_ms: i64, end_ms: i64) -> Result<TrafficReport> {
        let url = format!(
            "{}{}/v2/api/site/{}/traffic?start={start_ms}&end={end_ms}&includeUnidentified=true",
            self.base_url,
            self.prefix(),
            self.site
        );
        let body = self.fetch(&url)?;
        serde_json::from_str(&body).map_err(|e| Error::Unifi(format!("unifi parse response: {e}")))
    }

    /// Never cached: the default window is the same URL every time, but the
    /// history behind it moves on.
    fn wan_history(&self, history_seconds: i64) -> Result<WanHistory> {
        let url = format!(
            "{}{}/v2/api/site/{}/aggregated-dashboard?historySeconds={history_seconds}&widgets=wan_history",
            self.base_url,
            self.prefix(),
            self.site
        );
        let body = self.fetch(&url)?;
        serde_json::from_str(&body).map_err(|e| Error::Unifi(format!("unifi parse response: {e}")))
    }

    fn dpi_categories(&self) -> Result<Vec<DpiEntry>> {
        self.dpi_list("dpi/categories")
    }

    fn dpi_applications(&self) -> Result<Vec<DpiEntry>> {
        self.dpi_list("dpi/applications")
    }
}

/// A failed re-login after a 401, keeping whether it was refused or unreachable.
fn reauth_error(e: Error) -> Error {
    match e {
        Error::Auth(m) => Error::Auth(format!("unifi re-auth: {m}")),
        other => Error::Unifi(format!("unifi re-auth: {other}")),
    }
}

/// reqwest's Display omits the cause ("error sending request"); append the
/// source chain so TLS and DNS failures are visible.
fn chain(e: &reqwest::Error) -> String {
    let mut msg = e.to_string();
    let mut source = std::error::Error::source(e);
    while let Some(s) = source {
        msg.push_str(": ");
        msg.push_str(&s.to_string());
        source = s.source();
    }
    msg
}

fn describe(e: &reqwest::Error) -> String {
    with_tls_hint(chain(e))
}

pub fn classify(e: &reqwest::Error) -> TransportError {
    classify_chain(&chain(e), e.is_timeout(), e.is_connect())
}

/// Sorts a failed request by the text of its error chain. rustls reports
/// `invalid peer certificate: <Reason>`; hyper reports `dns error: …`.
fn classify_chain(chain: &str, timeout: bool, connect: bool) -> TransportError {
    let lower = chain.to_lowercase();
    if lower.contains("certificate") {
        let reason = chain.rsplit(": ").next().unwrap_or(chain);
        TransportError::Certificate(reason.to_string())
    } else if timeout {
        TransportError::Timeout
    } else if lower.contains("dns error") {
        TransportError::Dns(chain.to_string())
    } else if connect {
        TransportError::Connect(chain.to_string())
    } else {
        TransportError::Other(chain.to_string())
    }
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
    use crate::unifi::types::LogQuery;

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
    fn health_reads_stat_health() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(GET)
                .path("/proxy/network/api/s/default/stat/health")
                .header("X-API-KEY", "k");
            then.status(200)
                .json_body(json!({"meta": {"rc": "ok"}, "data": [
                    {"subsystem": "wlan", "status": "ok", "num_ap": 2},
                    {"subsystem": "vpn", "status": "unknown"}
                ]}));
        });
        let h = api_key_client(&server).health().unwrap();
        m.assert();
        assert_eq!(h.len(), 2);
        assert_eq!(
            (h[0].subsystem.as_str(), h[0].status.as_str()),
            ("wlan", "ok")
        );
        assert_eq!(h[1].status, "unknown");
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
    fn unavailable_login_is_not_an_auth_error() {
        for status in [429, 502, 503] {
            let server = MockServer::start();
            server.mock(|when, then| {
                when.method(POST).path("/api/login");
                then.status(status);
            });
            let err = session_client(&server)
                .system_log(&LogQuery::default())
                .unwrap_err();
            assert!(matches!(err, Error::Unifi(_)), "{status}: {err:?}");
            assert_eq!(err.to_string(), format!("unifi login: status {status}"));
        }
    }

    #[test]
    fn reauth_failure_keeps_its_kind() {
        let auth = reauth_error(Error::Auth("unifi login: status 401".into()));
        assert!(matches!(auth, Error::Auth(_)));
        assert_eq!(auth.to_string(), "unifi re-auth: unifi login: status 401");
        let down = reauth_error(Error::Unifi("unifi login: connection refused".into()));
        assert!(matches!(down, Error::Unifi(_)));
        assert_eq!(
            down.to_string(),
            "unifi re-auth: unifi login: connection refused"
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

    fn sites_body() -> serde_json::Value {
        json!({"meta": {"rc": "ok"}, "data": [
            {"name": "default", "desc": "Default", "_id": "x"},
            {"name": "ab12cd", "desc": "Lab"}
        ]})
    }

    #[test]
    fn sites_with_api_key() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(GET)
                .path("/proxy/network/api/self/sites")
                .header("X-API-KEY", "k");
            then.status(200).json_body(sites_body());
        });
        let sites = api_key_client(&server).sites().unwrap();
        m.assert();
        assert_eq!(
            sites,
            vec![
                Site {
                    name: "default".into(),
                    desc: "Default".into()
                },
                Site {
                    name: "ab12cd".into(),
                    desc: "Lab".into()
                },
            ]
        );
    }

    #[test]
    fn sites_with_session_logs_in_first() {
        let server = MockServer::start();
        let login = server.mock(|when, then| {
            when.method(POST).path("/api/login");
            then.status(200)
                .header("Set-Cookie", "unifises=abc; Path=/")
                .json_body(json!({}));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/api/self/sites")
                .header("cookie", "unifises=abc");
            then.status(200).json_body(sites_body());
        });
        assert_eq!(session_client(&server).sites().unwrap().len(), 2);
        login.assert();
    }

    #[test]
    fn sites_errors_are_typed() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/proxy/network/api/self/sites");
            then.status(401);
        });
        assert_eq!(
            api_key_client(&server).sites(),
            Err(AccessError::Status(401))
        );

        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/api/login");
            then.status(400);
        });
        assert_eq!(
            session_client(&server).sites(),
            Err(AccessError::Login(400))
        );

        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/proxy/network/api/self/sites");
            then.status(200).body("<html>");
        });
        assert!(matches!(
            api_key_client(&server).sites(),
            Err(AccessError::Parse(_))
        ));
    }

    #[test]
    fn handshake_accepts_any_http_response() {
        let server = MockServer::start();
        assert_eq!(api_key_client(&server).handshake(), Ok(()));
        let c = UnifiClient::with_base_url(
            "http://127.0.0.1:1".into(),
            Auth::ApiKey("k".into()),
            "default".into(),
            false,
        )
        .unwrap();
        assert!(matches!(c.handshake(), Err(TransportError::Connect(_))));
    }

    #[test]
    fn classify_chain_kinds() {
        let pre = "error sending request for url (https://h/): client error (Connect)";
        assert_eq!(
            classify_chain(
                &format!("{pre}: invalid peer certificate: UnknownIssuer"),
                false,
                true
            ),
            TransportError::Certificate("UnknownIssuer".into())
        );
        assert!(matches!(
            classify_chain(
                &format!(
                    "{pre}: dns error: failed to lookup address information: Name or service not known"
                ),
                false,
                true
            ),
            TransportError::Dns(_)
        ));
        assert!(matches!(
            classify_chain(
                &format!("{pre}: tcp connect error: Connection refused (os error 111)"),
                false,
                true
            ),
            TransportError::Connect(_)
        ));
        assert_eq!(
            classify_chain("operation timed out", true, false),
            TransportError::Timeout
        );
        assert!(matches!(
            classify_chain("boom", false, false),
            TransportError::Other(_)
        ));
    }

    fn log_query() -> LogQuery {
        LogQuery {
            timestamp_from: Some(1000),
            timestamp_to: Some(2000),
            severities: vec!["HIGH", "VERY_HIGH"],
            categories: vec!["SECURITY"],
            page_size: 1000,
            page_number: 0,
        }
    }

    #[test]
    fn system_log_posts_query_and_is_never_cached() {
        let dir = tempfile::tempdir().unwrap();
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(POST)
                .path("/proxy/network/v2/api/site/default/system-log/all")
                .header("X-API-KEY", "k")
                .json_body(json!({
                    "timestampFrom": 1000,
                    "timestampTo": 2000,
                    "severities": ["HIGH", "VERY_HIGH"],
                    "categories": ["SECURITY"],
                    "pageSize": 1000,
                    "pageNumber": 0
                }));
            then.status(200).json_body(json!({
                "data": [{
                    "id": "e1",
                    "timestamp": 1500,
                    "severity": "HIGH",
                    "parameters": {
                        "PORT": {"id": 5, "name": 5},
                        "EMPTY": {"id": "x", "name": ""},
                        "GONE": null
                    }
                }],
                "page_number": 0,
                "total_element_count": 1,
                "total_page_count": 1
            }));
        });
        let c = api_key_client(&server).with_cache(cache_in(&dir, true));
        for _ in 0..2 {
            let page = c.system_log(&log_query()).unwrap();
            assert_eq!(page.total_page_count, 1);
            let e = &page.data[0];
            assert_eq!((e.id.as_str(), e.timestamp), ("e1", 1500));
            assert_eq!(e.parameters["PORT"].id().as_deref(), Some("5"));
            assert_eq!(e.parameters["PORT"].name().as_deref(), Some("5"));
            assert_eq!(e.parameters["EMPTY"].name(), None);
            assert_eq!(e.parameters["GONE"].id(), None);
        }
        m.assert_calls(2);
    }

    #[test]
    fn system_log_omits_empty_filters() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(POST)
                .path("/proxy/network/v2/api/site/default/system-log/all")
                .json_body(json!({"pageSize": 50, "pageNumber": 3}));
            then.status(200)
                .json_body(json!({"data": [], "total_page_count": 0}));
        });
        let q = LogQuery {
            page_size: 50,
            page_number: 3,
            ..Default::default()
        };
        assert!(
            api_key_client(&server)
                .system_log(&q)
                .unwrap()
                .data
                .is_empty()
        );
        m.assert();
    }

    #[test]
    fn system_log_session_relogs_once_then_is_auth_error() {
        let server = MockServer::start();
        let login = server.mock(|when, then| {
            when.method(POST).path("/api/login");
            then.status(200).json_body(json!({}));
        });
        let log = server.mock(|when, then| {
            when.method(POST).path("/v2/api/site/lab/system-log/all");
            then.status(401);
        });
        let err = session_client(&server)
            .system_log(&log_query())
            .unwrap_err();
        assert!(matches!(err, Error::Auth(_)), "{err:?}");
        assert_eq!(err.to_string(), "unifi request: unauthorized after re-auth");
        login.assert_calls(2);
        log.assert_calls(2);
    }

    #[test]
    fn invalid_api_key_and_refused_login_are_auth_errors() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST)
                .path("/proxy/network/v2/api/site/default/system-log/all");
            then.status(401);
        });
        let err = api_key_client(&server)
            .system_log(&log_query())
            .unwrap_err();
        assert!(matches!(err, Error::Auth(_)), "{err:?}");

        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/api/login");
            then.status(403);
        });
        let err = session_client(&server)
            .system_log(&log_query())
            .unwrap_err();
        assert!(matches!(err, Error::Auth(_)), "{err:?}");
        assert_eq!(err.to_string(), "unifi login: status 403");
    }

    #[test]
    fn system_log_server_error_is_not_auth() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST)
                .path("/proxy/network/v2/api/site/default/system-log/all");
            then.status(503);
        });
        let err = api_key_client(&server)
            .system_log(&log_query())
            .unwrap_err();
        assert!(matches!(err, Error::Unifi(_)), "{err:?}");
        assert_eq!(err.to_string(), "unifi request: status 503");
    }

    #[test]
    fn traffic_sends_window_and_decodes() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(GET)
                .path("/proxy/network/v2/api/site/default/traffic")
                .query_param("start", "1000")
                .query_param("end", "2000")
                .query_param("includeUnidentified", "true");
            then.status(200).json_body(json!({
                "client_usage_by_app": [{"client": {"mac": "aa"}, "usage_by_app": [{"category": 4, "application": 1, "total_bytes": 9}]}],
                "total_usage_by_app": []
            }));
        });
        let r = api_key_client(&server).traffic(1000, 2000).unwrap();
        m.assert();
        assert_eq!(r.client_usage_by_app[0].usage_by_app[0].total_bytes, 9);
    }

    #[test]
    fn wan_history_asks_for_the_widget_and_window() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(GET)
                .path("/proxy/network/v2/api/site/default/aggregated-dashboard")
                .query_param("historySeconds", "604800")
                .query_param("widgets", "wan_history");
            then.status(200).json_body(json!({
                "dashboard_meta": {"start_timestamp": 1000, "end_timestamp": 2000},
                "wan_history": {"wan_history_details": [{"network_group": "WAN", "uptime": 99.5}]}
            }));
        });
        let h = api_key_client(&server).wan_history(604800).unwrap();
        m.assert();
        assert_eq!(h.dashboard_meta.unwrap().start_timestamp, 1000);
        assert_eq!(
            h.wan_history.unwrap().wan_history_details[0].uptime,
            Some(99.5)
        );
    }

    #[test]
    fn wan_history_is_never_cached() {
        let dir = tempfile::tempdir().unwrap();
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(GET)
                .path("/proxy/network/v2/api/site/default/aggregated-dashboard");
            then.status(200).json_body(json!({}));
        });
        let client = api_key_client(&server).with_cache(Cache::new(
            dir.path().join("c"),
            Duration::from_secs(60),
            true,
        ));
        client.wan_history(604800).unwrap();
        client.wan_history(604800).unwrap();
        m.assert_calls(2);
    }

    #[test]
    fn dpi_reads_every_page() {
        let server = MockServer::start();
        let page = |offset: i64, ids: std::ops::Range<i64>| {
            let data: Vec<_> = ids
                .map(|i| json!({"id": i, "name": format!("App {i}")}))
                .collect();
            json!({"offset": offset, "limit": 200, "count": data.len(), "totalCount": 450, "data": data})
        };
        let mocks: Vec<_> = [(0, 0..200), (200, 200..400), (400, 400..450)]
            .into_iter()
            .map(|(offset, ids)| {
                let body = page(offset, ids);
                server.mock(move |when, then| {
                    when.method(GET)
                        .path("/proxy/network/integration/v1/dpi/applications")
                        .header("X-API-KEY", "k")
                        .query_param("offset", offset.to_string())
                        .query_param("limit", "200");
                    then.status(200).json_body(body);
                })
            })
            .collect();
        let apps = api_key_client(&server).dpi_applications().unwrap();
        for m in &mocks {
            m.assert();
        }
        assert_eq!(apps.len(), 450);
        assert_eq!(apps[0].id, 0);
        assert_eq!(apps[449].name, "App 449");
    }

    #[test]
    fn dpi_categories_use_their_own_path() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(GET)
                .path("/proxy/network/integration/v1/dpi/categories")
                .query_param("offset", "0");
            then.status(200).json_body(
                json!({"totalCount": 1, "data": [{"id": 4, "name": "Media streaming services"}]}),
            );
        });
        let cats = api_key_client(&server).dpi_categories().unwrap();
        m.assert();
        assert_eq!(
            cats,
            vec![DpiEntry {
                id: 4,
                name: "Media streaming services".into()
            }]
        );
    }

    #[test]
    fn dpi_paging_is_capped() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(GET)
                .path("/proxy/network/integration/v1/dpi/applications");
            then.status(200)
                .json_body(json!({"totalCount": 1_000_000_000_i64, "data": []}));
        });
        api_key_client(&server).dpi_applications().unwrap();
        m.assert_calls(MAX_DPI_PAGES);
    }

    #[test]
    fn session_auth_has_no_catalogue() {
        let server = MockServer::start();
        let any = server.mock(|when, then| {
            when.any_request();
            then.status(200).json_body(json!({}));
        });
        let err = session_client(&server).dpi_categories().unwrap_err();
        assert!(err.to_string().contains("needs an API key"), "{err}");
        assert!(session_client(&server).dpi_applications().is_err());
        any.assert_calls(0);
    }

    #[test]
    fn dpi_catalogue_is_cached_for_a_day() {
        let dir = tempfile::tempdir().unwrap();
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(GET)
                .path("/proxy/network/integration/v1/dpi/categories");
            then.status(200)
                .json_body(json!({"totalCount": 1, "data": [{"id": 0, "name": "IM"}]}));
        });
        let client = || {
            api_key_client(&server).with_cache(Cache::new(
                dir.path().join("c"),
                Duration::from_secs(5),
                true,
            ))
        };
        client().dpi_categories().unwrap();
        // Older than the 5 s response TTL, younger than a day.
        for entry in std::fs::read_dir(dir.path().join("c")).unwrap() {
            let f = std::fs::File::options()
                .write(true)
                .open(entry.unwrap().path())
                .unwrap();
            f.set_modified(std::time::SystemTime::now() - Duration::from_secs(60))
                .unwrap();
        }
        client().dpi_categories().unwrap();
        m.assert_calls(1);
    }

    #[test]
    fn traffic_is_never_cached() {
        let dir = tempfile::tempdir().unwrap();
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(GET)
                .path("/proxy/network/v2/api/site/default/traffic");
            then.status(200).json_body(json!({}));
        });
        let client = api_key_client(&server).with_cache(Cache::new(
            dir.path().join("c"),
            Duration::from_secs(60),
            true,
        ));
        client.traffic(0, 60_000).unwrap();
        client.traffic(0, 60_000).unwrap();
        m.assert_calls(2);
    }

    #[test]
    fn catalogue_failure_reason_reaches_the_view() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/proxy/network/v2/api/site/default/traffic");
            then.status(200).json_body(json!({}));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/proxy/network/integration/v1/dpi/categories");
            then.status(404);
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/proxy/network/integration/v1/dpi/applications");
            then.status(200)
                .json_body(json!({"totalCount": 0, "data": []}));
        });
        for list in ["active", "history"] {
            server.mock(|when, then| {
                when.method(GET)
                    .path(format!("/proxy/network/v2/api/site/default/clients/{list}"));
                then.status(200).json_body(json!([]));
            });
        }
        let svc = crate::service::Service::new(Box::new(api_key_client(&server)), 30);
        let w = crate::service::TrafficWindow {
            start_ms: 0,
            end_ms: 60_000,
        };
        let v = svc.apps_traffic(w).unwrap();
        assert_eq!(
            v.names_unavailable.as_deref(),
            Some("unifi request: status 404")
        );
    }
}
