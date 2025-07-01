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
mod docker;
mod elements;
mod emulation;
mod emulationcore;
mod eventscheduler;
mod graph;
mod state;
mod xmlgraphparser;

use crate::emulationcore::EmulationCore;

use std::env;
use tokio::runtime;

use tracing::{Level, info};
use tracing_subscriber::FmtSubscriber;

fn main() {
    // Logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO) // set to `LevelFilter::OFF` to disable logging completely
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    // TODO move parsing arguments to here
    // TODO move runtime initialization to here
    if env::args().len() == 4 {
        container_deployment();
    } else {
        baremetal_deployment();
    }
}

fn container_deployment() {
    let id = env::args().nth(1).unwrap();

    let pid = env::args().nth(2).unwrap().parse::<u32>().unwrap();

    let orchestrator = env::args().nth(3).unwrap();

    let basic_rt = runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();

    info!("EC {} starting", id);
    let mut ec = EmulationCore::new(id.clone(), pid, orchestrator);
    ec.init();
    basic_rt.block_on(async move { ec.emulation_loop().await });
    info!("EC {}: stopping", id);
}

fn baremetal_deployment() {
    info!("EC: baremetal deployment started");
    let topology_file = env::args().nth(1).unwrap();

    let cm_file = env::args().nth(2).unwrap();

    let networkdevice = env::args().nth(3).unwrap();

    let mut ec = EmulationCore::new("".to_string(), 0, "baremetal".to_string());

    ec.set_topology_file(topology_file);

    ec.set_cm_file(cm_file);

    ec.set_network_device(networkdevice);

    ec.init_baremetal();

    let basic_rt = runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    basic_rt.block_on(async move { ec.emulation_loop().await });
    info!("EC: stopped emulation");
}
