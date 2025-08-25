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

use orchestrator::{Orchestrator, docker::DockerOrchestrator, kubernetes::KubernetesOrchestrator};
use tracing::{Level, info};
use tracing_subscriber::FmtSubscriber;

fn main() {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO) // set to `LevelFilter::OFF` to disable logging completely
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();

    if env::args().len() == 4 {
        let id = env::args().nth(1).unwrap();
        let pid = env::args().nth(2).unwrap().parse::<u32>().unwrap();
        let orchestrator = env::args().nth(3).unwrap();

        rt.block_on(container_deployment(id, pid, orchestrator));
    } else {
        let topology_file = env::args().nth(1).unwrap();
        let cm_file = env::args().nth(2).unwrap();
        let networkdevice = env::args().nth(3).unwrap();

        rt.block_on(baremetal_deployment(topology_file, cm_file, networkdevice));
    }
}

async fn container_deployment(id: String, pid: u32, orchestrator: String) {
    info!("EC {}: starting", id);
    let orchestrator = match orchestrator.as_str() {
        "docker" => Orchestrator::Docker(DockerOrchestrator),
        "kubernetes" => Orchestrator::Kubernetes(KubernetesOrchestrator),
        _ => unimplemented!("unkown argument: {}", orchestrator),
    };
    let mut ec = EmulationCore::new(id.clone(), pid, Some(orchestrator));
    ec.init().await;
    ec.emulation_loop().await;
    info!("EC {}: stopped", id);
}

async fn baremetal_deployment(topology_file: String, cm_file: String, ifname: String) {
    info!("EC: starting");

    let mut ec = EmulationCore::new("".to_string(), 0, None);
    ec.set_topology_file(topology_file);
    ec.set_cm_file(cm_file);
    ec.set_network_device(ifname);

    ec.init_baremetal().await;
    ec.emulation_loop().await;
    info!("EC: stopped");
}
