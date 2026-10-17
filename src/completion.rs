//! Shell completion through clap_complete's dynamic protocol: the script that
//! `completions <shell>` prints calls back into `COMPLETE=<shell> unifictl`
//! on every TAB. Subcommands and flags come from the clap definition, IDs from
//! the controller (through the response cache, so repeated TABs are instant).

use std::path::PathBuf;
use std::time::Duration;

use clap::{CommandFactory, ValueEnum};
use clap_complete::engine::CompletionCandidate;
use clap_complete::env::{CompleteEnv, Shells};

use crate::app;
use crate::cli::{Cli, Shell};
use crate::error::{Error, Result};
use crate::model::DeviceType;
use crate::service::{ClientFilter, DeviceFilter, Service};

/// Environment variable that turns a run into a completion request.
const VAR: &str = "COMPLETE";

/// A TAB waiting on an unreachable controller should give up quickly.
const TIMEOUT: Duration = Duration::from_secs(3);

/// Answers the shell and exits if this run is a completion request; returns
/// otherwise.
pub fn handle_request() {
    CompleteEnv::with_factory(Cli::command).var(VAR).complete();
}

/// The registration script for `shell`, the same one `COMPLETE=<shell>
/// unifictl` prints.
pub fn script(shell: Shell) -> Result<String> {
    let value = shell.to_possible_value().expect("no skipped variants");
    let shells = Shells::builtins();
    let completer = shells
        .completer(value.get_name())
        .expect("every Shell is a clap_complete builtin");
    let mut buf = Vec::new();
    completer
        .write_registration(VAR, "unifictl", "unifictl", &completer_path(), &mut buf)
        .map_err(|e| Error::Other(format!("write completion script: {e}")))?;
    String::from_utf8(buf).map_err(|e| Error::Other(format!("completion script: {e}")))
}

/// The binary the script calls on TAB: this one, as it was invoked. A
/// relative path (`./target/debug/unifictl`) is made absolute; a bare name is
/// left to `PATH`.
fn completer_path() -> String {
    let argv0 = PathBuf::from(
        std::env::args_os()
            .next()
            .unwrap_or_else(|| "unifictl".into()),
    );
    let path = if argv0.components().count() > 1 {
        let rel = argv0.strip_prefix(".").unwrap_or(&argv0);
        std::env::current_dir().map_or(argv0.clone(), |dir| dir.join(rel))
    } else {
        argv0
    };
    path.to_string_lossy().into_owned()
}

pub fn devices() -> Vec<CompletionCandidate> {
    ids(|svc| {
        let devices = svc.list_devices(&DeviceFilter::default())?.items;
        Ok(devices
            .into_iter()
            .map(|d| (d.id, format!("{} ({})", d.name, d.device_type)))
            .collect())
    })
}

/// Devices that `ports list` covers: switches and gateways.
pub fn port_devices() -> Vec<CompletionCandidate> {
    ids(|svc| {
        let devices = svc.list_devices(&DeviceFilter::default())?.items;
        Ok(devices
            .into_iter()
            .filter(|d| matches!(d.device_type, DeviceType::Switch | DeviceType::Gateway))
            .map(|d| (d.id, format!("{} ({})", d.name, d.device_type)))
            .collect())
    })
}

/// Devices that `radios list` covers: access points.
pub fn access_points() -> Vec<CompletionCandidate> {
    ids(|svc| {
        let devices = svc.list_devices(&DeviceFilter::default())?.items;
        Ok(devices
            .into_iter()
            .filter(|d| d.device_type == DeviceType::AccessPoint)
            .map(|d| (d.id, d.name))
            .collect())
    })
}

pub fn clients() -> Vec<CompletionCandidate> {
    ids(|svc| {
        let clients = svc.list_clients(&ClientFilter::default())?.items;
        Ok(clients
            .into_iter()
            .map(|c| (c.id, format!("{} ({})", c.name, c.status)))
            .collect())
    })
}

pub fn vlans() -> Vec<CompletionCandidate> {
    ids(|svc| {
        Ok(svc
            .list_vlans()?
            .items
            .into_iter()
            .map(|v| (v.id, v.name))
            .collect())
    })
}

pub fn ssids() -> Vec<CompletionCandidate> {
    ids(|svc| {
        Ok(svc
            .list_ssids()?
            .items
            .into_iter()
            .map(|s| (s.id, s.name))
            .collect())
    })
}

pub fn wans() -> Vec<CompletionCandidate> {
    ids(|svc| {
        Ok(svc
            .list_wans()?
            .items
            .into_iter()
            .map(|w| (w.id, w.name))
            .collect())
    })
}

/// `(id, help)` pairs from `list` as candidates. Any failure (no config,
/// controller down) completes nothing: a TAB never prints an error.
fn ids(list: impl FnOnce(&Service) -> Result<Vec<(String, String)>>) -> Vec<CompletionCandidate> {
    let Ok(svc) = service() else {
        return Vec::new();
    };
    list(&svc)
        .unwrap_or_default()
        .into_iter()
        .map(|(id, help)| CompletionCandidate::new(id).help(Some(help.into())))
        .collect()
}

/// The completer cannot see `--config` or `--fixtures` on the line being
/// completed, so it uses the environment and the default config path.
fn service() -> Result<Service> {
    // Testing only, like --fixtures.
    #[cfg(feature = "fixtures")]
    if let Some(dir) = std::env::var_os("UNIFICTL_FIXTURES") {
        return app::fixture_service(std::path::Path::new(&dir));
    }
    app::controller_service(None, false, Some(TIMEOUT)).map(|(svc, _warnings)| svc)
}
