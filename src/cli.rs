use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use clap_complete::engine::ArgValueCandidates;

use crate::completion;

#[derive(Debug, Parser)]
#[command(
    name = "unifictl",
    version,
    about = "Command-line client for UniFi network controllers"
)]
pub struct Cli {
    /// Output format
    #[arg(short, long, global = true, value_enum, default_value_t = OutputFormat::Table)]
    pub output: OutputFormat,

    /// When to colour status cells in tables
    #[arg(long, global = true, value_enum, value_name = "WHEN", default_value_t = ColorMode::Auto)]
    pub color: ColorMode,

    /// Ignore cached controller responses and fetch fresh ones
    #[arg(long, global = true)]
    pub refresh: bool,

    /// Config file (default: ~/.config/unifictl/config.toml)
    #[arg(long, global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// Read controller data from fixture files instead of a controller (testing only)
    #[cfg(feature = "fixtures")]
    #[arg(long, global = true, hide = true, value_name = "DIR")]
    pub fixtures: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    Table,
    Json,
    /// One ID per line (devices, clients, vlans, ssids and wans list)
    Ids,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ColorMode {
    /// When stdout is a terminal and NO_COLOR is unset or empty
    Auto,
    Always,
    Never,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Managed devices: gateways, switches, access points
    Devices {
        #[command(subcommand)]
        command: DevicesCommand,
    },
    /// Network clients
    Clients {
        #[command(subcommand)]
        command: ClientsCommand,
    },
    /// Show the network as a tree rooted at the gateway
    Topology {
        /// Include wired clients in the topology
        #[arg(long)]
        include_clients: bool,
        /// Also include wireless clients (implies --include-clients)
        #[arg(long)]
        include_wireless: bool,
    },
    /// Switch and gateway ports
    Ports {
        #[command(subcommand)]
        command: PortsCommand,
    },
    /// VLANs (LAN networks)
    #[command(mut_subcommand("get", |get| get.mut_arg("id", |id| id.add(ArgValueCandidates::new(completion::vlans)))))]
    Vlans {
        #[command(subcommand)]
        command: LookupCommand,
    },
    /// Wireless networks
    #[command(mut_subcommand("get", |get| get.mut_arg("id", |id| id.add(ArgValueCandidates::new(completion::ssids)))))]
    Ssids {
        #[command(subcommand)]
        command: LookupCommand,
    },
    /// WAN uplinks
    #[command(mut_subcommand("get", |get| get.mut_arg("id", |id| id.add(ArgValueCandidates::new(completion::wans)))))]
    Wans {
        #[command(subcommand)]
        command: LookupCommand,
    },
    /// The controller's system log, like journalctl
    Logs(LogsArgs),
    /// Create the config file, or check it and the controller connection
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Print a shell completion script
    Completions {
        #[arg(value_enum)]
        shell: Shell,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
    Powershell,
    Elvish,
}

#[derive(Debug, Subcommand)]
pub enum DevicesCommand {
    /// List devices
    List {
        /// Show additional columns (MAC, model, firmware, uptime, uplink, traffic)
        #[arg(long)]
        wide: bool,
    },
    /// Show one device
    Get {
        /// Device ID, as shown by `devices list`
        #[arg(add = ArgValueCandidates::new(completion::devices))]
        id: String,
        /// Show all ports (default: active ports only)
        #[arg(long)]
        all_ports: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum ClientsCommand {
    /// List clients (online and recently offline)
    List {
        /// Filter by status
        #[arg(long, value_enum)]
        status: Option<StatusFilter>,
    },
    /// Show one client
    Get {
        /// Client ID, as shown by `clients list`
        #[arg(add = ArgValueCandidates::new(completion::clients))]
        id: String,
    },
    /// Internet traffic per client, or by application
    Traffic(TrafficArgs),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum StatusFilter {
    Online,
    Offline,
}

#[derive(Debug, Args)]
pub struct TrafficArgs {
    /// Client ID: its traffic by application
    #[arg(add = ArgValueCandidates::new(completion::clients), conflicts_with = "by_app")]
    pub id: Option<String>,
    /// Traffic by application across the site
    #[arg(long)]
    pub by_app: bool,
    /// Traffic at or after WHEN [default: 24h before --until]: now, today, yesterday, -2h, "2h ago", YYYY-MM-DD [HH:MM[:SS]]
    #[arg(short = 'S', long, value_name = "WHEN", allow_hyphen_values = true)]
    pub since: Option<String>,
    /// Traffic at or before WHEN [default: now] (same forms as --since)
    #[arg(short = 'U', long, value_name = "WHEN", allow_hyphen_values = true)]
    pub until: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum PortsCommand {
    /// List ports across all switches and gateways
    List(PortsListArgs),
}

#[derive(Debug, Args)]
pub struct PortsListArgs {
    /// Filter by device ID (switch or gateway)
    #[arg(long, add = ArgValueCandidates::new(completion::port_devices))]
    pub device: Option<String>,
    /// Filter by VLAN mode
    #[arg(long, value_enum)]
    pub mode: Option<ModeFilter>,
    /// Filter by port state
    #[arg(long, value_enum, conflicts_with = "all_ports")]
    pub state: Option<StateFilter>,
    /// Filter to ports carrying this VLAN ID
    #[arg(long, value_parser = clap::value_parser!(u16).range(1..=4094))]
    pub vlan: Option<u16>,
    /// Show additional columns (link speed, PoE, LAG, SFP, traffic)
    #[arg(long)]
    pub wide: bool,
    /// Show all ports (overrides the default state=up filter)
    #[arg(long)]
    pub all_ports: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ModeFilter {
    Trunk,
    Access,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum StateFilter {
    Up,
    Down,
    Disabled,
}

#[derive(Debug, Args)]
pub struct LogsArgs {
    /// Show the N most recent entries [default: 50; all with --since/--until; else 10 with --follow]
    #[arg(short = 'n', long, value_name = "N")]
    pub lines: Option<usize>,
    /// Entries at or after WHEN: now, today, yesterday, -2h, "2h ago", YYYY-MM-DD [HH:MM[:SS]]
    #[arg(short = 'S', long, value_name = "WHEN", allow_hyphen_values = true)]
    pub since: Option<String>,
    /// Entries at or before WHEN (same forms as --since)
    #[arg(
        short = 'U',
        long,
        value_name = "WHEN",
        allow_hyphen_values = true,
        conflicts_with = "follow"
    )]
    pub until: Option<String>,
    /// Only entries of this severity and above
    #[arg(short = 'p', long, value_enum, value_name = "LEVEL")]
    pub priority: Option<PriorityArg>,
    /// Newest entries first
    #[arg(short = 'r', long, conflicts_with = "follow")]
    pub reverse: bool,
    /// Keep running and print new entries as they arrive
    #[arg(short = 'f', long)]
    pub follow: bool,
    /// Only these categories (repeatable or comma-separated)
    #[arg(long, value_enum, value_delimiter = ',', value_name = "CAT")]
    pub category: Vec<CategoryArg>,
    /// Only entries that mention this device (repeatable)
    #[arg(long, value_name = "ID", add = ArgValueCandidates::new(completion::devices))]
    pub device: Vec<String>,
    /// Only entries that mention this client (repeatable)
    #[arg(long, value_name = "ID", add = ArgValueCandidates::new(completion::clients))]
    pub client: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum PriorityArg {
    Low,
    Medium,
    High,
    VeryHigh,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CategoryArg {
    Security,
    UnifiDevices,
    SoftwareUpdates,
    Vpn,
    Power,
    UnifiEthernetPorts,
    ClientDevices,
    Audit,
    InternetAndWan,
    Unknown,
}

impl CategoryArg {
    /// The controller's name for the category.
    pub fn controller(self) -> &'static str {
        match self {
            Self::Security => "SECURITY",
            Self::UnifiDevices => "UNIFI_DEVICES",
            Self::SoftwareUpdates => "SOFTWARE_UPDATES",
            Self::Vpn => "VPN",
            Self::Power => "POWER",
            Self::UnifiEthernetPorts => "UNIFI_ETHERNET_PORTS",
            Self::ClientDevices => "CLIENT_DEVICES",
            Self::Audit => "AUDIT",
            Self::InternetAndWan => "INTERNET_AND_WAN",
            Self::Unknown => "UNKNOWN",
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum LookupCommand {
    /// List all
    List,
    /// Show one
    Get {
        /// ID, as shown by `list`
        id: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Create the config file, prompting for anything not given as a flag
    Init(InitArgs),
    /// Check the config and the connection to the controller, step by step
    Check,
}

#[derive(Debug, Default, Args)]
pub struct InitArgs {
    /// Controller host or host:port (always reached over https)
    #[arg(long)]
    pub host: Option<String>,
    /// API key (UniFi OS); defaults to $UNIFI_API_KEY
    #[arg(long, conflicts_with_all = ["username", "password"])]
    pub api_key: Option<String>,
    /// Username for a self-hosted Network application
    #[arg(long)]
    pub username: Option<String>,
    /// Password for --username; defaults to $UNIFI_PASSWORD
    #[arg(long, requires = "username")]
    pub password: Option<String>,
    /// Site name (default: picked from the controller's sites)
    #[arg(long)]
    pub site: Option<String>,
    /// Accept the controller's self-signed certificate
    #[arg(long)]
    pub insecure_tls: bool,
    /// Overwrite an existing config file
    #[arg(long)]
    pub force: bool,
    /// Do not contact the controller: skip the site lookup and the final check
    #[arg(long)]
    pub no_check: bool,
}
