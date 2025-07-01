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

use std::sync::{Arc, Mutex};

use tracing::{debug, error, info};

use crate::aux::print_message;
use crate::elements::{Flowu16, Link, Path};
use crate::emulation::Emulation;
use crate::graph::Graph;
use std::collections::HashMap;
use std::io::Result;

//Represents the State of the Emulation
pub struct State {
    pub age: usize, //Age represents which graph we currently have, a dynamic event that changes paths represent a new age
    pub graphs: Vec<Arc<Mutex<Graph>>>, //Vector containing the precomputed graphs
    pub graph_counter: usize, //auxiliary value with the number of graphs
    pub active_paths: Vec<Arc<Mutex<Path>>>, //paths that this container sent bytes to
    pub active_paths_ids: Vec<u32>,
    pub name: String,
    pub emulation: Arc<Mutex<Emulation>>, //object that handles TC
    pub id: String,
    pub link_count: u32,
    pub max_age: u32,
    pub ec_cycle: u16,
}

impl State {
    pub fn new(id: String) -> State {
        State {
            age: 0,
            graphs: vec![],
            graph_counter: 0,
            active_paths: vec![],
            active_paths_ids: vec![],
            name: "".to_string(),
            emulation: Arc::new(Mutex::new(Emulation::new())),
            id: id,
            link_count: 0,
            max_age: 2,
            ec_cycle: 0,
        }
    }

    //Here we setup the TC qdisc to emulate the network state
    pub fn init(&mut self, ip: u32) -> Result<()> {
        let mut emulation = self.emulation.lock().unwrap();
        let current_graph_handle = self.get_current_graph();
        let current_graph = current_graph_handle.lock().unwrap();
        let current_graph_root = current_graph.graph_root.as_ref().unwrap().lock().unwrap();

        emulation.init(ip);

        let ip_to_path_id = current_graph.ip_to_path_id.clone();

        let ips = current_graph.ips.clone();

        let activepaths = current_graph_root.activepaths.clone();

        let mut opened_paths = vec![];

        //self.get_current_graph().lock().unwrap().print_graph(self.name.clone());

        //If we do not have active paths we do not know which paths need to be shaped so we shape all
        if activepaths.is_empty() {
            let mut paths = current_graph.paths.clone();

            for (id, path) in paths.iter_mut() {
                let p = path.lock().unwrap();

                let service_name = p.finish.clone();

                if current_graph.bridges_by_name.contains_key(&service_name) {
                    continue;
                }

                let bandwidth = (p.max_bandwidth / 1000.0) as u32;

                let latency = p.latency;

                let jitter = p.jitter;

                let drop = p.drop;

                let start = p.start.clone();

                let finish = p.finish.clone();

                let links = p.links.clone();

                let ip = current_graph.path_id_to_ip.get(id).unwrap().to_owned();

                if p.links.is_empty() {
                    print_message(
                        self.name.clone(),
                        format!("disabled due to no links path to {} with ip {}", finish, ip),
                    );
                    emulation.disable_path(ip);
                // else is a path to another container
                } else {
                    print_message(
                        self.name.clone(),
                        format!(
                            "In init: origin is {} dest is {} latency is {} links are {:?} bw is {} drop is {}",
                            start, finish, latency, links, bandwidth, drop
                        ),
                    );
                    emulation.initialize_path(ip, bandwidth, latency, jitter, drop);
                    opened_paths.push(service_name.clone());
                }
            }

            let services = current_graph.services.clone();
            for ip in ips {
                if ip_to_path_id.get(&ip).is_none() {
                    let service = services.get(&ip);
                    let mut name = "".to_string();
                    if !service.is_none() {
                        name = service.unwrap().lock().unwrap().hostname.clone();
                    }
                    print_message(
                        self.name.clone(),
                        format!("Disabled due to no path to {} with name {}", ip, name),
                    );
                    emulation.disable_path(ip);
                }
            }
        }
        //If we do know active paths, we only shape those paths
        else {
            let mut opened_paths = vec![];
            //opens from me to them
            for service_name in activepaths {
                let ip = current_graph
                    .services_by_name
                    .get(&service_name)
                    .unwrap()
                    .first()
                    .unwrap()
                    .lock()
                    .unwrap()
                    .ip;

                let path_id = current_graph.ip_to_path_id.get(&ip).unwrap();

                let path = current_graph.paths.get(path_id).unwrap().clone();
                let p = path.lock().unwrap();

                let bandwidth = (p.max_bandwidth / 1000.0) as u32;

                let latency = p.latency;

                let jitter = p.jitter;

                let drop = p.drop;

                if p.links.is_empty() {
                    emulation.disable_path(ip);
                // else is a path to another container
                } else {
                    emulation.initialize_path(ip, bandwidth, latency, jitter, drop);
                    opened_paths.push(service_name);
                }
            }

            let my_name = current_graph_root.hostname.clone();

            let services = current_graph.services.clone();

            for (ip, service) in services.iter() {
                let s = service.lock().unwrap();
                let service_name = s.hostname.clone();

                //if we for some reason create 2 paths (qdiscs) to the same hosts then dynamic changes will not work
                if s.activepaths.contains(&my_name) && !opened_paths.contains(&service_name) {
                    let path_id = current_graph.ip_to_path_id.get(&ip).unwrap();

                    let path = current_graph.paths.get(path_id).unwrap().clone();
                    let p = path.lock().unwrap();

                    let bandwidth = (p.max_bandwidth / 1000.0) as u32;

                    let latency = p.latency;

                    let jitter = p.jitter;

                    let drop = p.drop;

                    if p.links.is_empty() {
                        emulation.disable_path(*ip);
                    // else is a path to another container
                    } else {
                        emulation.initialize_path(*ip, bandwidth, latency, jitter, drop);
                    }
                }
            }
        }
        Ok(())
    }

