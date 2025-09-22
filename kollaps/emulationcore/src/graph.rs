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

use crate::aux::Dijkstraentry;
use crate::elements::{Flowu16, Link, Path, Service};
use rand::Rng;
use std::collections::HashMap;
use std::f32::INFINITY;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, error, info};

//Graph of the current network state
#[derive(Clone)]
pub struct Graph {
    //Containers
    pub services: HashMap<u32, Arc<Mutex<Service>>>,
    //bridges
    pub bridges: HashMap<u32, Arc<Mutex<Service>>>,
    //Links between elements
    pub links: HashMap<u16, Arc<Mutex<Link>>>,
    //Paths between elements (multiple links)
    pub paths: HashMap<u32, Arc<Mutex<Path>>>,
    //Aux map that olds an IP to an id path
    pub ip_to_path_id: HashMap<u32, u32>,
    //Aux map that olds the path id to an IP
    pub path_id_to_ip: HashMap<u32, u32>,
    //Holds metadata received from other containers, with metadata specific to this graph
    pub flow_accumulator_u16: HashMap<String, Arc<Mutex<Flowu16>>>,
    //Aux vec with IPs of containers
    pub services_by_name: HashMap<String, Vec<Arc<Mutex<Service>>>>,

    pub bridges_by_name: HashMap<String, Vec<Arc<Mutex<Service>>>>,

    //hold flows sent by others
    pub flow_accumulator_keys: HashMap<String, String>,

    //hold all ips in deployment
    pub ips: Vec<u32>,

    pub link_counter: u16,

    pub path_counter: u32,

    //Who we are
    pub graph_root: Option<Arc<Mutex<Service>>>,

    pub removed_bridges: HashMap<String, Vec<Arc<Mutex<Service>>>>,

    pub removed_links: Vec<Arc<Mutex<Link>>>,

    //number of total links
    pub link_count: u32,

    pub name: String,
}

impl Graph {
    pub fn new() -> Graph {
        Graph {
            services: HashMap::new(),
            links: HashMap::new(),
            paths: HashMap::new(),
            ip_to_path_id: HashMap::new(),
            path_id_to_ip: HashMap::new(),
            flow_accumulator_u16: HashMap::new(),
            flow_accumulator_keys: HashMap::new(),
            ips: vec![],
            services_by_name: HashMap::new(),
            bridges_by_name: HashMap::new(),
            link_counter: 0,
            graph_root: None,
            path_counter: 0,
            bridges: HashMap::new(),
            removed_bridges: HashMap::new(),
            removed_links: vec![],
            link_count: 0,
            name: "".to_string(),
        }
    }

    pub async fn insert_service(
        &mut self,
        hostname: String,
        shared: bool,
        reuse: bool,
        replicas: u32,
        ip: Option<u32>,
        paths: Vec<String>,
        script: Option<&str>,
    ) {
        let mut service = Service::new(hostname.clone(), shared, reuse, replicas);

        if !script.is_none() {
            service.script = script.unwrap().to_string();
        }

        service.set_activepaths(paths);

        let service_locked = Arc::new(Mutex::new(service));

        match self.services_by_name.get_mut(&hostname) {
            Some(services) => services.push(Arc::clone(&service_locked)),
            None => {
                let mut new_services = vec![];
                new_services.push(Arc::clone(&service_locked));
                self.services_by_name.insert(hostname, new_services);
            }
        };

        if ip.is_none() {
            return;
        } else {
            service_locked.lock().await.ip = ip.unwrap();
            self.services.insert(ip.unwrap(), service_locked);
        }
    }

    pub fn insert_bridge(&mut self, hostname: String, ip: Option<u32>) {
        let mut bridge = Service::new(hostname.clone(), false, false, 0);

        if ip.is_none() {
            //generate random IP to put be able to put bridges in the map
            let mut ip_gen: u32 = 0;

            let mut rng = rand::rng();
            while self.bridges.contains_key(&ip_gen) {
                ip_gen = rng.random();
            }

            bridge.ip = ip_gen;

            let bridge_locked = Arc::new(Mutex::new(bridge));

            match self.bridges_by_name.get_mut(&hostname) {
                Some(bridges) => bridges.push(Arc::clone(&bridge_locked)),
                None => {
                    let mut new_bridges = vec![];
                    new_bridges.push(Arc::clone(&bridge_locked));
                    self.bridges_by_name.insert(hostname, new_bridges);
                }
            };

            self.bridges.insert(ip_gen, bridge_locked.clone());
        } else {
            bridge.ip = ip.unwrap();

            let bridge_locked = Arc::new(Mutex::new(bridge));

            match self.bridges_by_name.get_mut(&hostname) {
                Some(bridges) => bridges.push(Arc::clone(&bridge_locked)),
                None => {
                    let mut new_bridges = vec![];
                    new_bridges.push(Arc::clone(&bridge_locked));
                    self.bridges_by_name.insert(hostname, new_bridges);
                }
            };
            self.bridges.insert(ip.unwrap(), bridge_locked.clone());
        }
    }

