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
use crate::state::State;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use tokio::sync::Mutex;
use tracing::info;

pub struct EventScheduler {
    pub events: Vec<Event>,
    pub state: Arc<Mutex<State>>,
    pub timebetweenevents: Vec<f32>,
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

    pub async fn schedule_bridge_join(&mut self, time: f32, bridge_name: String) {
        {
            let state_handle = self.state.clone();
            let mut state = state_handle.lock().await;

            state.insert_graph().await;

            let most_recent_graph_handle = state.get_graph_most_recent();
            let mut most_recent_graph = most_recent_graph_handle.lock().await;

            let bridge = most_recent_graph
                .removed_bridges
                .remove(&bridge_name)
                .unwrap();
            most_recent_graph
                .bridges_by_name
                .insert(bridge_name.clone(), bridge);
        }

        self.recompute_and_store().await;

        let event = Event::new(1, time);

        self.events.push(event);
    }

    pub async fn schedule_bridge_leave(&mut self, time: f32, bridge_name: String) {
        {
            let state_handle = self.state.clone();
            let mut state = state_handle.lock().await;

            state.insert_graph().await;

            let most_recent_graph_handle = state.get_graph_most_recent();
            let mut most_recent_graph = most_recent_graph_handle.lock().await;

            let bridges = most_recent_graph
                .bridges_by_name
                .remove(&bridge_name)
                .unwrap();
            most_recent_graph
                .removed_bridges
                .insert(bridge_name.clone(), bridges);
        }

        self.recompute_and_store().await;

        let event = Event::new(1, time);

        self.events.push(event);
    }

    pub async fn schedule_link_leave(&mut self, time: f32, origin: String, destination: String) {
        {
            let state_handle = self.state.clone();
            let mut state = state_handle.lock().await;

            state.insert_graph().await;

            let most_recent_graph_handle = state.get_graph_most_recent();
            let mut most_recent_graph = most_recent_graph_handle.lock().await;

            let links = most_recent_graph.links.clone();

            //get id of links with origin and destination
            let mut ids = vec![];
            for (id, link_handle) in links.iter() {
                let link = link_handle.lock().await;

                let source = link.source.lock().await.hostname.clone();
                let dest = link.destination.lock().await.hostname.clone();

                if source == origin && dest == destination {
                    ids.push(id);
                    most_recent_graph.removed_links.push(link_handle.clone());
                }
            }

            //remove them from the services and bridges
            for id in ids {
                most_recent_graph.links.remove(id);

                let services = most_recent_graph.services.clone();

                for (_ip, service) in services.iter() {
                    service.lock().await.remove_link(*id);
                }

                let bridges = most_recent_graph.bridges.clone();

                for (_ip, bridge) in bridges.iter() {
                    bridge.lock().await.remove_link(*id);
                }
            }
        }

        let event = Event::new(1, time);

        self.events.push(event);

        self.recompute_and_store().await;
    }

    pub async fn schedule_link_join(
        &mut self,
        time: f32,
        origin: String,
        destination: String,
    ) -> bool {
        self.state.lock().await.insert_graph().await;

        let mut joining_links = vec![];

        let removed_links = self
            .state
            .lock()
            .await
            .get_graph_most_recent()
            .lock()
            .await
            .removed_links
            .clone();

        let mut link_existed = false;

        //remove from removed_links and get joining links
        for (i, link) in removed_links.iter().enumerate() {
            let source = link.lock().await.source.lock().await.hostname.clone();

            let dest = link.lock().await.destination.lock().await.hostname.clone();

            if source == origin && dest == destination {
                joining_links.push(link.clone());
                self.state
                    .lock()
                    .await
                    .get_graph_most_recent()
                    .lock()
                    .await
                    .removed_links
                    .remove(i);
                self.state
                    .lock()
                    .await
                    .get_graph_most_recent()
                    .lock()
                    .await
                    .links
                    .insert(link.lock().await.id, link.clone());
            }
        }

        //add them to services and bridges
        for link in joining_links {
            link_existed = true;

            let services = self
                .state
                .lock()
                .await
                .get_graph_most_recent()
                .lock()
                .await
                .services_by_name
                .clone();
            let source = link.lock().await.source.lock().await.hostname.clone();
            for (name, services) in services.iter() {
                if source == *name {
                    for service in services {
                        service.lock().await.attach_link(link.lock().await.id);
                    }
                }
            }
            let bridges = self
                .state
                .lock()
                .await
                .get_graph_most_recent()
                .lock()
                .await
                .bridges_by_name
                .clone();

            for (name, bridges) in bridges.iter() {
                if source == *name {
                    for bridge in bridges {
                        bridge.lock().await.attach_link(link.lock().await.id);
                    }
                }
            }
        }

        let event = Event::new(1, time);

        self.events.push(event);

        self.recompute_and_store().await;

        return link_existed;
    }

    pub async fn schedule_new_link(
        &mut self,
        time: f32,
        origin: String,
        destination: String,
        latency: f32,
        jitter: f32,
        drop: f32,
        bandwidth: f32,
    ) {
        self.state.lock().await.insert_graph().await;

        self.state
            .lock()
            .await
            .get_graph_most_recent()
            .lock()
            .await
            .insert_link(latency, jitter, drop, bandwidth, origin, destination).await;

        let event = Event::new(1, time);

        self.events.push(event);

        self.recompute_and_store().await;
    }

    pub async fn schedule_link_change(
        &mut self,
        time: f32,
        origin: String,
        destination: String,
        latency: f32,
        jitter: f32,
        drop: f32,
        bandwidth: f32,
    ) {
        self.state.lock().await.insert_graph().await;

        for (_id, link) in self
            .state
            .lock()
            .await
            .get_graph_most_recent()
            .lock()
            .await
            .links
            .iter_mut()
        {
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

                //print_message(self.name.clone(),format!("origin is {} and dest is {} and new latency is {}",origin,destination,latency).to_string());
            }
        }

        let event = Event::new(1, time);

        self.events.push(event);

        self.recompute_and_store().await;
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
                        info!("EC {}: started my script", self.name);
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
