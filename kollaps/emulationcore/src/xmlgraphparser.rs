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

use crate::aux::convert_to_int;
use crate::eventscheduler::{self, Event, EventKind};
use crate::graph::Graph;
use rand::prelude::*;
use rand_pcg::Pcg64;
use random_string::{charsets, generate};
use regex::Regex;
use roxmltree::{Document, Node};
use std::net::IpAddr;
use std::str::FromStr;
use tracing::warn;

pub struct Config {
    pub ips: Vec<String>,
    pub controller_ip: String,
    pub shortest_path_type: String,
    pub pool_period: f32,
    pub max_age: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            ips: Vec::new(),
            controller_ip: String::new(),
            shortest_path_type: "hop".to_string(),
            pool_period: 0.05,
            max_age: 2,
        }
    }
}

pub struct XMLGraphParser<'a> {
    document: Document<'a>,
    mode: String,
}

impl<'a> XMLGraphParser<'a> {
    pub fn try_new(text: &'a str, mode: String) -> Result<Self, roxmltree::Error> {
        let document = Document::parse(&text)?;
        Ok(Self { document, mode })
    }

    /// Create and fill a `Graph` according to the supplied topology XML `roxmltree::Document`.
    /// Current implementation uses the returned graph as the initial graph for `parse_schedule`.
    pub async fn fill_graph(&self) -> (Config, Graph) {
        let mut graph = Graph::new();

        let root = self.document.root().first_child().unwrap();

        if root.tag_name().name() != "experiment" {
            panic!("Not a valid Kollap topology file, root is not 'experiment'");
        }

        if !root.has_attribute("boot") {
            panic!("<experiment boot = > The experiment needs a valid boostrapper image name");
        }

        let mut config_node: Option<Node> = None;
        let mut services: Option<Node> = None;
        let mut bridges: Option<Node> = None;
        let mut links: Option<Node> = None;
        let mut dynamic: Option<Node> = None;

        root.children()
            .filter(|node| node.is_element())
            .for_each(|node| match node.tag_name().name() {
                "config" => {
                    if config_node.is_none() {
                        config_node = Some(node);
                    }
                }
                "services" => {
                    if services.is_some() {
                        panic!("Only one <services> block is allowed.");
                    }
                    services = Some(node);
                }
                "bridges" => {
                    if bridges.is_some() {
                        panic!("Only one <bridges> block is allowed.");
                    }
                    bridges = Some(node);
                }
                "links" => {
                    if links.is_some() {
                        panic!("Only one <links> block is allowed.");
                    }
                    links = Some(node);
                }
                "dynamic" => {
                    if dynamic.is_some() {
                        panic!("Only one <dynamic> block is allowed.");
                    }
                    dynamic = Some(node);
                }
                _ => (),
            });

        let mut config = Config::default();

        if let Some(c) = config_node {
            self.parse_config(c, &mut config);
        }

        self.parse_services(
            services.expect("declared services in topology file"),
            dynamic,
            &mut config,
            &mut graph,
        )
        .await;

        if let Some(bridges) = bridges {
            self.parse_bridges(bridges, &mut graph);
        }

        self.parse_links(links.expect("declared links in topology file"), &mut graph)
            .await;

        (config, graph)
    }

    fn parse_config(&self, config_node: Node<'a, 'a>, config: &mut Config) {
        for property in config_node.children() {
            if !property.is_element() {
                continue;
            }
            if property.has_attribute("shortest_path") {
                let shortest_path_type = property.attribute("shortest_path").unwrap();
                config.shortest_path_type = shortest_path_type.to_string();
            }

            if property.has_attribute("pool_period") {
                let pool_period: f32 = property.attribute("pool_period").unwrap().parse().unwrap();
                config.pool_period = pool_period;
            }

            if property.has_attribute("max_age") {
                let max_age: u32 = property.attribute("max_age").unwrap().parse().unwrap();
                config.max_age = max_age;
            }
        }
    }