    pub fn set_link_count(&mut self, link_count: u32) {
        self.link_count = link_count;

        for graph in self.graphs.iter_mut() {
            graph.lock().unwrap().link_count = link_count
        }
    }

    //Creates and inserts an empty graph in the vector
    pub fn insert_graph(&mut self) {
        let graph = Arc::new(Mutex::new(Graph::new()));
        if !self.graphs.is_empty() {
            graph
                .lock()
                .unwrap()
                .create_from_graph(self.get_graph_most_recent().clone());
        }
        self.graphs.push(graph);
        self.graph_counter += 1;
    }

    //Returns the graph the emulation is using
    pub fn get_current_graph(&self) -> Arc<Mutex<Graph>> {
        return Arc::clone(&self.graphs[self.age]);
    }

    //Returns the most recent graph we are editing (temporary only used to pass the graphs to rust)
    pub fn get_graph_most_recent(&mut self) -> Arc<Mutex<Graph>> {
        return Arc::clone(&self.graphs[self.graph_counter - 1]);
    }

    //Returns a graph based on age
    pub fn get_graph_with_age(&mut self, age: usize) -> Arc<Mutex<Graph>> {
        return Arc::clone(&self.graphs[age]);
    }

    pub fn shrink_maps(&mut self) {
        for graph in self.graphs.iter_mut() {
            let mut graph = graph.lock().unwrap();
            {
                graph.services.shrink_to_fit();
                graph.paths.shrink_to_fit();
                graph.links.shrink_to_fit();
                graph.bridges.shrink_to_fit();
                graph.services_by_name.shrink_to_fit();
                graph.bridges_by_name.shrink_to_fit();
                graph.ip_to_path_id.shrink_to_fit();
                graph.path_id_to_ip.shrink_to_fit();
            }
        }
    }

    //Increments the age and updates the new structures with older emulation metadata
    pub fn increment_age(&mut self) {
        info!("EC {}: started changing properties", self.name);
        self.path_changes();
        self.collect_old_usages();

        self.age += 1;

        //clear because we are reactive
        self.active_paths_ids.clear();
    }

