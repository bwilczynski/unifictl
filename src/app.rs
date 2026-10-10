use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::Parser;
use serde::Serialize;

use crate::cache::Cache;
use crate::cli::{
    Cli, ClientsCommand, ColorMode, Command, ConfigCommand, DevicesCommand, LookupCommand,
    ModeFilter, OutputFormat, PortsCommand, StateFilter, StatusFilter,
};
use crate::completion;
use crate::config;
use crate::error::{Error, Result};
use crate::model::{ClientStatus, Ids, PortState, VlanMode};
use crate::render;
use crate::service::{PortFilter, Service};
use crate::setup;
use crate::unifi::client::UnifiClient;

/// Entry point used by `main.rs`; returns the process exit code.
pub fn main() -> i32 {
    completion::handle_request();
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            let _ = e.print();
            return e.exit_code();
        }
    };
    match run(cli) {
        Ok(out) => match write_stdout(&out.stdout) {
            0 => out.code,
            code => code,
        },
        Err(e) => {
            eprintln!("Error: {e}");
            1
        }
    }
}

/// A reader that went away (`| head`, `| grep -q`) is not an error.
fn write_stdout(out: &str) -> i32 {
    use std::io::{ErrorKind, Write};
    let mut stdout = std::io::stdout().lock();
    match stdout
        .write_all(out.as_bytes())
        .and_then(|()| stdout.flush())
    {
        Ok(()) => 0,
        Err(e) if e.kind() == ErrorKind::BrokenPipe => 0,
        Err(e) => {
            eprintln!("Error: write output: {e}");
            1
        }
    }
}

/// What a command prints on stdout and the exit code that follows it.
pub struct Outcome {
    pub stdout: String,
    pub code: i32,
}

impl Outcome {
    fn ok(stdout: String) -> Self {
        Self { stdout, code: 0 }
    }
}

/// Executes a parsed command.
pub fn run(cli: Cli) -> Result<Outcome> {
    let env = |k: &str| std::env::var(k).ok();
    match &cli.command {
        Command::Version => {
            return Ok(Outcome::ok(format!(
                "unifictl {}\n",
                env!("CARGO_PKG_VERSION")
            )));
        }
        Command::Completions { shell } => return completion::script(*shell).map(Outcome::ok),
        _ => {}
    }
    if cli.output == OutputFormat::Ids && !lists_ids(&cli.command) {
        return Err(ids_unsupported());
    }
    let color = color_enabled(cli.color, cli.output, std::io::stdout().is_terminal(), &env);
    if let Command::Config { command } = &cli.command {
        return run_config(command, &cli, &env, color);
    }
    let svc = build_service(&cli)?;
    execute(&svc, &cli.command, cli.output, color).map(Outcome::ok)
}

/// Tables only; under `auto`, only on a terminal and with `NO_COLOR` unset or
/// empty (no-color.org).
fn color_enabled(
    mode: ColorMode,
    out: OutputFormat,
    is_tty: bool,
    env: &dyn Fn(&str) -> Option<String>,
) -> bool {
    if out != OutputFormat::Table {
        return false;
    }
    match mode {
        ColorMode::Always => true,
        ColorMode::Never => false,
        ColorMode::Auto => is_tty && env("NO_COLOR").is_none_or(|v| v.is_empty()),
    }
}

/// The config file in effect, and whether the user named it.
fn config_location(
    config_flag: Option<&Path>,
    env: &dyn Fn(&str) -> Option<String>,
) -> (PathBuf, bool) {
    let path = config::resolve_path(config_flag, env);
    let explicit = config_flag.is_some() || env("UNIFICTL_CONFIG").is_some_and(|v| !v.is_empty());
    (path, explicit)
}

fn run_config(
    cmd: &ConfigCommand,
    cli: &Cli,
    env: &dyn Fn(&str) -> Option<String>,
    color: bool,
) -> Result<Outcome> {
    let (path, explicit) = config_location(cli.config.as_deref(), env);
    if let ConfigCommand::Init(args) = cmd {
        setup::init::run(args, &path, env, &mut setup::prompt::TerminalPrompter)?;
        if args.no_check {
            return Ok(Outcome::ok(String::new()));
        }
    }
    let report = setup::check::run(&path, explicit, env);
    let code = if report.ok { 0 } else { 1 };
    Ok(Outcome {
        stdout: emit(cli.output, &report, |r| render::check::table(r, color))?,
        code,
    })
}

