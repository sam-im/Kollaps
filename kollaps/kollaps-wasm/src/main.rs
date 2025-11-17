mod config;
mod kollaps;
mod network;
mod service;

use crate::config::Config;
use crate::kollaps::Kollaps;

use anyhow::Result;
use tracing::{Level, error, info, subscriber};
use tracing_subscriber::FmtSubscriber;

fn main() -> Result<()> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::DEBUG)
        .finish();
    subscriber::set_global_default(subscriber)?;

    // TODO: Parse arguments
    let config = Config::default();

    let mut kollaps = Kollaps::new(config);

    info!("Running kollaps.");
    match kollaps.run() {
        Ok(_) => info!("Exiting."),
        Err(e) => error!("Kollaps is exiting with an error:\n{}.", e),
    }

    Ok(())
}
