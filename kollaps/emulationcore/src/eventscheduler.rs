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

use crate::aux::start_script;
use crate::docker::start_experiment;
use crate::docker::stop_experiment;
use crate::graph::Graph;
use crate::state::State;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use tokio::sync::Mutex;
use tracing::info;

pub struct EventScheduler {
    pub events: Vec<Event>,
    pub state: Arc<Mutex<State>>,
    pub timebetweenevents: Vec<f32>, // TODO remove
    pub pid: u32,
    orchestrator: String,
    pub script: String,
    pub name: String,
    pub shortest_path_type: String,
}

impl EventScheduler {
    pub fn new(state: Arc<Mutex<State>>, orchestrator: String) -> EventScheduler {
        EventScheduler {
            events: vec![],
            name: "".to_string(),
            state: state,
            timebetweenevents: vec![],
            pid: 0,
            orchestrator,
            script: "".to_string(),
            shortest_path_type: "hop".to_string(),
        }
    }

    pub fn schedule_join(&mut self, time: f32) {
        let event = Event::new(0, time);

        self.events.push(event);
    }

    pub fn schedule_leave(&mut self, time: f32) {
        let event = Event::new(2, time);
        self.events.push(event);
    }

    pub fn schedule_crash(&mut self, time: f32) {
        let event = Event::new(3, time);

        self.events.push(event);
    }

    pub fn schedule_disconnect(&mut self, time: f32) {
        let event = Event::new(4, time);

        self.events.push(event);
    }

    pub fn schedule_reconnect(&mut self, time: f32) {
        let event = Event::new(5, time);

        self.events.push(event);
    }

    pub async fn recompute_and_store(&mut self) {
        if self.shortest_path_type.eq("hop") {
            self.state
                .lock()
                .await
                .get_graph_most_recent()
                .lock()
                .await
                .calculate_shortest_paths()
                .await;
        }
        if self.shortest_path_type.eq("latency") {
            self.state
                .lock()
                .await
                .get_graph_most_recent()
                .lock()
                .await
                .calculate_shortest_paths_latency()
                .await;
        }

        self.state
            .lock()
            .await
            .get_graph_most_recent()
            .lock()
            .await
            .calculate_properties()
            .await;
    }

    // pub fn print_events(&mut self){
    //     for event in self.events.iter(){
    //         println!("id {} and time {}", event.id,event.time);
    //     }
    // }

    pub fn sort_events(&mut self) {
        self.events
            .sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap());
    }

    pub async fn start(&mut self) {
        info!("EC {}: started experiment", self.name);
        let now = Instant::now();
        let mut count = 0;

        loop {
            // TODO use tokio's sleep_until
            if now.elapsed().as_millis() >= (self.events[count].time * 1000.0) as u128 {
                if self.events[count].id == 0 {
                    let id = self.state.lock().await.id.clone();

                    if self.orchestrator == "baremetal" {
                        if self.script != "" {
                            info!(
                                "EC {}: started script with name {}",
                                self.name,
                                self.script.clone()
                            );
                            start_script(self.script.clone());
                        }
                    } else {
                        tokio::spawn(async move { start_experiment(id).await });
                        info!("EC {}: started experiment", self.name);
                    }
                    //task::spawn(start_experiment(self.state.lock().unwrap().id.clone()));
                }

                if self.events[count].id == 3 {
                    stop_experiment(self.pid.clone(), 3);
                }
                if self.events[count].id == 2 {
                    stop_experiment(self.pid, 2);
                }
                if self.events[count].id == 1 {
                    self.state.lock().await.increment_age().await;
                }

                if self.events[count].id == 4 {
                    self.state.lock().await.tcal_client.disconnect().await;
                }

                if self.events[count].id == 5 {
                    self.state.lock().await.tcal_client.reconnect().await;
                }
                if count == self.events.len() - 1 {
                    info!("EC {}: all events concluded", self.name);
                    break;
                }
                count = count + 1;
            }
            // TODO also remove this if you use sleep_until above
            tokio::time::sleep(Duration::from_secs_f32(0.5)).await;
        }
    }
}

pub async fn schedule_link_leave(
    graph: &Graph,
    shortest_path_type: &str,
    time: f32,
    origin: String,
    destination: String,
) -> (Graph, Event) {
    let mut new_graph = Graph::new();
    new_graph.create_from_graph(graph).await;

    let mut ids = vec![];
    let links = new_graph.links.clone();
    for (id, link_handle) in links {
        let link = link_handle.lock().await;

        let src = link.source.lock().await.hostname.clone();
        let dst = link.destination.lock().await.hostname.clone();

        if src == origin && dst == destination {
            ids.push(id);
            new_graph.removed_links.push(link_handle.clone());
        }
    }

    for id in ids {
        new_graph.links.remove(&id);

        for (_, service) in new_graph.services.iter() {
            service.lock().await.remove_link(id);
        }

        for (_, bridge) in new_graph.bridges.iter() {
            bridge.lock().await.remove_link(id);
        }
    }
    new_graph.recompute_properties(shortest_path_type).await;
    let event = Event::new(1, time);

    (new_graph, event)
}