    async fn parse_services(
        &self,
        services: Node<'a, 'a>,
        dynamic: Option<Node<'a, 'a>>,
        config: &mut Config,
        graph: &mut Graph,
    ) {
        for service in services.children() {
            if !service.is_element() {
                continue;
            }
            if service.tag_name().name() != "service" {
                panic!(
                    "Invalid tag inside <services> {}",
                    service.tag_name().name()
                );
            }
            if self.mode == "container"
                && (!service.has_attribute("name") || !service.has_attribute("image"))
            {
                panic!("A service needs a name and an image attribute.");
            }
            if self.mode == "baremetal" && !service.has_attribute("name") {
                panic!("A service needs a name.");
            }
            let mut paths: Vec<String> = vec![];
            if service.has_attribute("activepaths") {
                paths = self.process_active_paths(service.attribute("activepaths").unwrap());
            }

            // let mut command = "";
            // if service.has_attribute("command"){
            //     command = service.attribute("command").unwrap();
            // }

            let mut shared = false;

            if service.has_attribute("share") {
                shared = service.attribute("share").unwrap() == "true";
            }

            let mut supervisor = false;

            let mut supervisor_port = 0;
            if service.has_attribute("supervisor") {
                supervisor = true;

                if service.has_attribute("port") {
                    supervisor_port = service.attribute("port").unwrap().parse().unwrap();
                }
            }

            let mut reuse = true;

            if service.has_attribute("reuse") {
                reuse = service.attribute("reuse").unwrap() == "true";
            }

            let mut replicas = 1;

            if service.has_attribute("replicas") {
                replicas = service.attribute("replicas").unwrap().parse().unwrap();
            }

            replicas = self.calculate_required_replicas(dynamic, service, replicas, reuse);

            for _i in 0..replicas {
                if self.mode == "container" {
                    let name = service.attribute("name").unwrap().to_string();
                    let image = service.attribute("image").map(|i| i.to_string());
                    let command = service.attribute("command").map(|s| s.to_string());

                    graph
                        .insert_service(
                            name,
                            shared,
                            reuse,
                            replicas,
                            None,
                            paths.clone(),
                            None,
                            image,
                            command,
                        )
                        .await;
                }

                if self.mode == "baremetal" {
                    let name = service.attribute("name").unwrap().to_string();

                    let script = service.attribute("script");

                    let ip = service.attribute("ip");

                    if let Some(ip) = ip {
                        config.ips.push(ip.to_string());
                        let ip = IpAddr::from_str(ip).unwrap();
                        match ip {
                            IpAddr::V4(ipv4) => {
                                graph
                                    .insert_service(
                                        name,
                                        shared,
                                        reuse,
                                        replicas,
                                        Some(convert_to_int(ipv4.octets())),
                                        paths.clone(),
                                        script,
                                        None,
                                        None,
                                    )
                                    .await;
                            }
                            IpAddr::V6(_) => {
                                panic!("IPv6 is not supported");
                            }
                        }
                    }
                }

                if supervisor {
                    let name = service.attribute("name").unwrap().to_string();
                    graph.set_dashboard(name, supervisor_port).await;

                    if self.mode == "baremetal" {
                        let controller_ip = service.attribute("controller_ip");
                        config.controller_ip = controller_ip
                            .expect("controller requires an IP")
                            .to_string();
                    }
                }
            }
        }
    }

    fn process_active_paths(&self, activepaths: &str) -> Vec<String> {
        let activepaths = activepaths.replace("[", "");
        let activepaths = activepaths.replace("]", "");

        let activepaths = activepaths.split(",");

        let mut vector_paths = vec![];
        for path in activepaths {
            let path = path.replace("'", "");
            vector_paths.push(path.to_string());
        }
        vector_paths
    }

