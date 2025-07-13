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

use libloading::{Library, Symbol};
use tokio::sync::mpsc;
use tracing::{error, warn};

/// Sender to the Emulation (TCAL client) receiver loop.
/// Cloning this struct is cheap, since it only encapsulates the `mpsc::Sender`.
#[derive(Clone)]
pub struct Emulation {
    tx: mpsc::Sender<EmulationCmd>,
}

impl Emulation {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel(32);

        tokio::spawn(async move { recv_loop(rx).await });
        Self { tx }
    }

    pub async fn init(&self, ip: u32, port: u32) {
        let _ = self.tx.send(EmulationCmd::Init { ip, port }).await;
    }
    pub async fn enable_path(&self, ip: u32, bandwidth: u32, latency: f32, jitter: f32, drop: f32) {
        let _ = self
            .tx
            .send(EmulationCmd::EnablePath {
                ip,
                bandwidth,
                latency,
                jitter,
                drop,
            })
            .await;
    }
    pub async fn disable_path(&self, ip: u32) {
        let _ = self.tx.send(EmulationCmd::DisablePath { ip }).await;
    }
    pub async fn set_bandwidth(&self, ip: u32, bandwidth: u32) {
        let _ = self
            .tx
            .send(EmulationCmd::SetBandwidth { ip, bandwidth })
            .await;
    }
    pub async fn set_loss(&self, ip: u32, loss: f32) {
        let _ = self.tx.send(EmulationCmd::SetLoss { ip, loss }).await;
    }
    pub async fn set_latency(&self, ip: u32, latency: f32, jitter: f32) {
        let _ = self
            .tx
            .send(EmulationCmd::SetLatency {
                ip,
                latency,
                jitter,
            })
            .await;
    }
    pub async fn disconnect(&self) {
        let _ = self.tx.send(EmulationCmd::Disconnect).await;
    }
    pub async fn reconnect(&self) {
        let _ = self.tx.send(EmulationCmd::Reconnect).await;
    }
    pub async fn teardown(&self) {
        let _ = self.tx.send(EmulationCmd::Teardown).await;
    }
}

/// Accepted command / message types by the receiver loop of `Emulation`.
enum EmulationCmd {
    Init {
        ip: u32,
        port: u32,
    },
    EnablePath {
        ip: u32,
        bandwidth: u32,
        latency: f32,
        jitter: f32,
        drop: f32,
    },
    DisablePath {
        ip: u32,
    },
    SetBandwidth {
        ip: u32,
        bandwidth: u32,
    },
    SetLoss {
        ip: u32,
        loss: f32,
    },
    SetLatency {
        ip: u32,
        latency: f32,
        jitter: f32,
    },
    Disconnect,
    Reconnect,
    Teardown,
}

async fn recv_loop(mut rx: mpsc::Receiver<EmulationCmd>) {
    let tc_lib = unsafe { Library::new("/usr/local/bin/libTCAL.so").unwrap() };

    let init: Symbol<unsafe extern "C" fn(u32, u32, u32)> = unsafe { tc_lib.get(b"init").unwrap() };
    let set_path: Symbol<unsafe extern "C" fn(u32, u32, f32, f32, f32)> =
        unsafe { tc_lib.get(b"initDestination").unwrap() };
    let set_bw: Symbol<unsafe extern "C" fn(u32, u32)> =
        unsafe { tc_lib.get(b"changeBandwidth").unwrap() };
    let set_loss: Symbol<unsafe extern "C" fn(u32, f32)> =
        unsafe { tc_lib.get(b"changeLoss").unwrap() };
    let set_latency: Symbol<unsafe extern "C" fn(u32, f32, f32)> =
        unsafe { tc_lib.get(b"changeLatency").unwrap() };
    let disconnect: Symbol<unsafe extern "C" fn()> = unsafe { tc_lib.get(b"disconnect").unwrap() };
    let reconnect: Symbol<unsafe extern "C" fn()> = unsafe { tc_lib.get(b"reconnect").unwrap() };
    let teardown: Symbol<unsafe extern "C" fn(u32)> = unsafe { tc_lib.get(b"tearDown").unwrap() };

    while let Some(msg) = rx.recv().await {
        match msg {
            EmulationCmd::Init { ip, port } => unsafe {
                init(port, 1000, ip);
            },
            EmulationCmd::EnablePath {
                ip,
                bandwidth,
                latency,
                jitter,
                drop,
            } => {
                if latency == 0.0 {
                    error!(latency, "latency between two nodes can not be 0");
                    std::process::exit(-1);
                }
                unsafe {
                    set_path(ip, bandwidth, latency, jitter, drop);
                }
            }
            EmulationCmd::DisablePath { ip } => unsafe { set_path(ip, 10000, 1.0, 0.0, 1.0) },
            EmulationCmd::SetBandwidth { ip, bandwidth } => {
                let bw_in_kbps = match bandwidth {
                    0 => {
                        warn!(bandwidth, "can not be set to 0, it is set to 1 instead");
                        1
                    }
                    bw => bw / 1000,
                };
                unsafe { set_bw(ip, bw_in_kbps) }
            }
            EmulationCmd::SetLoss { ip, loss } => unsafe { set_loss(ip, loss) },
            EmulationCmd::SetLatency {
                ip,
                latency,
                jitter,
            } => unsafe { set_latency(ip, latency, jitter) },
            EmulationCmd::Disconnect => unsafe { disconnect() },
            EmulationCmd::Reconnect => unsafe { reconnect() },
            EmulationCmd::Teardown => unsafe { teardown(0) },
        }
    }
}