pub async fn schedule_link_join(
    graph: &Graph,
    shortest_path_type: &str,
    time: f32,
    origin: &str,
    destination: &str,
) -> (Graph, Event, bool) {
    let mut new_graph = Graph::new();
    new_graph.create_from_graph(graph).await;

    let mut joining_links = vec![];
    let mut link_existed = false;
    let removed_links = new_graph.removed_links.clone();

    // remove from removed_links and get joining links
    for (i, link) in removed_links.iter().enumerate() {
        let src = link.lock().await.source.lock().await.hostname.clone();
        let dst = link.lock().await.destination.lock().await.hostname.clone();

        if src == origin && dst == destination {
            joining_links.push(link.clone());
            new_graph.removed_links.remove(i);
            new_graph.links.insert(link.lock().await.id, link.clone());
        }
    }

    // add them to services and bridges
    for link in joining_links {
        link_existed = true;

        let source = link.lock().await.source.lock().await.hostname.clone();

        let services = new_graph.services_by_name.clone();
        for (name, services) in services.iter() {
            if source == *name {
                for service in services {
                    service.lock().await.attach_link(link.lock().await.id);
                }
            }
        }

        let bridges = new_graph.bridges_by_name.clone();
        for (name, bridges) in bridges.iter() {
            if source == *name {
                for bridge in bridges {
                    bridge.lock().await.attach_link(link.lock().await.id);
                }
            }
        }
    }
    new_graph.recompute_properties(shortest_path_type).await;
    let event = Event::new(1, time);

    (new_graph, event, link_existed)
}

pub async fn schedule_new_link(
    graph: &Graph,
    shortest_path_type: &str,
    time: f32,
    origin: &str,
    destination: &str,
    latency: f32,
    jitter: f32,
    drop: f32,
    bandwidth: f32,
) -> (Graph, Event) {
    let mut new_graph = Graph::new();
    new_graph.create_from_graph(graph).await;

    new_graph
        .insert_link(
            latency,
            jitter,
            drop,
            bandwidth,
            origin.to_string(),
            destination.to_string(),
        )
        .await;
    new_graph.recompute_properties(shortest_path_type).await;
    let event = Event::new(1, time);

    (new_graph, event)
}

pub async fn schedule_link_change(
    graph: &Graph,
    shortest_path_type: &str,
    time: f32,
    origin: &str,
    destination: &str,
    latency: f32,
    jitter: f32,
    drop: f32,
    bandwidth: f32,
) -> (Graph, Event) {
    let mut new_graph = Graph::new();
    new_graph.create_from_graph(graph).await;

    for (_, link) in new_graph.links.iter_mut() {
        let link_origin = link.lock().await.source.lock().await.hostname.clone();
        let link_dest = link.lock().await.destination.lock().await.hostname.clone();

        if origin == link_origin && link_dest == destination {
            if bandwidth >= 0.0 {
                link.lock().await.bandwidth = bandwidth;
            }
            if latency >= 0.0 {
                link.lock().await.latency = latency;
            }
            if jitter >= 0.0 {
                link.lock().await.jitter = jitter;
            }
            if drop >= 0.0 {
                link.lock().await.drop = drop;
            }
        }
    }
    new_graph.recompute_properties(shortest_path_type).await;
    let event = Event::new(1, time);

    (new_graph, event)
}

pub async fn schedule_bridge_join(
    graph: &Graph,
    shortest_path_type: &str,
    time: f32,
    bridge_name: &str,
) -> (Graph, Event) {
    let mut new_graph = Graph::new();
    new_graph.create_from_graph(graph).await;

    let bridge = new_graph.removed_bridges.remove(bridge_name).unwrap();
    new_graph
        .bridges_by_name
        .insert(bridge_name.to_string(), bridge);

    new_graph.recompute_properties(shortest_path_type).await;
    let event = Event::new(1, time);

    (new_graph, event)
}

pub async fn schedule_bridge_leave(
    graph: &Graph,
    shortest_path_type: &str,
    time: f32,
    bridge_name: &str,
) -> (Graph, Event) {
    let mut new_graph = Graph::new();
    new_graph.create_from_graph(graph).await;

    let bridge = new_graph.bridges_by_name.remove(bridge_name).unwrap();
    new_graph
        .removed_bridges
        .insert(bridge_name.to_string(), bridge);

    new_graph.recompute_properties(shortest_path_type).await;
    let event = Event::new(1, time);

    (new_graph, event)
}

// TODO use an enum, e.g. EventType, instead of event ids
pub struct Event {
    pub id: u32,
    pub time: f32,
}

impl Event {
    pub fn new(id: u32, time: f32) -> Event {
        Event { id: id, time: time }
    }
}
