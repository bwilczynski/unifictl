use clap::Parser;
use serde::Serialize;

use crate::cli::{
    Cli, ClientsCommand, Command, DevicesCommand, LookupCommand, ModeFilter, OutputFormat,
    PortsCommand, StateFilter, StatusFilter,
};
use crate::config;
use crate::error::{Error, Result};
use crate::model::{ClientStatus, PortState, VlanMode};
use crate::render;
use crate::service::{PortFilter, Service};
use crate::unifi::client::UnifiClient;

/// Entry point used by `main.rs`; returns the process exit code.
pub fn main() -> i32 {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            let _ = e.print();
            return e.exit_code();
        }
    };
    match run(cli) {
        Ok(out) => write_stdout(&out),
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

/// Executes a parsed command and returns everything destined for stdout.
pub fn run(cli: Cli) -> Result<String> {
    if let Command::Version = cli.command {
        return Ok(format!("unifictl {}\n", env!("CARGO_PKG_VERSION")));
    }
    let svc = build_service(&cli)?;
    execute(&svc, &cli.command, cli.output)
}

fn build_service(cli: &Cli) -> Result<Service> {
    #[cfg(feature = "fixtures")]
    if let Some(dir) = &cli.fixtures {
        let api = crate::unifi::fixtures::FixtureApi::load(dir)?;
        return Ok(Service::new(
            Box::new(api),
            crate::service::DEFAULT_HISTORY_DAYS,
        ));
    }
    let env = |k: &str| std::env::var(k).ok();
    let path = config::resolve_path(cli.config.as_deref(), &env);
    let explicit = cli.config.is_some() || env("UNIFICTL_CONFIG").is_some_and(|v| !v.is_empty());
    let loaded = config::load(&path, explicit, &env)?;
    for w in &loaded.warnings {
        eprintln!("warning: {w}");
    }
    let client = UnifiClient::new(&loaded.config)?;
    Ok(Service::new(
        Box::new(client),
        loaded.config.client_history_days,
    ))
}

pub fn execute(svc: &Service, cmd: &Command, out: OutputFormat) -> Result<String> {
    match cmd {
        Command::Devices { command } => match command {
            DevicesCommand::List => emit(out, &svc.list_devices()?, render::devices::list),
            DevicesCommand::Get { id, all_ports } => emit(out, &svc.get_device(id)?, |d| {
                render::devices::get(d, *all_ports)
            }),
        },
        Command::Clients { command } => match command {
            ClientsCommand::List { status } => {
                let status = status.map(|s| match s {
                    StatusFilter::Online => ClientStatus::Online,
                    StatusFilter::Offline => ClientStatus::Offline,
                });
                emit(out, &svc.list_clients(status)?, render::clients::list)
            }
            ClientsCommand::Get { id } => emit(out, &svc.get_client(id)?, render::clients::get),
        },
        Command::Topology {
            include_clients,
            include_wireless,
        } => {
            let topo = svc.get_topology(*include_clients || *include_wireless)?;
            match out {
                OutputFormat::Json => json(&topo),
                OutputFormat::Table => render::topology::tree(&topo, *include_wireless),
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
                render::ports::list_table(l, args.wide)
            })
        }
        Command::Vlans { command } => match command {
            LookupCommand::List => emit(out, &svc.list_vlans()?, render::vlans::list),
            LookupCommand::Get { id } => emit(out, &svc.get_vlan(id)?, render::vlans::get),
        },
        Command::Ssids { command } => match command {
            LookupCommand::List => emit(out, &svc.list_ssids()?, render::ssids::list),
            LookupCommand::Get { id } => emit(out, &svc.get_ssid(id)?, render::ssids::get),
        },
        Command::Wans { command } => match command {
            LookupCommand::List => emit(out, &svc.list_wans()?, render::wans::list),
            LookupCommand::Get { id } => emit(out, &svc.get_wan(id)?, render::wans::get),
        },
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
    }
}

fn json<T: Serialize>(value: &T) -> Result<String> {
    let mut s = serde_json::to_string_pretty(value)
        .map_err(|e| Error::Other(format!("encode JSON: {e}")))?;
    s.push('\n');
    Ok(s)
}
