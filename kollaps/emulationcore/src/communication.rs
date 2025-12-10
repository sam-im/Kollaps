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

use crate::state::State;
use capnp::message::{Builder, HeapAllocator};
use capnp::serialize_packed;
use capnp_schemas::message_capnp;
use libc::O_WRONLY;
use std::ffi::CString;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::BufReader;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use tracing::{info, warn};

const READPIPE_PATH: &str = "/tmp/kollaps/pipes/piperead";
const WRITEPIPE_PATH: &str = "/tmp/kollaps/pipes/pipewrite";

enum CommunicationCmd {
    Init {
        state: Arc<Mutex<State>>,
    },
    SendFlowMsg {
        cycle_number: u32,
        flows: Vec<PathFlowData>,
    },
}

/// Flow data per active path
pub struct PathFlowData {
    pub bandwidth: u32,
    pub links: Vec<u16>,
}

#[derive(Clone)]
pub struct Communication {
    tx: mpsc::Sender<CommunicationCmd>,
}

impl Communication {
    pub fn new(id: String) -> Self {
        let (tx, mut rx) = mpsc::channel(16);
        // Waits for an `Init` command before creating the read/write pipes and accepting commands
        tokio::spawn(async move {
            if let Some(msg) = rx.recv().await {
                match msg {
                    CommunicationCmd::Init { state } => {
                        let (readpipe, writepipe) = create_pipes(&id);
                        tokio::spawn(async move { recv_cmd_loop(writepipe, rx).await });
                        tokio::spawn(async move { recv_msg_loop(readpipe, state).await });
                        info!("EC {}: Communication initialized", id);
                    }
                    _ => {
                        panic!(
                            "Communication is not initialized, expected an `Init` command. Did you run `init` first?"
                        );
                    }
                }
            }
        });

        Self { tx }
    }

    /// Creates the read/write pipes and starts command and message receiver tasks.
    pub async fn init(&self, state: Arc<Mutex<State>>) {
        let _ = self.tx.send(CommunicationCmd::Init { state }).await;
    }

    /// Writes flow information of the current paths to the write pipe.
    pub async fn send_flows(&self, cycle_number: u32, flows: Vec<PathFlowData>) {
        let _ = self
            .tx
            .send(CommunicationCmd::SendFlowMsg {
                cycle_number,
                flows,
            })
            .await;
    }
}

async fn recv_cmd_loop(writepipe: File, mut rx: mpsc::Receiver<CommunicationCmd>) {
    while let Some(cmd) = rx.recv().await {
        match cmd {
            CommunicationCmd::Init { .. } => warn!("Communication is already initialized"),
            CommunicationCmd::SendFlowMsg {
                cycle_number,
                flows,
            } => {
                let mut message: Builder<HeapAllocator> = Builder::new_default();
                let mut msg: message_capnp::message::Builder =
                    message.init_root::<message_capnp::message::Builder>();

                msg.set_round(cycle_number);
                msg.reborrow().init_flows(flows.len() as u32);

                let mut msg_flows = msg.get_flows().unwrap();

                flows.iter().enumerate().for_each(|(i, flow)| {
                    let mut msg_flow = msg_flows.reborrow().get(i as u32);
                    msg_flow.set_bw(flow.bandwidth);

                    let links_len = flow.links.len() as u32;
                    if !(links_len > 0 && links_len < 254) {
                        warn!(links_len, "EC: links should be between 0 and 254");
                    }
                    let mut links = msg_flow.init_links(links_len);

                    flow.links.iter().enumerate().for_each(|(i, link)| {
                        links.reborrow().get(i as u32).set_id(*link);
                    });
                });
                serialize_packed::write_message(&writepipe, &message).unwrap();
            }
        }
    }
}
async fn recv_msg_loop(readpipe: File, state: Arc<Mutex<State>>) {
    std::thread::spawn(move || {
        let mut buf_reader = BufReader::new(readpipe);
        loop {
            let message_reader = serialize_packed::read_message(
                &mut buf_reader,
                capnp::message::ReaderOptions::new(),
            )
            .unwrap();
            let message: message_capnp::message::Reader<'_> =
                match message_reader.get_root::<message_capnp::message::Reader>() {
                    Ok(msg) => msg,
                    Err(e) => {
                        warn!("error while parsing message: {}", e);
                        continue;
                    }
                };
            let mut flows_data = Vec::new();
            let flows = message.get_flows().unwrap();
            flows.iter().for_each(|f| {
                let bw = f.get_bw() as f32;
                let links = f.get_links().unwrap();
                let links_len = links.len() as u16;
                let ids: Vec<u16> = links.into_iter().map(|l| l.get_id()).collect();
                flows_data.push((bw, links_len, ids));
            });
            // TODO allow batch updates in `collect_flow_u16` to reduce locking
            for (bw, len, ids) in flows_data {
                state
                    .blocking_lock()
                    .get_current_graph()
                    .blocking_lock()
                    .collect_flow_u16(bw, len, ids);
            }
        }
    });
}

fn create_pipes(id: &str) -> (File, File) {
    let pathread = format!("{}{}", READPIPE_PATH, id);

    let filename = CString::new(pathread.clone()).unwrap();
    unsafe {
        libc::mkfifo(filename.as_ptr(), O_WRONLY as u32);
    }

    let pathwrite = format!("{}{}", WRITEPIPE_PATH, id);

    let filename = CString::new(pathwrite.clone()).unwrap();
    unsafe {
        libc::mkfifo(filename.as_ptr(), O_WRONLY as u32);
    }

    let readpipe = OpenOptions::new()
        .read(true)
        .open(pathread.clone())
        .expect("file not found");

    let writepipe = OpenOptions::new()
        .write(true)
        .open(pathwrite.clone())
        .expect("file not found");

    (readpipe, writepipe)
}