fn build_service(cli: &Cli) -> Result<Service> {
    #[cfg(feature = "fixtures")]
    if let Some(dir) = &cli.fixtures {
        return fixture_service(dir);
    }
    let (svc, warnings) = controller_service(cli.config.as_deref(), cli.refresh, None)?;
    for w in &warnings {
        eprintln!("warning: {w}");
    }
    Ok(svc)
}

#[cfg(feature = "fixtures")]
pub(crate) fn fixture_service(dir: &Path) -> Result<Service> {
    let api = crate::unifi::fixtures::FixtureApi::load(dir)?;
    Ok(Service::new(
        Box::new(api),
        crate::service::DEFAULT_HISTORY_DAYS,
    ))
}

/// A service for the configured controller, and the config warnings for the
/// caller to show or not. `timeout` overrides the default request timeout.
pub(crate) fn controller_service(
    config_flag: Option<&Path>,
    refresh: bool,
    timeout: Option<Duration>,
) -> Result<(Service, Vec<String>)> {
    let env = |k: &str| std::env::var(k).ok();
    let (path, explicit) = config_location(config_flag, &env);
    let loaded = config::load(&path, explicit, &env)?;
    let mut client = UnifiClient::new(&loaded.config)?;
    if let Some(timeout) = timeout {
        client = client.with_timeout(timeout);
    }
    if !loaded.config.cache_ttl.is_zero() {
        client = client.with_cache(Cache::new(
            config::resolve_cache_dir(&env),
            loaded.config.cache_ttl,
            !refresh,
        ));
    }
    let svc = Service::new(Box::new(client), loaded.config.client_history_days);
    Ok((svc, loaded.warnings))
}

pub fn execute(svc: &Service, cmd: &Command, out: OutputFormat, color: bool) -> Result<String> {
    match cmd {
        Command::Devices { command } => match command {
            DevicesCommand::List => emit_list(out, &svc.list_devices()?, |l| {
                render::devices::list(l, color)
            }),
            DevicesCommand::Get { id, all_ports } => emit(out, &svc.get_device(id)?, |d| {
                render::devices::get(d, *all_ports, color)
            }),
        },
        Command::Clients { command } => match command {
            ClientsCommand::List { status } => {
                let status = status.map(|s| match s {
                    StatusFilter::Online => ClientStatus::Online,
                    StatusFilter::Offline => ClientStatus::Offline,
                });
                emit_list(out, &svc.list_clients(status)?, |l| {
                    render::clients::list(l, color)
                })
            }
            ClientsCommand::Get { id } => emit(out, &svc.get_client(id)?, |c| {
                render::clients::get(c, color)
            }),
        },
        Command::Topology {
            include_clients,
            include_wireless,
        } => {
            let topo = svc.get_topology(*include_clients || *include_wireless)?;
            match out {
                OutputFormat::Json => json(&topo),
                OutputFormat::Ids => Err(ids_unsupported()),
                OutputFormat::Table => render::topology::tree(&topo, *include_wireless, color),
            }
        }
        Command::Ports {
            command: PortsCommand::List(args),
        } => {
            let filter = PortFilter {
                device_id: args.device.clone(),
                mode: args.mode.map(|m| match m {
                    ModeFilter::Trunk => VlanMode::Trunk,
                    ModeFilter::Access => VlanMode::Access,
                }),
                // hlctl defaults to up-only unless --state or --all-ports is given.
                state: if args.all_ports {
                    None
                } else {
                    Some(match args.state {
                        Some(StateFilter::Down) => PortState::Down,
                        Some(StateFilter::Disabled) => PortState::Disabled,
                        Some(StateFilter::Up) | None => PortState::Up,
                    })
                },
                vlan_id: (args.vlan > 0).then_some(args.vlan),
            };
            emit(out, &svc.list_ports(&filter)?, |l| {
                render::ports::list_table(l, args.wide, color)
            })
        }
        Command::Vlans { command } => match command {
            LookupCommand::List => emit_list(out, &svc.list_vlans()?, render::vlans::list),
            LookupCommand::Get { id } => emit(out, &svc.get_vlan(id)?, render::vlans::get),
        },
        Command::Ssids { command } => match command {
            LookupCommand::List => emit_list(out, &svc.list_ssids()?, render::ssids::list),
            LookupCommand::Get { id } => emit(out, &svc.get_ssid(id)?, render::ssids::get),
        },
        Command::Wans { command } => match command {
            LookupCommand::List => {
                emit_list(out, &svc.list_wans()?, |l| render::wans::list(l, color))
            }
            LookupCommand::Get { id } => {
                emit(out, &svc.get_wan(id)?, |w| render::wans::get(w, color))
            }
        },
        Command::Config { .. } => unreachable!("config commands are handled by run"),
        Command::Completions { shell } => completion::script(*shell),
        Command::Version => Ok(format!("unifictl {}\n", env!("CARGO_PKG_VERSION"))),
    }
}

