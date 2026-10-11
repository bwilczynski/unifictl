use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::Parser;
use jiff::tz::TimeZone;
use jiff::{Timestamp, Zoned};
use serde::Serialize;

use crate::cache::Cache;
use crate::cli::{
    CategoryArg, Cli, ClientsCommand, ColorMode, Command, ConfigCommand, DevicesCommand, LogsArgs,
    LookupCommand, ModeFilter, OutputFormat, PortsCommand, PriorityArg, StateFilter, StatusFilter,
    TrafficArgs,
};
use crate::completion;
use crate::config;
use crate::error::{Error, Result};
use crate::model::{ClientStatus, Ids, LogEntry, LogSeverity, PortState, VlanMode};
use crate::render;
use crate::service::{LogFilter, PortFilter, Service, TrafficView, TrafficWindow, follow_loop};
use crate::setup;
use crate::timespec;
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
    if let Command::Completions { shell } = &cli.command {
        return completion::script(*shell).map(Outcome::ok);
    }
    if cli.output == OutputFormat::Ids && !lists_ids(&cli.command) {
        return Err(ids_unsupported());
    }
    let color = color_enabled(cli.color, cli.output, std::io::stdout().is_terminal(), &env);
    if let Command::Config { command } = &cli.command {
        return run_config(command, &cli, &env, color);
    }
    let svc = build_service(&cli)?;
    if let Command::Logs(args) = &cli.command {
        run_logs(&svc, args, cli.output, color, &env)?;
        return Ok(Outcome::ok(String::new()));
    }
    if let Command::Clients {
        command: ClientsCommand::Traffic(args),
    } = &cli.command
    {
        return run_traffic(&svc, args, cli.output, color, &env).map(Outcome::ok);
    }
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

