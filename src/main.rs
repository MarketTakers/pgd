mod cli;
mod config;
mod state;

mod consts {
    pub const USERNAME: &str = "postgres";
    pub const DATABASE: &str = "postgres";
}

mod controller;

use clap::Parser;
use cli::Cli;
use miette::Result;
use tracing::info;

use crate::controller::Controller;

#[tokio::main]
async fn main() -> Result<()> {
    println!("{}", include_str!("./banner.txt"));

    let cli = Cli::parse();
    init_tracing(cli.verbose);

    info!("pgx.start");
    let controller = Controller::new().await?;

    match cli.command {
        cli::Commands::Init => controller.init_project().await?,
        cli::Commands::Instance { name, cmd } => todo!(),
        cli::Commands::Sync => todo!(),
    }

    Ok(())
}

fn init_tracing(verbose: bool) {
    

    tracing_subscriber::fmt::init();
}
