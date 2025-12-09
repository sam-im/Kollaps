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

    let config = Args::parse();

    info!("Starting WASI runtime.");
    match Runtime::from(config).run() {
        Ok(_) => info!("Exiting."),
        Err(e) => error!("Failed to run the runtime, reason: {}", e),
    }
    Ok(())
}

#[derive(Parser)]
struct Args {
    /// Specifies a path to a WASM module.
    wasm_path: PathBuf,
    /// Optionally map a directory in the host as the current directory in the runtime.
    #[arg(long)]
    opt_dir: Option<PathBuf>,
    /// List of arguments that will be passed to the WASM module.
    wasm_args: Vec<String>,
}