    //Retrieves bandwidth values of older graph
    pub fn path_changes(&mut self) {
        let age = self.age;

        //Get current ip_to_id (can be cloned just u32 values)
        let ip_to_path_ids = self
            .get_graph_with_age(age + 1)
            .lock()
            .unwrap()
            .ip_to_path_id
            .clone();

        //Get old ip_to_path_ids (can be cloned just u32 values)
        let old_ip_to_path_ids = self
            .get_graph_with_age(age)
            .lock()
            .unwrap()
            .ip_to_path_id
            .clone();

        for (ip, id) in &old_ip_to_path_ids {
            match ip_to_path_ids.get(&ip) {
                Some(_new_id) => {
                    continue;
                }
                None => {
                    let root_ip = self
                        .get_current_graph()
                        .lock()
                        .unwrap()
                        .graph_root
                        .as_ref()
                        .unwrap()
                        .lock()
                        .unwrap()
                        .ip;
                    if root_ip == *ip {
                        continue;
                    }
                    let start = self
                        .get_graph_with_age(age)
                        .lock()
                        .unwrap()
                        .get_path(*id)
                        .unwrap()
                        .lock()
                        .unwrap()
                        .start
                        .clone();
                    let finish = self
                        .get_graph_with_age(age)
                        .lock()
                        .unwrap()
                        .get_path(*id)
                        .unwrap()
                        .lock()
                        .unwrap()
                        .finish
                        .clone();

                    if self
                        .get_graph_with_age(age)
                        .lock()
                        .unwrap()
                        .bridges_by_name
                        .contains_key(&finish)
                    {
                        continue;
                    }

                    print_message(
                        self.name.clone(),
                        format!("Blocked path from {} to {}", start, finish),
                    );
                    self.emulation.lock().unwrap().change_loss(*ip, 1.0);
                }
            }
        }

        //Iterate through the new ones
        for (ip, id) in &ip_to_path_ids {
            //If it is the connection to myself skip
            let root_ip;
            {
                root_ip = self
                    .get_current_graph()
                    .lock()
                    .unwrap()
                    .graph_root
                    .as_ref()
                    .unwrap()
                    .lock()
                    .unwrap()
                    .ip;
            }
            if root_ip == *ip {
                continue;
            }
            //Check old ones to see if there was a path to the IP
            match old_ip_to_path_ids.get(&ip) {
                //If there was get old bandwidth values
                Some(old_id) => {
                    match self
                        .get_graph_with_age(age)
                        .lock()
                        .unwrap()
                        .paths
                        .get_mut(old_id)
                    {
                        Some(path) => {
                            let p = path.lock().unwrap();
                            let used_bandwidth = p.used_bandwidth.clone();
                            let current_bandwidth = p.current_bandwidth.clone();

                            let new_path_handle = self
                                .get_graph_with_age(age + 1)
                                .lock()
                                .unwrap()
                                .get_path(*id)
                                .clone();

                            if new_path_handle.is_none() {
                                continue;
                            }

                            let new_path_handle = new_path_handle.unwrap();
                            let mut new_path = new_path_handle.lock().unwrap();

                            let finish = new_path.finish.clone();

                            if self
                                .get_graph_with_age(age + 1)
                                .lock()
                                .unwrap()
                                .bridges_by_name
                                .contains_key(&finish)
                            {
                                print_message(self.name.clone(), "Continued".to_string());
                                continue;
                            }

                            new_path.set_used_bandwidth(used_bandwidth);
                            new_path.set_current_bandwidth(current_bandwidth);

                            let new_latency = new_path.latency;
                            let new_jitter = new_path.jitter;
                            let new_loss = new_path.drop;
                            //print_message(self.name.clone(),format!("Changed path from {} to {} and new latency is {} and new drop is {}",start,finish,new_latency.clone(),new_loss.clone()));
                            let mut emulation = self.emulation.lock().unwrap();
                            emulation.change_loss(*ip, new_loss);

                            if new_latency != 0.0 {
                                emulation.change_latency(*ip, new_latency, new_jitter);
                            }
                        }
                        None => {}
                    }
                }
                None => {
                    let graph_handle = self.get_graph_with_age(age + 1);
                    let graph = graph_handle.lock().unwrap();
                    let new_path_handle = graph.get_path(*id).unwrap();
                    let new_path = new_path_handle.lock().unwrap();

                    let start = new_path.start.clone();

                    let finish = new_path.finish.clone();

                    if graph.bridges_by_name.contains_key(&finish) {
                        continue;
                    }

                    let new_latency = new_path.latency;
                    let new_jitter = new_path.jitter;
                    let new_loss = new_path.drop;
                    print_message(
                        self.name.clone(),
                        format!(
                            "New path from {} to {} and new latency is {} and new drop is {}",
                            start, finish, new_latency, new_loss
                        ),
                    );
                    let mut emulation = self.emulation.lock().unwrap();
                    emulation.change_loss(*ip, new_loss);

                    if new_latency != 0.0 {
                        emulation.change_latency(*ip, new_latency, new_jitter);
                    }
                }
            };
        }
    }