    pub async fn set_dashboard(&mut self, name: String, supervisor_port: u32) {
        match self.services_by_name.get_mut(&name) {
            Some(services) => {
                services[services.len() - 1].lock().await.supervisor = true;
                services[services.len() - 1].lock().await.supervisor_port = supervisor_port;
            }
            None => {
                error!(
                    "EC {} - expected dashboard name in services_by_name",
                    self.name
                );
            }
        }
    }

    pub async fn insert_link(
        &mut self,
        latency: f32,
        jitter: f32,
        drop: f32,
        bandwidth: f32,
        source: String,
        destination: String,
    ) {
        let source_nodes = self.get_nodes(source.clone());

        let destination_nodes = self.get_nodes(destination.clone());

        for source_node in source_nodes {
            for destination_node in destination_nodes.clone() {
                let id = self.link_counter.clone();
                let link = Link::new(
                    id,
                    latency,
                    jitter,
                    drop,
                    bandwidth,
                    source_node.clone(),
                    destination_node.clone(),
                );
                let link = Arc::new(Mutex::new(link));
                source_node.lock().await.attach_link(id);
                self.links.insert(id, link);
                self.link_counter += 1;
            }
        }
    }

    pub fn insert_path(&mut self, id: u32, links: Vec<u16>) {
        let path = Path::new(id, links);
        let path = Arc::new(Mutex::new(path));
        self.paths.insert(id, path);
    }

    //from name retrieves the services/bridges
    pub fn get_nodes(&mut self, name: String) -> Vec<Arc<Mutex<Service>>> {
        let services = self.get_service_nodes(name.clone());

        if services.is_none() {
            return self.get_bridge_nodes(name.clone()).unwrap();
        } else {
            return services.unwrap();
        }
    }

    pub fn get_service_nodes(&mut self, name: String) -> Option<Vec<Arc<Mutex<Service>>>> {
        match self.services_by_name.get_mut(&name) {
            Some(services) => return Some(services.clone()),
            None => return None,
        };
    }

    pub fn get_bridge_nodes(&mut self, name: String) -> Option<Vec<Arc<Mutex<Service>>>> {
        match self.bridges_by_name.get_mut(&name) {
            Some(bridges) => return Some(bridges.clone()),
            None => return None,
        };
    }

    pub fn get_path(&self, id: u32) -> Option<Arc<Mutex<Path>>> {
        match self.paths.get(&id) {
            Some(path) => {
                return Some(Arc::clone(path));
            }
            None => return None,
        }
    }

    pub fn insert_ip_to_path(&mut self, ip: u32, id: u32) {
        self.ip_to_path_id.insert(ip, id);
        self.path_id_to_ip.insert(id, ip);
    }

    pub fn get_ip_from_path_id(&mut self, id: u32) -> u32 {
        return *self.path_id_to_ip.get(&id).unwrap();
    }

    pub fn get_path_id_from_ip(&mut self, ip: u32) -> Option<u32> {
        match self.ip_to_path_id.get(&ip) {
            Some(id) => return Some(*id),
            None => return None,
        }
    }

    pub async fn print_graph(&mut self, name: String) {
        //     for (name, services) in &self.services_by_name {
        //         for service in services{
        //             let service = service.lock().unwrap();
        //             println!("Service with ip {}, hostname {}, command {}, image {}, shared {}, reuse {}, replicas {}, replica_id {}, supervisor {},supervisor_port {}",
        //          service.ip,service.hostname,service.command,service.image,service.shared,service.reuse,service.replicas,service.replica_id,service.supervisor,service.supervisor_port);
        //         }
        //     }
        //     for (name, bridges) in &self.bridges_by_name {
        //         for bridge in bridges{
        //             let bridge = bridge.lock().unwrap();
        //             println!("Bridge with name {}",bridge.hostname);
        //         }
        //     }
        // for (_id, link) in &self.links {
        //       link.lock().unwrap().print(name.clone());
        // }

        for (_id, path) in &self.paths {
            path.lock().await.print(&name);
        }

        //     for (ip, id) in &self.ip_to_path_id {
        //          println!("ip is {} and id is {}",ip,id);
        //     }

        //     for (id, ip) in &self.path_id_to_ip {
        //         println!("id is {} and ip is {}",id,ip);
        //    }
    }