/// `logs` writes as it goes rather than returning one string, so `--follow`
/// can stream and a closed pipe ends it quietly.
fn run_logs(
    svc: &Service,
    args: &LogsArgs,
    out: OutputFormat,
    color: bool,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<()> {
    let tz = TimeZone::system();
    let now = now(env).to_zoned(tz.clone());
    let at = |when: &Option<String>| {
        when.as_deref()
            .map(|w| timespec::parse(w, &now).map(|t| t.as_millisecond()))
            .transpose()
    };
    let (since_ms, until_ms) = (at(&args.since)?, at(&args.until)?);
    if let (Some(since), Some(until)) = (since_ms, until_ms)
        && since > until
    {
        return Err(Error::Other("--since is after --until".into()));
    }
    let lines = args.lines.or(default_lines(
        args.follow,
        since_ms.is_some() || until_ms.is_some(),
    ));
    let filter = LogFilter {
        since_ms,
        until_ms,
        min_severity: args.priority.map(|p| match p {
            PriorityArg::Low => LogSeverity::Low,
            PriorityArg::Medium => LogSeverity::Medium,
            PriorityArg::High => LogSeverity::High,
            PriorityArg::VeryHigh => LogSeverity::VeryHigh,
        }),
        categories: args
            .category
            .iter()
            .map(|c| CategoryArg::controller(*c))
            .collect(),
        devices: args.device.clone(),
        clients: args.client.clone(),
        lines,
        newest_first: args.reverse,
        with_refs: out == OutputFormat::Json,
    };
    let page = svc.logs(&filter, now.timestamp().as_millisecond(), &tz)?;

    let mut stdout = std::io::stdout().lock();
    let mut emit = |entries: &[LogEntry]| -> Result<bool> {
        let mut buf = String::new();
        for e in entries {
            match out {
                OutputFormat::Json => buf.push_str(&render::logs::json_line(e)?),
                _ => buf.push_str(&render::logs::line(e, &tz, color)),
            }
        }
        write_flush(&mut stdout, &buf)
    };
    if !args.follow {
        emit(&page.entries)?;
        return Ok(());
    }
    let (first, mut follower) = page.follow(filter);
    if !emit(&first)? {
        return Ok(());
    }
    follow_loop(
        || follower.poll(svc, now_ms(env), &tz),
        emit,
        std::thread::sleep,
        |w| eprintln!("warning: {w}"),
    )
}

/// How many entries `logs` shows without `-n`: the whole window when one is
/// given (even with `-f`, as journalctl), else 10 when following, else 50.
fn default_lines(follow: bool, window: bool) -> Option<usize> {
    if window {
        None
    } else if follow {
        Some(10)
    } else {
        Some(50)
    }
}

/// `clients traffic`: needs the clock for its window, and notes on stderr
/// when application names are unavailable.
fn run_traffic(
    svc: &Service,
    args: &TrafficArgs,
    out: OutputFormat,
    color: bool,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<String> {
    let now = now(env).to_zoned(TimeZone::system());
    let w = traffic_window(args.since.as_deref(), args.until.as_deref(), &now)?;
    fn noted<T>(view: TrafficView<T>) -> T {
        if let Some(reason) = &view.names_unavailable {
            eprintln!("note: application names unavailable ({reason})");
        }
        view.data
    }
    if let Some(id) = &args.id {
        emit(out, &noted(svc.client_traffic(id, w)?), |l| {
            render::traffic::client(l, color)
        })
    } else if args.by_app {
        emit(out, &noted(svc.apps_traffic(w)?), |l| {
            render::traffic::apps(l, color)
        })
    } else {
        emit_list(out, &noted(svc.clients_traffic(w)?), |l| {
            render::traffic::clients(l, color)
        })
    }
}

/// The `--since`/`--until` window: `--until` defaults to now, `--since` to a
/// day before it.
fn traffic_window(since: Option<&str>, until: Option<&str>, now: &Zoned) -> Result<TrafficWindow> {
    const DAY_MS: i64 = 24 * 60 * 60 * 1000;
    let end_ms = match until {
        Some(u) => timespec::parse(u, now)?,
        None => now.timestamp(),
    }
    .as_millisecond();
    let start_ms = match since {
        Some(s) => timespec::parse(s, now)?.as_millisecond(),
        None => end_ms - DAY_MS,
    };
    if start_ms > end_ms {
        return Err(Error::Other("--since is after --until".into()));
    }
    Ok(TrafficWindow { start_ms, end_ms })
}

/// Writes and flushes `s`; `false` when the reader has gone away.
fn write_flush(w: &mut impl std::io::Write, s: &str) -> Result<bool> {
    use std::io::ErrorKind;
    match w.write_all(s.as_bytes()).and_then(|()| w.flush()) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == ErrorKind::BrokenPipe => Ok(false),
        Err(e) => Err(Error::Other(format!("write output: {e}"))),
    }
}

/// The current time. Fixture builds read `UNIFICTL_NOW` (RFC 3339) so tests
/// get a fixed clock.
fn now(env: &dyn Fn(&str) -> Option<String>) -> Timestamp {
    #[cfg(feature = "fixtures")]
    if let Some(t) = env("UNIFICTL_NOW").and_then(|v| v.parse().ok()) {
        return t;
    }
    #[cfg(not(feature = "fixtures"))]
    let _ = env;
    Timestamp::now()
}

fn now_ms(env: &dyn Fn(&str) -> Option<String>) -> i64 {
    now(env).as_millisecond()
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
            DevicesCommand::List { wide } => emit_list(out, &svc.list_devices()?, |l| {
                render::devices::list(l, *wide, color)
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
            ClientsCommand::Traffic(_) => unreachable!("traffic is handled by run"),
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
                vlan_id: args.vlan.map(i64::from),
            };
            emit(out, &svc.list_ports(&filter)?, |l| {
                render::ports::list_table(l, args.wide, color)
            })
        }
        Command::Vlans { command } => match command {
            LookupCommand::List => {
                emit_list(out, &svc.list_vlans()?, |l| render::vlans::list(l, color))
            }
            LookupCommand::Get { id } => {
                emit(out, &svc.get_vlan(id)?, |v| render::vlans::get(v, color))
            }
        },
        Command::Ssids { command } => match command {
            LookupCommand::List => {
                emit_list(out, &svc.list_ssids()?, |l| render::ssids::list(l, color))
            }
            LookupCommand::Get { id } => {
                emit(out, &svc.get_ssid(id)?, |s| render::ssids::get(s, color))
            }
        },
        Command::Wans { command } => match command {
            LookupCommand::List => {
                emit_list(out, &svc.list_wans()?, |l| render::wans::list(l, color))
            }
            LookupCommand::Get { id } => {
                emit(out, &svc.get_wan(id)?, |w| render::wans::get(w, color))
            }
        },
        Command::Logs(_) => unreachable!("logs streams its output from run"),
        Command::Config { .. } => unreachable!("config commands are handled by run"),
        Command::Completions { shell } => completion::script(*shell),
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
    Error::Other("-o ids is only supported by devices, clients, vlans, ssids and wans list, and clients traffic".into())
}

/// Commands whose output is a list of items with IDs.
fn lists_ids(cmd: &Command) -> bool {
    matches!(
        cmd,
        Command::Devices {
            command: DevicesCommand::List { .. }
        } | Command::Clients {
            command: ClientsCommand::List { .. }
                | ClientsCommand::Traffic(TrafficArgs {
                    id: None,
                    by_app: false,
                    ..
                })
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
    fn logs_default_lines_like_journalctl() {
        assert_eq!(default_lines(false, false), Some(50));
        assert_eq!(default_lines(false, true), None);
        assert_eq!(default_lines(true, false), Some(10));
        // journalctl -f --since shows the whole window, then follows.
        assert_eq!(default_lines(true, true), None);
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

    fn noon() -> jiff::Zoned {
        "2026-10-11T12:00:30Z"
            .parse::<Timestamp>()
            .unwrap()
            .to_zoned(TimeZone::UTC)
    }

    fn ms(s: &str) -> i64 {
        s.parse::<Timestamp>().unwrap().as_millisecond()
    }

    #[test]
    fn traffic_window_defaults_to_the_last_day() {
        let w = traffic_window(None, None, &noon()).unwrap();
        assert_eq!(w.end_ms, ms("2026-10-11T12:00:30Z"));
        assert_eq!(w.start_ms, ms("2026-10-10T12:00:30Z"));
    }

    #[test]
    fn traffic_window_until_alone_reaches_back_a_day() {
        let w = traffic_window(None, Some("2026-10-05 08:00"), &noon()).unwrap();
        assert_eq!(w.end_ms, ms("2026-10-05T08:00:00Z"));
        assert_eq!(w.start_ms, ms("2026-10-04T08:00:00Z"));
    }

    #[test]
    fn traffic_window_since_and_until() {
        let w = traffic_window(Some("-2h"), Some("now"), &noon()).unwrap();
        assert_eq!(w.start_ms, ms("2026-10-11T10:00:30Z"));
        assert_eq!(w.end_ms, ms("2026-10-11T12:00:30Z"));
    }

    #[test]
    fn traffic_window_since_after_until_is_an_error() {
        let err = traffic_window(Some("today"), Some("yesterday"), &noon()).unwrap_err();
        assert_eq!(err.to_string(), "--since is after --until");
    }
}