    //Retrieve the amount of bytes sent to services to update in the new HashMap
    pub fn collect_old_usages(&mut self) {
        let age = self.age;
        let services_ips = self.get_graph_with_age(age + 1).lock().unwrap().ips.clone();

        for ip in services_ips {
            match self
                .get_graph_with_age(age)
                .lock()
                .unwrap()
                .services
                .get(&ip)
            {
                Some(service_old) => {
                    let last_bytes = service_old.lock().unwrap().last_bytes;
                    self.get_graph_with_age(age + 1)
                        .lock()
                        .unwrap()
                        .set_lastbytes(&ip, last_bytes);
                }
                None => {}
            }
        }
    }

    pub fn insert_active_path_id(&mut self, ip: u32) {
        let path_id = self
            .get_current_graph()
            .lock()
            .unwrap()
            .get_path_id_from_ip(ip);

        match path_id {
            Some(id) => {
                if id == 0 {
                    return;
                }
                if !self.active_paths_ids.contains(&id) {
                    self.active_paths_ids.push(id);
                }
            }
            None => {
                return;
            }
        };
    }

    pub fn calculate_bandwidth(&mut self) {
        let rtt = 0;
        let bw = 1;

        self.ec_cycle += 1;
        let mut active_links_ids = vec![];

        //add info about our flows

        let mut flows_to_remove: Vec<String> = vec![];

        for path_id in self.active_paths_ids.clone() {
            let path_handle = self.get_path(path_id).unwrap().clone();
            let path = path_handle.lock().unwrap();

            let links = path.links.clone();
            let pathrtt = path.rtt;
            let path_used_bandwidth = path.used_bandwidth;

            for link_id in links {
                let link = self.get_link(link_id);
                let mut flow = vec![];
                flow.push(pathrtt);
                flow.push(path_used_bandwidth);
                link.lock().unwrap().flows.push(flow);
                active_links_ids.push(link_id);
            }
        }

        //add info about other flows
        let flow_keys = self.get_flow_keys();

        for (key, _value) in flow_keys.iter() {
            let flow = self.get_flow_u16(&key);
            let age = flow.lock().unwrap().age;

            if age < self.max_age {
                self.apply_flow_u16(&key);
                let link_indices = flow.lock().unwrap().link_indices.clone();

                for link_id in link_indices {
                    active_links_ids.push(link_id as u16);
                }
                flow.lock().unwrap().age = age + 1;
            } else {
                flows_to_remove.push(key.clone());
            }
        }
        //Calculations

        for path_id in self.active_paths_ids.clone() {
            let path_handle = self.get_path(path_id).unwrap().clone();

            let mut path = path_handle.lock().unwrap();

            let mut max_bandwidth = path.max_bandwidth;
            let max_bandwidth_path = path.max_bandwidth;

            for link_id in path.links.clone() {
                let mut rtt_reverse_sum = 0.000;

                let _link = self.get_link(link_id);
                let link = _link.lock().unwrap();
                let link_flows = link.flows.clone();

                if link_flows.is_empty() {
                    continue;
                }

                for flow in link_flows.clone() {
                    rtt_reverse_sum += 1.0 / flow[rtt];
                }

                let mut max_bandwidth_on_link = vec![];

                //calculate our bandwidth
                let bandwidth_bps = link.bandwidth;
                let value = 1.0 / link_flows[0][rtt];

                let max_bandwidth_element = (value / rtt_reverse_sum) * bandwidth_bps;
                //calculated our bandwidth push to vector
                max_bandwidth_on_link.push(max_bandwidth_element);

                //maximize link utilization to 100%

                let mut spare_bw = bandwidth_bps - max_bandwidth_on_link[0];

                let our_share = max_bandwidth_on_link[0] / bandwidth_bps;

                let mut hungry_usage_sum = our_share;

                for position in 1..link_flows.len() {
                    let flow = &link_flows[position];
                    //calculate bandwidth for everyone
                    let value = 1.0 / flow[rtt];

                    let max_bandwidth_element = (value / rtt_reverse_sum) * bandwidth_bps;

                    max_bandwidth_on_link.push(max_bandwidth_element);
                    //check if a flow is hungry (wants more than its allocated share)
                    if flow[bw] > max_bandwidth_on_link[position] {
                        spare_bw -= max_bandwidth_on_link[position];

                        hungry_usage_sum += max_bandwidth_on_link[position] / bandwidth_bps;
                    } else {
                        spare_bw -= flow[bw]
                    }
                }
                //we get a share of the spare proportional to our RTT
                let normalized_share = our_share / hungry_usage_sum;

                let maximized = max_bandwidth_on_link[0] + (normalized_share * spare_bw);

                if maximized > max_bandwidth_on_link[0] {
                    max_bandwidth_on_link[0] = maximized;
                }
                //If this link restricts us more than previously try to assume this bandwidth as the max
                if max_bandwidth_on_link[0] < max_bandwidth {
                    max_bandwidth = max_bandwidth_on_link[0];
                }
            }

            let current_bandwidth = path.current_bandwidth;

            if max_bandwidth <= max_bandwidth_path && max_bandwidth != current_bandwidth {
                if max_bandwidth <= current_bandwidth {
                    path.set_current_bandwidth(max_bandwidth);
                } else {
                    //TODO check max before changing
                    let new_current_bandwidth = 0.75 * current_bandwidth + 0.25 * max_bandwidth;

                    path.set_current_bandwidth(new_current_bandwidth);
                }
                //get_services
                let ip = self
                    .get_current_graph()
                    .lock()
                    .unwrap()
                    .get_ip_from_path_id(path_id)
                    .clone();
                //call tc to change
                let new_bandwidth = path.current_bandwidth;

                self.emulation
                    .lock()
                    .unwrap()
                    .change_bandwidth(ip, new_bandwidth as u32);
            }
        }

        for id in active_links_ids {
            self.clear_link(id as u16);
        }

        for key in flows_to_remove {
            let current_graph_handle = self.get_current_graph();
            let mut current_graph = current_graph_handle.lock().unwrap();
            current_graph.flow_accumulator_u16.remove(&key);
            current_graph.flow_accumulator_keys.remove(&key);
        }
    }