    //gets the last amount of bytes sent to an ip
    pub async fn get_lastbytes(&self, ip: &u32) -> u32 {
        match self.services.get(ip) {
            Some(service) => return service.lock().await.last_bytes,
            None => 0,
        }
    }

    //sets the last amount of bytes sent to an ip
    pub async fn set_lastbytes(&mut self, ip: &u32, last_bytes: u32) {
        match self.services.get_mut(ip) {
            Some(service) => service.lock().await.last_bytes = last_bytes,
            None => (),
        }
    }

    // creates a graph from an older graph
    pub async fn create_from_graph(&mut self, old_graph: &Graph) {
        for (ip, service) in old_graph.services.iter() {
            let hostname = service.lock().await.hostname.clone();

            match self.services_by_name.get_mut(&hostname) {
                Some(services) => services.push(Arc::clone(&service)),
                None => {
                    let mut new_services = vec![];
                    new_services.push(Arc::clone(&service));
                    self.services_by_name.insert(hostname, new_services);
                }
            };

            self.services.insert(*ip, service.clone());
        }

        for (ip, bridge) in old_graph.bridges.iter() {
            let hostname = bridge.lock().await.hostname.clone();

            match self.bridges_by_name.get_mut(&hostname.to_string()) {
                Some(bridges) => bridges.push(Arc::clone(&bridge)),
                None => {
                    let mut new_bridges = vec![];
                    new_bridges.push(Arc::clone(&bridge));
                    self.bridges_by_name
                        .insert(hostname.to_string(), new_bridges);
                }
            };

            self.bridges.insert(*ip, bridge.clone());
        }

        self.removed_links = old_graph.removed_links.clone();
        self.removed_bridges = old_graph.removed_bridges.clone();

        for (_id, link) in old_graph.links.iter() {
            let link = link.lock().await;

            let id = link.id;
            let latency = link.latency;
            let jitter = link.jitter;
            let drop = link.drop;
            let bandwidth = link.bandwidth;
            let source = link.source.clone();
            let destination = link.destination.clone();
            let link = Link::new(id, latency, jitter, drop, bandwidth, source, destination);
            let link = Arc::new(Mutex::new(link));
            self.links.insert(id, link);
        }

        for link in self.removed_links.iter() {
            let link = link.lock().await;

            let id = link.id;
            let latency = link.latency;
            let jitter = link.jitter;
            let drop = link.drop;
            let bandwidth = link.bandwidth;
            let source = link.source.clone();
            let destination = link.destination.clone();
            let link = Link::new(id, latency, jitter, drop, bandwidth, source, destination);
            let link = Arc::new(Mutex::new(link));
            self.links.insert(id, link);
        }

        self.link_counter = old_graph.link_counter.clone();
        self.graph_root = old_graph.graph_root.clone();
    }

    pub async fn calculate_properties(&mut self) {
        for (id, _path) in self.paths.clone() {
            self.calculate_end_to_end_properties(id).await;
        }
    }

    pub async fn calculate_end_to_end_properties(&mut self, id: u32) {
        let mut total_not_drop_probability = 1.0;

        match self.paths.get_mut(&id) {
            Some(path) => {
                let mut path = path.lock().await;

                for link in path.links.clone() {
                    match self.links.get(&link) {
                        Some(link_object) => {
                            let link_object = link_object.lock().await;

                            //confusing in python
                            if path.max_bandwidth == 0.0 {
                                path.max_bandwidth = link_object.bandwidth;
                                path.current_bandwidth = link_object.bandwidth;
                            }

                            if link_object.bandwidth < path.max_bandwidth {
                                path.max_bandwidth = link_object.bandwidth;
                                path.current_bandwidth = link_object.bandwidth;
                            }

                            path.jitter = ((path.jitter * path.jitter) as f32
                                + (link_object.jitter * link_object.jitter))
                                .sqrt();

                            path.latency += link_object.latency;
                            total_not_drop_probability *= 1.0 - link_object.drop;
                        }
                        None => (),
                    }
                }

                path.rtt = path.latency * 2.0;
                path.drop = 1.0 - total_not_drop_probability;
                //print_message(self.name.clone(),format!{"In e to e: start is {} finish is {} bw is {} latency is {}",start,finish,path.current_bandwidth,path.latency}.to_string());
            }
            None => (),
        }
    }

