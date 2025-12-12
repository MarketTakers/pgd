use clap::{Parser, Subcommand, builder::styling};

const STYLES: styling::Styles = styling::Styles::styled()
    .header(styling::AnsiColor::Green.on_default().bold())
    .usage(styling::AnsiColor::Green.on_default().bold())
    .literal(styling::AnsiColor::Blue.on_default().bold())
    .placeholder(styling::AnsiColor::Cyan.on_default());

#[derive(Parser)]
#[command(name = "pgd")]
#[command(about = "Project-scoped PostgreSQL instance manager", long_about = include_str!("./banner.txt"))]
#[command(version)]
#[command(styles = STYLES)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    #[command(flatten)]
    pub verbosity: clap_verbosity_flag::Verbosity,
}

#[derive(Clone, clap::ValueEnum)]
pub enum ConnectionFormat {
    /// DSN Url
    Dsn,
    // Human readable format
    Human,
}

#[derive(Subcommand)]
pub enum ControlCommands {
    /// Start postgres instance
    Start,
    /// Stop postgres instance
    Stop,
    /// Restart postgres instance
    Restart,
    /// (WARNING!) Destroy postgres instance
    Destroy { force: bool },
    /// (WARNING!) Destruct database
    Wipe { force: bool },

    /// Status of instance
    Status,
    /// View logs produced by postgres
    Logs {
        #[arg(short, long, default_value = "false")]
        follow: bool,
    },
    /// (Sensitive) get connection details
    Conn {
        #[arg(short, long, default_value = "dsn")]
        format: ConnectionFormat,
    },
}

#[derive(Subcommand)]
pub enum Commands {
    /// Create a new project, or initialize instance for existing one
    Init,

    /// Start the PostgreSQL container for the current project
    Instance {
        // Name of the instance you want to control. Defaults to current project
        name: Option<String>,
        #[command(subcommand)]
        cmd: ControlCommands,
    },
}
