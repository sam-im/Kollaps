mod comms;
mod config;
mod ecore;
mod kollaps;
mod network;
mod service;

use crate::config::Config;
use crate::kollaps::Kollaps;

use std::{net::Ipv4Addr, path::PathBuf};

use anyhow::Result;
use clap::Parser;
use tracing::{Level, error, info, subscriber};
use tracing_subscriber::FmtSubscriber;

fn main() -> Result<()> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::DEBUG)
        .finish();
    subscriber::set_global_default(subscriber)?;

    let args = Args::parse();
    let config = Config::from(args);

    let mut kollaps = Kollaps::new(config);

    info!("Running kollaps.");
    match kollaps.run() {
        Ok(_) => info!("Exiting."),
        Err(e) => error!("Kollaps is exiting with an error:\n{}.", e),
    }

    Ok(())
}

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Args {
    /// Specifies a path to a topology file.
    topology: PathBuf,
    /// Sets a custom address in CIDR notation.
    #[arg(long)]
    addr: Option<Ipv4Addr>,
    /// Sets a custom subnet mask in CIDR notation.
    #[arg(long)]
    subnet: Option<u8>,
}