    fn calculate_required_replicas(
        &self,
        dynamic: Option<Node>,
        service: Node,
        replicas: u32,
        reuse: bool,
    ) -> u32 {
        if dynamic.is_none() {
            return replicas;
        }

        // first we collect the join/leave/crash/disconnect/reconnect events
        // so we can later sort them and calculate the required replicas
        let mut events = vec![];

        let join_event = 1.0;
        let leave_event = 2.0;
        let crash_event = 3.0;
        let disconnect_event = 4.0;
        let reconnect_event = 5.0;

        let time = 0;

        let amount = 1;

        let event_type = 2;

        let mut has_joins = false;

        for event in dynamic.unwrap().children() {
            if !event.is_element() {
                continue;
            }
            if event.tag_name().name() != "schedule" {
                panic!("Only <schedule> is allowed inside <dynamic>");
            }

            if event.has_attribute("name")
                && event.has_attribute("time")
                && event.has_attribute("action")
            {
                if event.attribute("name").unwrap() != service.attribute("name").unwrap() {
                    continue;
                }
                //missing check on time
                let event_time: f32 = event.attribute("time").unwrap().parse().unwrap();

                if event_time.is_nan() {
                    panic!("time attribute must be a valid real number");
                }

                let mut event_amount: f32 = 1.0;

                if event.has_attribute("amount") {
                    event_amount = event.attribute("amount").unwrap().parse().unwrap();

                    if event_amount.is_nan() {
                        panic!("amount attribute must be an integer >= 1");
                    }
                }

                // parse action
                if event.attribute("action").unwrap() == "join" {
                    has_joins = true;
                    let event_entry = vec![event_time, event_amount, join_event];
                    events.push(event_entry);
                }
                if event.attribute("action").unwrap() == "leave" {
                    let event_entry = vec![event_time, event_amount, leave_event];
                    events.push(event_entry);
                }
                if event.attribute("action").unwrap() == "crash" {
                    let event_entry = vec![event_time, event_amount, crash_event];
                    events.push(event_entry);
                }
                if event.attribute("action").unwrap() == "disconnect" {
                    let event_entry = vec![event_time, event_amount, disconnect_event];
                    events.push(event_entry);
                }
                if event.attribute("action").unwrap() == "reconnect" {
                    let event_entry = vec![event_time, event_amount, reconnect_event];
                    events.push(event_entry);
                }
            }
        }

        if !has_joins {
            return replicas;
        }

        events.sort_by(|a, b| a[time].partial_cmp(&b[time]).unwrap());

        let mut max_replicas = 0.0;

        let mut cumulative_replicas = 0.0;

        let mut disconnected = 0.0;

        let mut current_replicas = 0.0;
        for event in events {
            if event[event_type] == join_event {
                current_replicas += event[amount];
                cumulative_replicas += event[amount];
            }

            if event[event_type] == leave_event || event[event_type] == crash_event {
                current_replicas -= event[amount];
            }

            if event[event_type] == disconnect_event {
                disconnected += event[amount];
                if event[amount] > current_replicas {
                    panic!(
                        "Dynamic section for {} disconnects more replicas than are joined at second {:.1}",
                        service.attribute("name").unwrap(),
                        event[time]
                    );
                }
            }

            if event[event_type] == reconnect_event {
                disconnected -= event[amount];

                if event[amount] > disconnected {
                    panic!(
                        "Dynamic section for {} reconnects more replicas than are joined at second {:.1}",
                        service.attribute("name").unwrap(),
                        event[time]
                    );
                }
            }

            if current_replicas < 0.0 {
                panic!(
                    "Dynamic section for {} causes a negative number of replicas at second {:.1}",
                    service.attribute("name").unwrap(),
                    event[time]
                );
            }
            if current_replicas > max_replicas {
                max_replicas = current_replicas;
            }
        }

        if reuse {
            max_replicas as u32
        } else {
            cumulative_replicas as u32
        }
    }

    fn parse_bridges(&self, bridges: Node<'a, 'a>, graph: &mut Graph) {
        bridges.children().filter(|b| b.is_element()).for_each(|b| {
            if b.tag_name().name() != "bridge" {
                panic!("Invalid tag inside <bridges>: {}", b.tag_name().name());
            }

            if !b.has_attribute("name") {
                panic!("A bridge needs to have a name");
            }

            graph.insert_bridge(b.attribute("name").unwrap().to_string(), None);
        });
    }