    //Processes usages received from eBPF
    pub async fn process_usage(&mut self, ip: u32, throughput: f32) -> bool {
        let errormargin = 0.01;
        let path_id = match self.ip_to_path_id.get_mut(&ip) {
            Some(path_id) => path_id,
            None => return false, //error
        };

        let path = match self.paths.get_mut(path_id) {
            Some(path) => path,
            None => return false, //error
        };

        let path_max_bandwidth = path.lock().await.max_bandwidth.clone();

        if throughput <= (path_max_bandwidth * errormargin) {
            path.lock().await.used_bandwidth = throughput;
            return false;
        }

        path.lock().await.used_bandwidth = throughput;

        return true;
    }

    pub fn collect_flow_u16(&mut self, bandwidth: f32, link_count: u16, ids: Vec<u16>) {
        let link_count_usize = link_count as usize;
        let key = format!("{}:{}", ids[0], ids[link_count_usize - 1]);

        match self.flow_accumulator_u16.get_mut(&key) {
            Some(flow) => {
                let mut flow = flow.blocking_lock();
                flow.bandwidth = bandwidth;
                flow.age = 0;
            }
            //If it doesn't exist insert
            None => {
                let new_flow_u16 = Arc::new(Mutex::new(Flowu16::new(bandwidth, ids.clone())));
                self.flow_accumulator_u16.insert(key.clone(), new_flow_u16);
                self.flow_accumulator_keys
                    .insert(key.clone(), "".to_string());
            }
        }
    }

    pub async fn set_graph_root(&mut self, self_addr: u32) -> Result<(), ()> {
        match self.services.get_mut(&self_addr) {
            Some(root) => {
                self.graph_root = Some(Arc::clone(root));
                Ok(())
            }
            None => {
                error!(
                    "EC {}: couldn't set graph root for self IP {}",
                    self.name, self_addr
                );
                Err(())
            }
        }
    }

    pub async fn get_name(&mut self) -> String {
        let name = self
            .graph_root
            .as_ref()
            .unwrap()
            .lock()
            .await
            .hostname
            .clone();
        self.name = name.clone();
        return name.clone();
    }

    pub async fn calculate_shortest_paths(&mut self) {
        if self.graph_root.is_none() {
            debug!("EC {} - graph root is none", self.name);
        }

        let inf: f32 = INFINITY;

        let mut dist = HashMap::new();

        let mut q = vec![];

        let root_ip = self.graph_root.as_ref().unwrap().lock().await.ip;

        for (_name, services) in self.services_by_name.iter_mut() {
            for service in services {
                let s = service.lock().await;
                if s.supervisor {
                    info!("EC {} - skipped dashboard in paths", self.name);
                    continue;
                }

                let mut distance = 0.0;

                if s.ip != root_ip {
                    distance = inf;
                }
                dist.insert(s.ip, distance);

                let entry = Dijkstraentry::new(distance, service.clone());
                q.push(entry);
            }
        }

        for (_name, bridges) in self.bridges_by_name.iter_mut() {
            dist.insert(bridges[0].lock().await.ip.clone(), inf);

            let entry = Dijkstraentry::new(inf, bridges[0].clone());
            q.push(entry);
        }

        self.insert_path(self.path_counter, vec![]);

        self.insert_ip_to_path(root_ip, self.path_counter);

        self.path_counter += 1;

        loop {
            if q.is_empty() {
                break;
            }
            q.sort_by(|a, b| a.distance.partial_cmp(&b.distance).unwrap());

            let node = q.remove(0).node.clone();

            let links = node.lock().await.links.clone();

            let mut alt;

            for link in links {
                let node_ip = node.lock().await.ip;

                alt = dist.get(&node_ip).unwrap() + 1.0;

                let link_object = self.links.get_mut(&link).unwrap().clone();

                let destination_ip = link_object.lock().await.destination.lock().await.ip;

                if !dist.contains_key(&destination_ip) {
                    continue;
                }

                if alt < *dist.get(&destination_ip).unwrap() {
                    dist.insert(destination_ip, alt);

                    let path_id = self.ip_to_path_id.get(&node_ip).unwrap();

                    let mut new_links = self
                        .paths
                        .get_mut(&path_id)
                        .unwrap()
                        .lock()
                        .await
                        .links
                        .clone();

                    new_links.push(link);
                    let new_path_id = self.path_counter.clone();

                    self.insert_path(new_path_id, new_links);

                    self.insert_ip_to_path(destination_ip, new_path_id);

                    self.path_counter += 1;

                    for i in 0..q.len() {
                        if q[i].node.lock().await.ip == destination_ip {
                            q[i].distance = alt;
                        }
                    }
                }
            }
        }

        self.set_start_and_finish().await;
    }