    //Add flow received from CM to our state
    pub fn apply_flow_u16(&mut self, key: &String) {
        let flow_handle = self.get_flow_u16(key);
        let flow = flow_handle.lock().unwrap();

        let link_indices = flow.link_indices.clone();
        let path_used_bandwidth = flow.bandwidth;
        let mut pathrtt = 0.0;
        //calculate RTT
        for index in link_indices.clone() {
            let link = self.get_link(index as u16);
            let rtt = link.lock().unwrap().latency * 2.0;
            pathrtt += rtt;
        }

        for index in link_indices {
            let link = self.get_link(index as u16);

            let mut flow = vec![];

            flow.push(pathrtt);

            flow.push(path_used_bandwidth);

            link.lock().unwrap().flows.push(flow);
        }
    }

    pub fn get_link(&mut self, id: u16) -> Arc<Mutex<Link>> {
        match self.get_current_graph().lock().unwrap().links.get_mut(&id) {
            Some(link) => {
                return link.clone();
            }
            None => {
                error!("EC {}: link {} does not exist", self.name, id);
                std::process::exit(0);
            }
        };
    }

    pub fn clear_link(&mut self, id: u16) {
        self.get_current_graph()
            .lock()
            .unwrap()
            .links
            .get_mut(&id)
            .unwrap()
            .lock()
            .unwrap()
            .clear_flows();
    }

    pub fn get_path(&mut self, id: u32) -> Option<Arc<Mutex<Path>>> {
        return self.get_current_graph().lock().unwrap().get_path(id);
    }

    pub fn get_flow_u16(&mut self, key: &String) -> Arc<Mutex<Flowu16>> {
        return self
            .get_current_graph()
            .lock()
            .unwrap()
            .flow_accumulator_u16
            .get_mut(key)
            .unwrap()
            .clone();
    }

    pub fn get_flow_keys(&mut self) -> HashMap<String, String> {
        return self
            .get_current_graph()
            .lock()
            .unwrap()
            .flow_accumulator_keys
            .clone();
    }
    pub fn clear_paths(&mut self) {
        self.active_paths.clear();
        self.active_paths_ids.clear();
    }

    pub fn get_active_paths(&mut self) -> Vec<Arc<Mutex<Path>>> {
        let mut paths = vec![];

        for id in &self.active_paths_ids.clone() {
            paths.push(self.get_path(*id).unwrap().clone());
        }

        return paths;
    }
}
