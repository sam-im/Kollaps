// Licensed to the Apache Software Foundation (ASF) under one or more
// contributor license agreements.  See the NOTICE file distributed with
// this work for additional information regarding copyright ownership.
// The ASF licenses this file to You under the Apache License, Version 2.0
// (the "License"); you may not use this file except in compliance with
// the License.  You may obtain a copy of the License at

//    http://www.apache.org/licenses/LICENSE-2.0

// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
mod aux;
mod communication;
mod elements;
mod emulation;
mod emulationcore;
mod eventscheduler;
mod graph;
mod orchestrator;
mod state;
mod xmlgraphparser;

use crate::emulationcore::EmulationCore;
use crate::orchestrator::{
    Orchestrator, docker::DockerOrchestrator, kubernetes::KubernetesOrchestrator,
    wasm::WasmOrchestrator,
};

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use tracing::{Level, info};
use tracing_subscriber::FmtSubscriber;

fn main() {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO) // set to `LevelFilter::OFF` to disable logging completely
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let args = Args::parse();
    match args.deployment {
        Deployment::Baremetal { cm_path } => {
            rt.block_on(baremetal_deployment(args.topology, cm_path, args.ifname))
        }
        _ => rt.block_on(container_deployment(args)),
    }
}

async fn container_deployment(args: Args) {
    let (id, pid, orchestrator) = match args.deployment {
        Deployment::Docker { id, pid } => (id, pid, Orchestrator::Docker(DockerOrchestrator)),
        Deployment::Kubernetes { id, pid } => {
            (id, pid, Orchestrator::Kubernetes(KubernetesOrchestrator))
        }
        Deployment::Wasm { id, pid } => (id, pid, Orchestrator::Wasm(WasmOrchestrator)),
        _ => panic!(),
    };
    info!("EC {}: starting", id);
    let mut ec = EmulationCore::new(
        args.topology,
        id.clone(),
        pid,
        Some(orchestrator),
        args.ifname,
    );
    ec.init().await;
    ec.emulation_loop().await;
    info!("EC {}: stopped", id);
}

async fn baremetal_deployment(topology: PathBuf, cm: String, ifname: Option<String>) {
    info!("EC: starting");
    let mut ec = EmulationCore::new(topology, "".to_string(), 0, None, ifname);
    ec.set_cm_file(cm);
    ec.init_baremetal().await;
    ec.emulation_loop().await;
    info!("EC: stopped");
}

// Argument parsing.
#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Args {
    /// Specifies a path to a topology file.
    topology: PathBuf,

    /// Sets the interface name to use. Defaults to "eth0".
    #[arg(short, long)]
    ifname: Option<String>,

    /// Specifies the deployment type.
    #[command(subcommand)]
    deployment: Deployment,
}

#[derive(Subcommand)]
enum Deployment {
    Baremetal {
        /// Path to a communicationmanager binary.
        cm_path: String,
    },
    Docker {
        /// Service ID.
        id: String,
        /// Container PID.
        pid: u32,
    },
    Kubernetes {
        /// Service ID.
        id: String,
        /// Container PID.
        pid: u32,
    },
    Wasm {
        /// Service ID.
        id: String,
        /// Service PID.
        pid: u32,
    },
}
