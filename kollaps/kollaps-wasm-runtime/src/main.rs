//! Wrapper around wasmtime's runtime with wasi-preview-2 and networking enabled by default.
//! [Reference](https://github.com/bytecodealliance/wasmtime/blob/main/examples/wasip2/main.rs)

mod runtime;

use crate::runtime::Runtime;

use std::path::PathBuf;

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

    info!("Starting WASI runtime.");
    match Runtime::from(args).run() {
        Ok(_) => info!("Exiting."),
        Err(e) => error!("Failed to run the runtime, reason: {}", e),
    }
    Ok(())
}

#[derive(Parser)]
struct Args {
    /// Specifies a path to a WASM module.
    path: PathBuf,
    /// Maps a directory on the host as the current directory in the runtime.
    #[arg(long)]
    dir: Option<PathBuf>,
}