    async fn parse_links(&self, links: Node<'a, 'a>, graph: &mut Graph) {
        for link in links.children() {
            if !link.is_element() {
                continue;
            }

            let source = link.attribute("origin").unwrap().to_string();

            let destination = link.attribute("dest").unwrap().to_string();

            if !link.is_element() {
                continue;
            }
            if link.tag_name().name() != "link" {
                panic!("Invalid tag inside <link> {}", link.tag_name().name());
            }

            if self.mode == "container"
                && (!link.has_attribute("origin")
                    || !link.has_attribute("dest")
                    || !link.has_attribute("latency")
                    || !link.has_attribute("upload")
                    || !link.has_attribute("network"))
            {
                panic!("Incomplete network description");
            }

            if self.mode == "baremetal"
                && (!link.has_attribute("origin")
                    || !link.has_attribute("dest")
                    || !link.has_attribute("latency")
                    || !link.has_attribute("upload"))
            {
                panic!("Incomplete network description");
            }

            let source_node = &graph.get_nodes(source.clone())[0];
            let destination_node = &graph.get_nodes(destination.clone())[0];

            let source_node_shared_link = source_node.lock().await.shared;
            let destination_node_shared_link = destination_node.lock().await.shared;

            // let mut network = "";
            // if self.mode == "container"{
            //     network = link.attribute("network").unwrap();
            // }
            // if self.mode == "baremetal"{
            //     network = "baremetal";
            // }
            let mut jitter: f32 = 0.0;
            if link.has_attribute("jitter") {
                jitter = link.attribute("jitter").unwrap().parse().unwrap();
            }

            let mut drop: f32 = 0.0;
            if link.has_attribute("drop") {
                drop = link.attribute("drop").unwrap().parse().unwrap();
            }

            let both_shared = source_node_shared_link && destination_node_shared_link;

            let latency: f32 = link.attribute("latency").unwrap().parse().unwrap();

            if latency == 0.0 {
                warn!(
                    "Latency in a path between two containers/machines/VMs can not be 0, make sure one of the links in the path has a latency bigger than 0"
                );
            }

            let bidirectional = link.has_attribute("download");
            let has_download = link.has_attribute("download");
            let has_upload = link.has_attribute("upload");

            let mut download = 0.0;
            let mut upload = 0.0;

            if has_upload {
                let upload_str = link.attribute("upload").unwrap();
                upload = self.parse_bandwidth(upload_str);
            }

            if has_download {
                let download_str = link.attribute("download").unwrap();
                download = self.parse_bandwidth(download_str);
            }

            // if has_upload && !has_download{
            //     download = upload;
            // }
            // if !has_upload && has_download{
            //     upload = download;
            // }

            if both_shared {
                let src_meta_bridge = self.create_meta_bridge(graph);
                let dst_meta_bridge = self.create_meta_bridge(graph);

                // create a link between both meta bridges
                graph
                    .insert_link(
                        latency,
                        jitter,
                        drop,
                        upload,
                        src_meta_bridge.clone(),
                        dst_meta_bridge.clone(),
                    )
                    .await;

                if bidirectional {
                    graph
                        .insert_link(
                            latency,
                            jitter,
                            drop,
                            download,
                            dst_meta_bridge.clone(),
                            src_meta_bridge.clone(),
                        )
                        .await;
                }

                graph
                    .insert_link(
                        0.0,
                        0.0,
                        0.0,
                        upload,
                        source.clone(),
                        src_meta_bridge.clone(),
                    )
                    .await;

                if bidirectional {
                    graph
                        .insert_link(
                            0.0,
                            0.0,
                            0.0,
                            download,
                            src_meta_bridge.clone(),
                            source.clone(),
                        )
                        .await;
                }

                graph
                    .insert_link(
                        0.0,
                        0.0,
                        0.0,
                        upload,
                        dst_meta_bridge.clone(),
                        destination.clone(),
                    )
                    .await;

                if bidirectional {
                    graph
                        .insert_link(
                            0.0,
                            0.0,
                            0.0,
                            download,
                            destination.clone(),
                            dst_meta_bridge.clone(),
                        )
                        .await;
                }
            } else if source_node_shared_link {
                let meta_bridge = self.create_meta_bridge(graph);

                graph
                    .insert_link(
                        latency,
                        jitter,
                        drop,
                        upload,
                        meta_bridge.clone(),
                        destination.clone(),
                    )
                    .await;

                if bidirectional {
                    graph
                        .insert_link(
                            latency,
                            jitter,
                            drop,
                            download,
                            destination.clone(),
                            meta_bridge.clone(),
                        )
                        .await;
                }

                graph
                    .insert_link(0.0, 0.0, 0.0, upload, source.clone(), meta_bridge.clone())
                    .await;

                if bidirectional {
                    graph
                        .insert_link(0.0, 0.0, 0.0, download, meta_bridge.clone(), source.clone())
                        .await;
                }
            } else if destination_node_shared_link {
                let meta_bridge = self.create_meta_bridge(graph);
                graph
                    .insert_link(
                        latency,
                        jitter,
                        drop,
                        upload,
                        source.clone(),
                        meta_bridge.clone(),
                    )
                    .await;

                if bidirectional {
                    graph
                        .insert_link(
                            latency,
                            jitter,
                            drop,
                            download,
                            meta_bridge.clone(),
                            source.clone(),
                        )
                        .await;
                }

                graph
                    .insert_link(
                        0.0,
                        0.0,
                        0.0,
                        upload,
                        meta_bridge.clone(),
                        destination.clone(),
                    )
                    .await;

                if bidirectional {
                    graph
                        .insert_link(
                            0.0,
                            0.0,
                            0.0,
                            download,
                            destination.clone(),
                            meta_bridge.clone(),
                        )
                        .await;
                }
            } else {
                graph
                    .insert_link(
                        latency,
                        jitter,
                        drop,
                        upload,
                        source.clone(),
                        destination.clone(),
                    )
                    .await;

                if bidirectional {
                    graph
                        .insert_link(
                            latency,
                            jitter,
                            drop,
                            download,
                            destination.clone(),
                            source.clone(),
                        )
                        .await;
                }
            }
        }
    }

