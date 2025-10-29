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

// TODO these elements belong to the graph module
use std::sync::Arc;
use tokio::sync::Mutex;

use tracing::info;

//Represents a bridge and a container
pub struct Service {
    pub ip: u32,
    //amount of bytes sent to this container from the perspective of this EC
    pub last_bytes: u32,
    pub hostname: String,
    pub shared: bool,
    pub reuse: bool,
    pub replicas: u32,
    pub supervisor: bool,
    pub supervisor_port: u32,
    pub links: Vec<u16>,
    pub replica_id: usize,
    pub activepaths: Vec<String>,
    pub script: String,
    pub image: Option<String>,
    pub command: Option<String>,
}

impl Service {
    pub fn new(hostname: String, shared: bool, reuse: bool, replicas: u32, image: Option<String>, command: Option<String>) -> Service {
        Service {
            ip: 0,
            last_bytes: 0,
            hostname: hostname,
            shared: shared,
            reuse: reuse,
            replicas: replicas,
            supervisor: false,
            supervisor_port: 0,
            links: vec![],
            replica_id: 0,
            activepaths: vec![],
            script: "".to_string(),
            image,
            command,
        }
    }

    pub fn attach_link(&mut self, id: u16) {
        self.links.push(id);
    }

    pub fn remove_link(&mut self, id: u16) {
        let index = self.links.iter().position(|&r| r == id);
        if !(index.is_none()) {
            self.links.remove(index.unwrap());
        }
    }

    pub fn set_activepaths(&mut self, activepaths: Vec<String>) {
        self.activepaths = activepaths;
    }
}

//Structure used to represent a link, paths have bandwidth, Links are descriptive
pub struct Link {
    pub id: u16,
    pub bandwidth: f32,
    pub latency: f32,
    pub jitter: f32,
    pub drop: f32,
    pub source: Arc<Mutex<Service>>,
    pub destination: Arc<Mutex<Service>>,
    pub flows: Vec<Vec<f32>>,
}

impl Link {
    pub fn new(
        id: u16,
        latency: f32,
        jitter: f32,
        drop: f32,
        bandwidth: f32,
        source: Arc<Mutex<Service>>,
        destination: Arc<Mutex<Service>>,
    ) -> Self {
        Self {
            id: id,
            bandwidth: bandwidth,
            latency: latency,
            jitter: jitter,
            drop: drop,
            source: source,
            destination: destination,
            flows: vec![],
        }
    }

    pub fn clear_flows(&mut self) {
        self.flows.clear();
    }

    pub async fn print(&mut self, name: String) {
        info!(
            "Name {}: Link with id {} from {} to {} and parameters: bw {} | latency {:.} | jitter {:.1} | drop {:.1}",
            name.clone(),
            self.id,
            self.source.lock().await.hostname,
            self.destination.lock().await.hostname,
            self.bandwidth,
            self.latency,
            self.jitter,
            self.drop
        );
    }
}
//Path between two network elements, hold multiple links with different network values, the ones in the structure result from calculations
#[derive(Debug)]
pub struct Path {
    pub links: Vec<u16>,
    pub id: u32,
    pub latency: f32,
    pub rtt: f32,
    pub drop: f32,
    pub max_bandwidth: f32,
    pub jitter: f32,
    pub used_bandwidth: f32,
    pub current_bandwidth: f32,
    pub start: String,
    pub finish: String,
    pub last_cycle_change: u16,
}

impl Path {
    pub fn new(id: u32, links: Vec<u16>) -> Self {
        Self {
            links: links,
            id: id,
            latency: 0.0000,
            rtt: 0.0,
            drop: 0.0,
            max_bandwidth: 0.0,
            jitter: 0.0,
            used_bandwidth: 0.0,
            current_bandwidth: 0.0,
            start: "".to_string(),
            finish: "".to_string(),
            last_cycle_change: 0,
        }
    }

    pub fn print(&self, name: &String) {
        info!("EC {}: path {:?}", name, self);
    }

    pub fn set_used_bandwidth(&mut self, used_bandwidth: f32) {
        self.used_bandwidth = used_bandwidth;
    }

    pub fn set_current_bandwidth(&mut self, current_bandwidth: f32) {
        self.current_bandwidth = current_bandwidth;
    }
}

// Metadata circulating in the emulation
pub struct Flowu16 {
    // bytes that circulated
    pub bandwidth: f32,
    // links it went through
    pub link_indices: Vec<u16>,
    pub age: u32,
}

impl Flowu16 {
    pub fn new(bandwidth: f32, link_indices: Vec<u16>) -> Flowu16 {
        Flowu16 {
            bandwidth: bandwidth,
            link_indices: link_indices,
            age: 0,
        }
    }
}
