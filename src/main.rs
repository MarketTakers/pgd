mod cli;
mod config;
mod state;

mod consts;

mod controller;

use clap::Parser;
use clap_verbosity_flag::Verbosity;
use cli::Cli;
use miette::Result;
use tracing::debug;

use crate::{
    cli::ControlCommands,
    controller::{Context, Controller},
};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    init_tracing(cli.verbosity);

    debug!("pgd.start");

    macro_rules! do_cmd {
        ($name:expr, $method:ident $(, $arg:expr)*) => {{
            let ctx = Context::new($name).await?;
            Controller::new(ctx).$method($($arg),*).await?;
        }};
    }

    match cli.command {
        cli::Commands::Init => {
            do_cmd!(None, init_project);
        }
        cli::Commands::Instance { name, cmd } => match cmd {
            ControlCommands::Start => do_cmd!(name, start),
            ControlCommands::Stop => do_cmd!(name, stop),
            ControlCommands::Restart => do_cmd!(name, restart),
            ControlCommands::Destroy { force } => do_cmd!(name, destroy, force),
            ControlCommands::Logs { follow } => do_cmd!(name, logs, follow),
            ControlCommands::Status => do_cmd!(name, status),
            // can't override an instance for this command, because password is in config
            ControlCommands::Conn { format } => do_cmd!(None, show_connection, format),
            ControlCommands::Wipe { force } => do_cmd!(name, wipe, force),
        },
    }

    Ok(())
}

fn init_tracing(verbosity: Verbosity) {
    tracing_subscriber::fmt()
        .with_max_level(verbosity)
        .without_time()
        .with_target(false)
        .init();
}
