mod network;
mod runtime;
mod service;

use crate::service::Service;

use emulationcore::xmlgraphparser::XMLGraphParser;

use std::env::args;
use std::fs::read_to_string;

use anyhow::Result;
use tracing::{debug, subscriber, Level};
use tracing_subscriber::FmtSubscriber;

// TODO: update emulationcore (new orchestrator for resolve hostnames and start/stop experiment)
// TODO: accept these constants as arguments
const TOPOLOGY_PATH: &str = "./topology.xml";
const _WASM_PATH: &str = "./wasm/";
const _RUNTIME: &str = "wasmtime";
const _IFNAME: &str = "eth0";

fn main() -> Result<()> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::DEBUG)
        .finish();
    subscriber::set_global_default(subscriber)?;

    let topology_path = args().nth(1).unwrap_or(TOPOLOGY_PATH.to_string());

    let topology = read_to_string(topology_path)?;
    let parser = XMLGraphParser::try_new(&topology, "container".to_string())?;

    let services = {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let (_config, graph) = rt.block_on(parser.fill_graph());

        graph.services_by_name
            .iter()
            .flat_map(|(name, vec)| {
                vec.iter().map(|e| {
                    let s = e.blocking_lock();
                    let image = s.image.clone().unwrap();
                    let command = s.command.clone();
                    Service::new(name.to_owned(), image, command)
                })
            })
            .collect::<Vec<Service>>()
    };
    debug!("Parsed services: {:?}", services);

    // TODO: create networking components
    // TODO: setup communicationmanager
    // TODO: setup emulationcore
    // TODO: execute paused/hanged runtimes
    // TODO: poll runtimes/emulationcore and cleanup

    Ok(())
}
