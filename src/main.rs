mod cli;
mod config;
mod state;

mod controller;

use clap::Parser;
use cli::Cli;
use miette::Result;
use tracing::info;

use crate::controller::Controller;

#[tokio::main]
async fn main() -> Result<()> {
    println!("{}", include_str!("./banner.txt"));
    let controller = Controller::new().await?;

    let cli = Cli::parse();
    init_tracing(cli.verbose);

    info!("pgx.start");

    match cli.command {
        cli::Commands::Init => controller.init_project().await?,
        cli::Commands::Instance { name, cmd } => todo!(),
        cli::Commands::Sync => todo!(),
    }

    Ok(())
}

fn init_tracing(verbose: bool) {
    use tracing_subscriber::{fmt, prelude::*};

    tracing_subscriber::registry()
        .with(fmt::layer().with_target(false).with_level(true))
        .init();
}
