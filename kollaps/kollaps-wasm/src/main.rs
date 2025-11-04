mod config;
mod network;
mod runtime;
mod service;

use crate::config::Config;
use crate::service::Service;

use emulationcore::xmlgraphparser::XMLGraphParser;
use network::Bridge;
use service::{ActiveService, ReadyService};

use std::fs::{self, read_to_string};
use std::net::Ipv4Addr;
use std::process::{Child, Command};
use std::str::FromStr;

use anyhow::{Context, Result};
use tracing::{Level, debug, info, subscriber};
use tracing_subscriber::FmtSubscriber;

fn main() -> Result<()> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::DEBUG)
        .finish();
    subscriber::set_global_default(subscriber)?;

    // TODO: Parse arguments
    let config = Config::default();

    info!("Parsing topology file.");
    let topology =
        read_to_string(&config.topoinfo_path).context("Failed to read the topology file.")?;
    let parser = XMLGraphParser::try_new(&topology, "container".to_string())
        .context("Failed to parse the topology file.")?;

    info!("Extracting services.");
    let services = {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let (_config, graph) = rt.block_on(parser.fill_graph());

        graph
            .services_by_name
            .into_iter()
            .flat_map(|(name, vec)| {
                let name = name.to_ascii_lowercase();
                vec.iter()
                    .map(|e| {
                        let s = e.blocking_lock();
                        let image = s.image.clone().unwrap();
                        let command = s.command.clone();
                        Service::new(name.clone(), image, command)
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<Service>>()
    };
    debug!("Services = {:?}", services);

    info!("Creating virtual bridge and namespaces.");
    let mut bridge = Bridge::new("k_wasm", Ipv4Addr::from_str(&config.addr)?, 24);
    let services = services
        .into_iter()
        .map(|s| -> Result<ReadyService> {
            let ns = bridge.create_namespace()?;
            Ok(ReadyService::new(s, ns))
        })
        .map(|s| s.expect("Failed to create network namespaces for services."))
        .collect::<Vec<ReadyService>>();

    info!("Setting up temporary directory.");
    let mut topoinfo = String::new();
    let mut topoinfodashboard = String::new();
    services.iter().for_each(|s| {
        if s.service.name == "dashboard" {
            topoinfodashboard.push_str(&s.id);
            topoinfodashboard.push_str("\n");
        } else {
            topoinfo.push_str(&s.id);
            topoinfo.push_str("\n");
        }
    });

    let _ = fs::create_dir(&config.tmp_dir);
    let _ = fs::create_dir(format!("{}{}", &config.tmp_dir, "pipes"));
    fs::File::create(format!("{}{}", &config.tmp_dir, &config.remote_ips_path))?;
    fs::write(
        format!("{}{}", &config.tmp_dir, &config.topoinfo_path),
        topoinfo,
    )?;
    fs::write(
        format!("{}{}", &config.tmp_dir, &config.topoinfodashboard_path),
        topoinfodashboard,
    )?;

    info!("Starting Communication Manager.");
    let _commanager = Command::new("communicationmanager")
        .arg(services.len().to_string())
        .spawn()?;

    info!("Starting service runtimes.");
    let service_names = services.iter().map(|s| s.service.name.clone()).collect::<Vec<String>>();
    let services: Vec<ActiveService> = services
        .into_iter()
        .filter(|s| s.service.name != "dashboard") // TODO: check how bootstrapper bootstraps dashboard
        .map(|s| {
            // Replace each service name by its address.
            let mut service_args: Vec<String> = Vec::new();
            if let Some(args_str) = &s.service.command {
                args_str
                    .split_ascii_whitespace()
                    .for_each(|a| {
                        if let Some(addr) = service_names.iter().find(|n| *n == a) {
                            service_args.push(addr.to_string());
                        } else {
                            service_args.push(a.to_string());
                        }
                    });
            }
            // TODO: fork and raise sigstop, after sigcont run the runtime
            let handle = Command::new(&config.runtime_name)
                .arg(format!("{}{}.wasm", &config.wasm_dir, &s.service.image))
                .args(&service_args)
                .spawn()
                .expect(&format!("Failed to start {} runtime", &config.runtime_name));
            ActiveService::new(handle, s)
        })
        .collect();

    info!("Starting Emulation Core instances...");
    let _ecores: Vec<Child> = services
        .iter()
        .filter(|s| s.name() != "dashboard")
        .map(|s| {
            Command::new("emulationcore")
                .arg(s.id())
                .arg(s.pid().to_string())
                .arg("wasm")
                .arg(s.veth())
                .spawn()
                .expect("Failed to start emulationcore instance.")
        })
        .collect();

    // TODO: wait for runtimes to exit and clean up

    info!("Exiting.");
    Ok(())
}
