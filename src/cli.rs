use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

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
    Vlans {
        #[command(subcommand)]
        command: LookupCommand,
    },
    /// Wireless networks
    Ssids {
        #[command(subcommand)]
        command: LookupCommand,
    },
    /// WAN uplinks
    Wans {
        #[command(subcommand)]
        command: LookupCommand,
    },
    /// Print the unifictl version
    Version,
}

#[derive(Debug, Subcommand)]
pub enum DevicesCommand {
    /// List devices
    List,
    /// Show one device
    Get {
        /// Device ID, as shown by `devices list`
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
        id: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum StatusFilter {
    Online,
    Offline,
}

#[derive(Debug, Subcommand)]
pub enum PortsCommand {
    /// List ports across all switches and gateways
    List(PortsListArgs),
}

#[derive(Debug, Args)]
pub struct PortsListArgs {
    /// Filter by device ID (switch or gateway)
    #[arg(long)]
    pub device: Option<String>,
    /// Filter by VLAN mode
    #[arg(long, value_enum)]
    pub mode: Option<ModeFilter>,
    /// Filter by port state
    #[arg(long, value_enum, conflicts_with = "all_ports")]
    pub state: Option<StateFilter>,
    /// Filter to ports carrying this VLAN ID
    #[arg(long, default_value_t = 0)]
    pub vlan: i64,
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