    pub async fn calculate_shortest_paths_latency(&mut self) {
        if self.graph_root.is_none() {
            debug!("EC {} - graph root is none", self.name);
        }

        let inf: f32 = INFINITY;

        let mut dist = HashMap::new();

        let mut q = vec![];

        let root_ip = self.graph_root.as_ref().unwrap().lock().await.ip;

        for (_name, services) in self.services_by_name.iter_mut() {
            for service in services {
                let s = service.lock().await;
                let mut distance = 0.0;

                if s.ip != root_ip {
                    distance = inf;
                }
                dist.insert(s.ip, distance);

                let entry = Dijkstraentry::new(distance, service.clone());
                q.push(entry);
            }
        }

        for (_name, bridges) in self.bridges_by_name.iter_mut() {
            dist.insert(bridges[0].lock().await.ip.clone(), inf);

            let entry = Dijkstraentry::new(inf, bridges[0].clone());
            q.push(entry);
        }

        self.insert_path(self.path_counter, vec![]);

        self.insert_ip_to_path(root_ip, self.path_counter);

        self.path_counter += 1;

        loop {
            if q.is_empty() {
                break;
            }
            q.sort_by(|a, b| a.distance.partial_cmp(&b.distance).unwrap());

            let node = q.remove(0).node.clone();

            let links = node.lock().await.links.clone();

            let mut alt;

            for link in links {
                let node_ip = node.lock().await.ip;

                let link_object = self.links.get_mut(&link).unwrap().clone();

                let latency;
                let destination_ip;
                {
                    let l = link_object.lock().await;
                    latency = l.latency;
                    destination_ip = l.destination.lock().await.ip;
                }

                alt = dist.get(&node_ip).unwrap() + latency;
                if !dist.contains_key(&destination_ip) {
                    continue;
                }

                if alt < *dist.get(&destination_ip).unwrap() {
                    dist.insert(destination_ip, alt);

                    let path_id = self.ip_to_path_id.get(&node_ip).unwrap();

                    let mut new_links = self
                        .paths
                        .get_mut(&path_id)
                        .unwrap()
                        .lock()
                        .await
                        .links
                        .clone();

                    new_links.push(link);
                    let new_path_id = self.path_counter.clone();

                    self.insert_path(new_path_id, new_links);

                    self.insert_ip_to_path(destination_ip, new_path_id);

                    self.path_counter += 1;

                    for i in 0..q.len().clone() {
                        if q[i].node.lock().await.ip == destination_ip {
                            q[i].distance = alt;
                        }
                    }
                }
            }
        }

        self.set_start_and_finish().await;
    }

    pub async fn set_start_and_finish(&mut self) {
        for (_id, path) in self.paths.iter_mut() {
            let mut p = path.lock().await;
            if p.links.is_empty() {
                continue;
            }

            let source_id = p.links.first().unwrap();
            let source_link = self.links.get(source_id).unwrap().to_owned();

            let destination_id = p.links.last().unwrap();
            let destination_link = self.links.get(destination_id).unwrap().to_owned();

            let source_name = source_link
                .lock()
                .await
                .source
                .lock()
                .await
                .hostname
                .clone();

            let destination_name = destination_link
                .lock()
                .await
                .destination
                .lock()
                .await
                .hostname
                .clone();

            p.start = source_name;
            p.finish = destination_name;
        }
    }

    pub async fn recompute_properties(&mut self, shortest_path_type: &str) {
        match shortest_path_type {
            "hop" => self.calculate_shortest_paths().await,
            "latency" => self.calculate_shortest_paths_latency().await,
            _ => error!("shortest path type must be either 'hop' or 'latency'"),
        }
        self.calculate_properties().await;
    }
}
