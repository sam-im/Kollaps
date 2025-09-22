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
use crate::graph::Graph;
use crate::orchestrator::{Orchestrator, SignalCode};
use crate::state::State;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{Duration, Instant, sleep_until};
use tracing::{error, info};

/// Represents a dynamic event as parsed from a topology file.
pub struct Event {
    kind: EventKind,
    time: f32,
}

impl Event {
    /// Creates a new `Event`.
    ///
    /// The `time` parameter specifies how many seconds after the start of
    /// the experiment this event will be triggered by the `EventScheduler`.
    pub fn new(kind: EventKind, time: f32) -> Self {
        Self { kind, time }
    }
}

/// Describes the action that will be taken by the `EventScheduler` at some
/// point in the experiment.
pub enum EventKind {
    /// Increment the age of `State`, i.e. switch to the next pre-computed Graph.
    NextGraph,
    Join,
    Leave,
    Crash,
    Disconnect,
    Reconnect,
}

pub struct EventScheduler {
    pub events: Vec<Event>,
    pub state: Arc<Mutex<State>>,
    pub pid: u32,
    orchestrator: Option<Orchestrator>,
    pub script: String,
    pub name: String,
}

impl EventScheduler {
    pub fn new(state: Arc<Mutex<State>>, orchestrator: Option<Orchestrator>, pid: u32) -> Self {
        EventScheduler {
            events: vec![],
            name: "".to_string(),
            state,
            pid,
            orchestrator,
            script: String::new(),
        }
    }

    pub fn sort_events(&mut self) {
        self.events
            .sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap());
    }

    pub async fn start(&mut self) {
        assert!(
            self.pid != 0,
            "eventscheduler.pid must be set before calling start"
        );

        info!("EC {}: event scheduler started", self.name);
        self.sort_events();

        let start_time = Instant::now();
        let mut current_event_index = 0;

        loop {
            if self.events.len() <= current_event_index {
                break;
            }
            let event_time = Duration::from_secs_f32(self.events[current_event_index].time);
            sleep_until(start_time + event_time).await;

            match self.events[current_event_index].kind {
                EventKind::NextGraph => {
                    self.state.lock().await.increment_age().await;
                }
                EventKind::Join => {
                    let id = self.state.lock().await.id.clone();
                    match &self.orchestrator {
                        Some(o) => {
                            o.start_experiment(&id).await;
                            info!("EC {}: started experiment", self.name);
                        }
                        // `None` currently means baremetal deployment
                        None => {
                            if self.script.is_empty() {
                                error!("EC {}: script is empty", self.name);
                            } else {
                                info!(
                                    "EC {}: started experiment with script {}",
                                    self.name,
                                    self.script.clone()
                                );
                                start_script(self.script.clone());
                            }
                        }
                    }
                }
                EventKind::Leave => {
                    if let Some(o) = &self.orchestrator {
                        o.stop_experiment(self.pid, SignalCode::SigInt).await
                    }
                }
                EventKind::Crash => {
                    if let Some(o) = &self.orchestrator {
                        o.stop_experiment(self.pid, SignalCode::SigKill).await
                    }
                }
                EventKind::Disconnect => {
                    self.state.lock().await.tcal_client.disconnect().await;
                }
                EventKind::Reconnect => {
                    self.state.lock().await.tcal_client.reconnect().await;
                }
            }
            current_event_index += 1;
        }
        info!("EC {}: all events are concluded", self.name);
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
    let event = Event::new(EventKind::NextGraph, time);

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
    let event = Event::new(EventKind::NextGraph, time);

    (new_graph, event, link_existed)
}

// TODO: consider moving to xmlgraphparser
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
    let event = Event::new(EventKind::NextGraph, time);

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
    let event = Event::new(EventKind::NextGraph, time);

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
    let event = Event::new(EventKind::NextGraph, time);

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
    let event = Event::new(EventKind::NextGraph, time);

    (new_graph, event)
}
