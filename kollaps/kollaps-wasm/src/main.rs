mod network;
mod runtime;
mod service;

use std::env::args;
use std::fs::read_to_string;

use emulationcore::xmlgraphparser::XMLGraphParser;

use anyhow::Result;
use tracing::{Level, subscriber};
use tracing_subscriber::FmtSubscriber;

// TODO: update emulationcore (new orchestrator for resolve hostnames and start/stop experiment)

// TODO accept these constants as arguments
const _WASM_MODULES_PATH: &str = "./wasm_modules/";
const _RUNTIME: &str = "wasmtime";

fn main() -> Result<()> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::DEBUG)
        .finish();
    subscriber::set_global_default(subscriber)?;

    let topology_path = args().nth(1).expect("missing argument: topology path");

    let topology = read_to_string(topology_path)?;
    let parser = XMLGraphParser::try_new(&topology, "container".to_string())?;
    let _ = {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let (_config, graph) = rt.block_on(parser.fill_graph());
        // TODO: retrieve required data from graph
        let _service_count = graph.services_by_name.len(); // communicationmanager argument
    };

    // TODO: create networking components
    // TODO: setup communicationmanager
    // TODO: setup emulationcore
    // TODO: execute paused runtimes
    // TODO: poll runtimes/emulationcore and cleanup

    Ok(())
}
