mod cli;
mod config;
mod state;

mod consts;

mod controller;

use clap::Parser;
use cli::Cli;
use miette::Result;
use tracing::info;

use crate::{
    cli::ControlCommands,
    controller::{Context, Controller},
};

#[tokio::main]
async fn main() -> Result<()> {
    println!("{}", include_str!("./banner.txt"));

    let cli = Cli::parse();
    init_tracing(cli.verbose);

    info!("pgd.start");

    match cli.command {
        cli::Commands::Init => {
            let ctx = Context::new(None).await?;
            Controller::new(ctx).init_project().await?;
        }
        cli::Commands::Instance { name, cmd } => match cmd {
            ControlCommands::Start => {}
            ControlCommands::Stop => {}
            ControlCommands::Restart => {}
            ControlCommands::Destroy => {}
            ControlCommands::Logs { follow } => todo!(),
            ControlCommands::Status => {}
            ControlCommands::Connection { format: _ } => {}
        },
    }

    Ok(())
}

fn init_tracing(_verbose: bool) {
    tracing_subscriber::fmt::init();
}