    fn create_meta_bridge(&self, graph: &mut Graph) -> String {
        let random_name = generate(5, charsets::ALPHANUMERIC);

        graph.insert_bridge(random_name.clone(), None);

        random_name
    }

    fn parse_bandwidth(&self, bandwidth: &str) -> f32 {
        let bandwidth_regex = Regex::new(r"([0-9]+)([KMG])bps").unwrap();

        if bandwidth_regex.is_match(&bandwidth) {
            let captures = bandwidth_regex.captures(&bandwidth).unwrap();
            let base: f32 = captures.get(1).unwrap().as_str().parse().unwrap();
            let multiplier = captures.get(2).unwrap().as_str();

            match multiplier {
                "K" => base * 1000.0,
                "M" => base * 1000.0 * 1000.0,
                "G" => base * 1000.0 * 1000.0 * 1000.0,
                _ => 0.0, // not reachable (regex)
            }
        } else {
            panic!("failed to parse bandwidth: {}", bandwidth);
        }
    }

    pub async fn parse_schedule(
        &self,
        initial_graph: Graph,
        config: &Config,
    ) -> (Vec<Graph>, Vec<Event>) {
        let graph_root_handle = initial_graph
            .graph_root
            .clone()
            .expect("graph root should have been set before calling parse_schedule");

        let mut events: Vec<Event> = Vec::new();
        let mut graphs = vec![initial_graph];

        let root = self.document.root().first_child().unwrap();

        let mut dynamic = None;

        for node in root.children() {
            if !node.is_element() {
                continue;
            }
            if node.tag_name().name() == "dynamic" {
                if dynamic.is_some() {
                    panic!("Only one <dynamic> block is allowed.");
                }
                dynamic = Some(node);
            }
        }

        if dynamic.is_none() {
            events.push(Event::new(EventKind::Join, 0.0));
            return (graphs, events);
        }

        let mut first_join = -1.0;
        let mut first_leave = f32::INFINITY;

        let mut rng = Pcg64::seed_from_u64(12345);

        let mut replicas = Vec::new();

        for _ in 0..graph_root_handle.lock().await.replicas {
            let element = vec![false, false, false];

            replicas.push(element);
        }

        let joined = 0;
        let disconnected = 1;
        let used = 2;

        for event in dynamic.unwrap().children() {
            if !event.is_element() {
                continue;
            }

            if event.tag_name().name() != "schedule" {
                panic!(
                    "Only <schedule> is allowed inside <dynamic> {} ",
                    event.tag_name().name()
                );
            }

            let mut time = 0.0;

            if event.has_attribute("time") {
                time = event.attribute("time").unwrap().parse().unwrap();

                if time < 0.0 {
                    panic!("Time attribute must be a positive number.");
                }
            }

            if event.has_attribute("name") && event.has_attribute("action") {
                let node_name = event.attribute("name").unwrap().to_string();

                let bridge_names: Vec<String> = graphs
                    .last()
                    .unwrap()
                    .bridges_by_name
                    .keys()
                    .cloned()
                    .collect();

                if bridge_names.contains(&node_name) {
                    if event.attribute("action").unwrap() == "join" {
                        let (new_graph, new_event) = eventscheduler::schedule_bridge_join(
                            graphs.last().unwrap(),
                            &config.shortest_path_type,
                            time,
                            &node_name,
                        )
                        .await;
                        graphs.push(new_graph);
                        events.push(new_event);
                    }
                    if event.attribute("action").unwrap() == "leave" {
                        let (new_graph, new_event) = eventscheduler::schedule_bridge_leave(
                            graphs.last().unwrap(),
                            &config.shortest_path_type,
                            time,
                            &node_name,
                        )
                        .await;
                        graphs.push(new_graph);
                        events.push(new_event);
                    }
                }

                if node_name != graph_root_handle.lock().await.hostname {
                    continue;
                }

                let mut amount: u32 = 1;

                if event.has_attribute("amount") {
                    amount = event.attribute("amount").unwrap().parse().unwrap();
                }

                let event_type = event.attribute("action").unwrap().to_string();

                if event_type == "join" {
                    for _i in 0..amount {
                        let mut available;

                        let mut id: usize;

                        loop {
                            id = rng.random_range(0..graph_root_handle.lock().await.replicas)
                                as usize;

                            available = !replicas[id][joined];

                            if !graph_root_handle.lock().await.reuse {
                                available = available && !replicas[id][used];
                            }
                            if available {
                                break;
                            }
                        }

                        // mark the state
                        replicas[id][joined] = true;

                        if !graph_root_handle.lock().await.reuse {
                            replicas[id][used] = true;
                        }

                        // if it is us
                        if graph_root_handle.lock().await.replica_id == id {
                            events.push(Event::new(EventKind::Join, time));
                        }

                        if first_join < 0.0 {
                            first_join = time;
                        }
                    }
                }

                if event_type == "leave" || event_type == "crash" {
                    for _i in 0..amount {
                        let mut up;

                        let mut id: usize;

                        loop {
                            id = rng.random_range(0..graph_root_handle.lock().await.replicas)
                                as usize;

                            up = replicas[id][joined];

                            if up {
                                break;
                            }
                        }

                        replicas[id][joined] = false;

                        if graph_root_handle.lock().await.replica_id == id {
                            if event_type == "leave" {
                                // temporary fix before we change all wiki
                                // events.push(Event::new(EventKind::Crash, time));
                                events.push(Event::new(EventKind::Leave, time));
                            }

                            if event_type == "crash" {
                                events.push(Event::new(EventKind::Crash, time));
                            }
                        }

                        if first_leave > time {
                            first_leave = time;
                        }
                    }
                }

                if event_type == "reconnect" {
                    for _i in 0..amount {
                        let mut disconnected_bool;

                        let mut id;

                        loop {
                            id = rng.random_range(0..graph_root_handle.lock().await.replicas)
                                as usize;

                            disconnected_bool = replicas[id][disconnected];

                            if disconnected_bool {
                                break;
                            }
                        }

                        replicas[id][disconnected] = false;
                        if graph_root_handle.lock().await.replica_id == id {
                            events.push(Event::new(EventKind::Reconnect, time));
                        }
                    }
                }

                if event_type == "disconnect" {
                    for _i in 0..amount {
                        let mut connected;

                        let mut id;

                        loop {
                            id = rng.random_range(0..graph_root_handle.lock().await.replicas)
                                as usize;

                            connected = replicas[id][joined] && !(replicas[id][disconnected]);

                            if connected {
                                break;
                            }
                        }

                        replicas[id][disconnected] = true;

                        if graph_root_handle.lock().await.replica_id == id {
                            events.push(Event::new(EventKind::Disconnect, time));
                        }
                    }
                }
            }

            if event.has_attribute("origin")
                && event.has_attribute("dest")
                && event.has_attribute("time")
            {
                let origin = event.attribute("origin").unwrap().to_string();
                let destination = event.attribute("dest").unwrap().to_string();

                if event.has_attribute("action") {
                    let event_type = event.attribute("action").unwrap().to_string();

                    if event_type == "leave" {
                        let (new_graph, event) = eventscheduler::schedule_link_leave(
                            graphs.last().unwrap(),
                            &config.shortest_path_type,
                            time,
                            origin,
                            destination,
                        )
                        .await;
                        graphs.push(new_graph);
                        events.push(event);
                    }

                    if event_type == "join" {
                        let origin = event.attribute("origin").unwrap().to_string();
                        let destination = event.attribute("dest").unwrap().to_string();

                        let (new_graph, new_event, link_existed) =
                            eventscheduler::schedule_link_join(
                                graphs.last().unwrap(),
                                &config.shortest_path_type,
                                time,
                                &origin,
                                &destination,
                            )
                            .await;
                        graphs.push(new_graph);
                        events.push(new_event);

                        if link_existed {
                            continue;
                        } else {
                            let bandwidth = event.attribute("upload").unwrap();

                            let latency: f32 = event.attribute("latency").unwrap().parse().unwrap();

                            let mut drop: f32 = 0.0;

                            if event.has_attribute("drop") {
                                drop = event.attribute("drop").unwrap().parse().unwrap();
                            }

                            let mut jitter: f32 = 0.0;

                            if event.has_attribute("jitter") {
                                jitter = event.attribute("jitter").unwrap().parse().unwrap();
                            }

                            let (new_graph, new_event) = eventscheduler::schedule_new_link(
                                graphs.last().unwrap(),
                                &config.shortest_path_type,
                                time,
                                &origin,
                                &destination,
                                latency,
                                jitter,
                                drop,
                                self.parse_bandwidth(bandwidth),
                            )
                            .await;
                            graphs.push(new_graph);
                            events.push(new_event);

                            if event.has_attribute("download") {
                                let bandwidth = event.attribute("download").unwrap();
                                let (new_graph, new_event) = eventscheduler::schedule_new_link(
                                    graphs.last().unwrap(),
                                    &config.shortest_path_type,
                                    time,
                                    &destination,
                                    &origin,
                                    latency,
                                    jitter,
                                    drop,
                                    self.parse_bandwidth(bandwidth),
                                )
                                .await;
                                graphs.push(new_graph);
                                events.push(new_event);
                            }
                        }
                    }
                } else {
                    let mut bandwidth = -1.0;

                    if event.has_attribute("upload") {
                        bandwidth = self.parse_bandwidth(event.attribute("upload").unwrap());
                    }

                    let mut latency = -1.0;

                    if event.has_attribute("latency") {
                        latency = event.attribute("latency").unwrap().parse().unwrap();
                    }

                    let mut drop = -1.0;

                    if event.has_attribute("drop") {
                        drop = event.attribute("drop").unwrap().parse().unwrap();
                    }

                    let mut jitter = -1.0;
                    if event.has_attribute("jitter") {
                        jitter = event.attribute("jitter").unwrap().parse().unwrap();
                    }

                    if event.has_attribute("download") {
                        let download_bw =
                            self.parse_bandwidth(event.attribute("download").unwrap());
                        let (new_graph, new_event) = eventscheduler::schedule_link_change(
                            graphs.last().unwrap(),
                            &config.shortest_path_type,
                            time,
                            &destination,
                            &origin,
                            latency,
                            jitter,
                            drop,
                            download_bw,
                        )
                        .await;
                        graphs.push(new_graph);
                        events.push(new_event);
                    }
                    let (new_graph, new_event) = eventscheduler::schedule_link_change(
                        graphs.last().unwrap(),
                        &config.shortest_path_type,
                        time,
                        &origin,
                        &destination,
                        latency,
                        jitter,
                        drop,
                        bandwidth,
                    )
                    .await;
                    graphs.push(new_graph);
                    events.push(new_event);
                }
            }
        }
        (graphs, events)
    }
}