fn emit<T: Serialize>(
    out: OutputFormat,
    value: &T,
    table: impl FnOnce(&T) -> String,
) -> Result<String> {
    match out {
        OutputFormat::Json => json(value),
        OutputFormat::Table => Ok(table(value)),
        OutputFormat::Ids => Err(ids_unsupported()),
    }
}

/// `emit` for lists, which can also print just their IDs.
fn emit_list<T: Serialize + Ids>(
    out: OutputFormat,
    value: &T,
    table: impl FnOnce(&T) -> String,
) -> Result<String> {
    if out == OutputFormat::Ids {
        return Ok(value.ids().iter().map(|id| format!("{id}\n")).collect());
    }
    emit(out, value, table)
}

fn ids_unsupported() -> Error {
    Error::Other("-o ids is only supported by devices, clients, vlans, ssids and wans list".into())
}

/// Commands whose output is a list of items with IDs.
fn lists_ids(cmd: &Command) -> bool {
    matches!(
        cmd,
        Command::Devices {
            command: DevicesCommand::List
        } | Command::Clients {
            command: ClientsCommand::List { .. }
        } | Command::Vlans {
            command: LookupCommand::List
        } | Command::Ssids {
            command: LookupCommand::List
        } | Command::Wans {
            command: LookupCommand::List
        }
    )
}

fn json<T: Serialize>(value: &T) -> Result<String> {
    let mut s = serde_json::to_string_pretty(value)
        .map_err(|e| Error::Other(format!("encode JSON: {e}")))?;
    s.push('\n');
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_with(no_color: Option<&'static str>) -> impl Fn(&str) -> Option<String> {
        move |k| {
            (k == "NO_COLOR")
                .then_some(no_color)
                .flatten()
                .map(String::from)
        }
    }

    #[test]
    fn auto_colours_only_a_terminal() {
        let none = env_with(None);
        assert!(color_enabled(
            ColorMode::Auto,
            OutputFormat::Table,
            true,
            &none
        ));
        assert!(!color_enabled(
            ColorMode::Auto,
            OutputFormat::Table,
            false,
            &none
        ));
    }

    #[test]
    fn non_empty_no_color_disables_auto_but_empty_does_not() {
        assert!(!color_enabled(
            ColorMode::Auto,
            OutputFormat::Table,
            true,
            &env_with(Some("1"))
        ));
        assert!(color_enabled(
            ColorMode::Auto,
            OutputFormat::Table,
            true,
            &env_with(Some(""))
        ));
    }

    #[test]
    fn always_and_never_ignore_terminal_and_no_color() {
        let set = env_with(Some("1"));
        assert!(color_enabled(
            ColorMode::Always,
            OutputFormat::Table,
            false,
            &set
        ));
        assert!(!color_enabled(
            ColorMode::Never,
            OutputFormat::Table,
            true,
            &env_with(None)
        ));
    }

    #[test]
    fn json_is_never_coloured() {
        assert!(!color_enabled(
            ColorMode::Always,
            OutputFormat::Json,
            true,
            &env_with(None)
        ));
    }
}
