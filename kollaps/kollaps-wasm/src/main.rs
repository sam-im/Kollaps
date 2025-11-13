mod config;
mod kollaps;
mod network;
mod runtime;
mod service;

use crate::config::Config;
use crate::kollaps::Kollaps;

use anyhow::Result;
use tracing::{Level, error, info, subscriber};
use tracing_subscriber::FmtSubscriber;

// TODO: consider setting `setpgid` and sending sigkill to all childs as cleanup (check pid1.c)
// TODO: wait on emulationcores and clean up (send kill to all services and communicationmanager)
// TODO: handle at least sigint

fn main() -> Result<()> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::DEBUG)
        .finish();
    subscriber::set_global_default(subscriber)?;

    // TODO: Parse arguments
    let config = Config::default();

    let mut kollaps = Kollaps::new(config);

    match kollaps.run() {
        Ok(_) => info!("Exiting."),
        Err(e) => error!("Kollaps is exiting with an error: {}.", e),
    }

    Ok(())
}
