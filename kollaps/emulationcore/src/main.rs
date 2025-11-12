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

use std::env;

use orchestrator::{
    Orchestrator, docker::DockerOrchestrator, kubernetes::KubernetesOrchestrator,
    wasm::WasmOrchestrator,
};
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

    // TODO: proper argument parsing
    // TODO: optional argument to set the kollaps tmp dir (then update resolve_hostnames of wasm)
    if !(env::args().nth(4) == Some("baremetal".to_string())) {
        let id = env::args().nth(1).unwrap();
        let pid = env::args().nth(2).unwrap().parse::<u32>().unwrap();
        let orchestrator = env::args().nth(3).unwrap();
        // optional interface name, defaults to "eth0"
        let ifname = env::args().nth(4);

        rt.block_on(container_deployment(id, pid, orchestrator, ifname));
    } else {
        let topology_file = env::args().nth(1).unwrap();
        let cm_file = env::args().nth(2).unwrap();
        let ifname = env::args().nth(3);

        rt.block_on(baremetal_deployment(topology_file, cm_file, ifname));
    }
}

async fn container_deployment(id: String, pid: u32, orchestrator: String, ifname: Option<String>) {
    info!("EC {}: starting", id);
    let orchestrator = match orchestrator.as_str() {
        "docker" => Orchestrator::Docker(DockerOrchestrator),
        "kubernetes" => Orchestrator::Kubernetes(KubernetesOrchestrator),
        "wasm" => Orchestrator::Wasm(WasmOrchestrator),
        _ => unimplemented!("unkown orchestrator: {}", orchestrator),
    };
    let mut ec = EmulationCore::new(id.clone(), pid, Some(orchestrator), ifname);
    ec.init().await;
    ec.emulation_loop().await;
    info!("EC {}: stopped", id);
}

async fn baremetal_deployment(topology_file: String, cm_file: String, ifname: Option<String>) {
    info!("EC: starting");

    let mut ec = EmulationCore::new("".to_string(), 0, None, ifname);
    ec.set_topology_file(topology_file);
    ec.set_cm_file(cm_file);

    ec.init_baremetal().await;
    ec.emulation_loop().await;
    info!("EC: stopped");
}
