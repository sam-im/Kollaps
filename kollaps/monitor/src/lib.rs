//! This library contains the userspace part for [https://docs.rs/aya/latest/aya/](aya).
//!
//! # Usage
//!
//! Call the `run` function to receive an `EbpfHandle` which contains
//! a `tokio::sync::mpsc::Receiver<Message>` and the Ebpf struct from `aya`.
//!
//! Use the receiver to receive `Message`s from PerfEvents.
//! To receive PerfEvents, spawn a new task that polls the receiver.
//!
//! ## Example
//! ```
//! TODO
//! ```
//!
//! Note that dropping this struct will cause `aya` to unload the eBPF program and
//! it's maps from the kernel, ultimately performing the necessary clean-up.

mod error;

use error::MonitorError;
use monitor_common::Message;

use aya::maps::AsyncPerfEventArray;
use aya::programs::{Xdp, XdpFlags};
use aya::util::online_cpus;
use aya::Ebpf;
use bytes::BytesMut;
use std::mem::size_of;
use std::ptr;
use tokio::sync::mpsc;
use tokio::task;

pub struct EbpfHandle {
    pub ebpf: Ebpf,
    pub rx: tokio::sync::mpsc::Receiver<Message>,
}

pub async fn run(iface: &str) -> Result<EbpfHandle, MonitorError> {
    // Include monitor-ebpf's eBPF object file
    // as raw bytes at compile-time and load it at runtime
    let mut ebpf = aya::Ebpf::load(aya::include_bytes_aligned!(concat!(
        env!("OUT_DIR"),
        "/monitor"
    )))?;

    let program: &mut Xdp = ebpf
        .program_mut("monitor")
        .ok_or(MonitorError::AyaNone)?
        .try_into()?;

    program.load()?;
    program.attach(iface, XdpFlags::default())?;

    // try to convert PerfEventArray to it's Async equivalent
    let mut perf_array =
        AsyncPerfEventArray::try_from(ebpf.take_map("PERF_EVENTS").ok_or(MonitorError::AyaNone)?)?;

    let (tx, rx) = mpsc::channel::<Message>(512);

    // aya will use a different PerfEventArray for each CPU
    // so we open a seperate buffer, task, etc. for each
    for cpu_id in online_cpus().map_err(|_| MonitorError::CpuId)? {
        let tx = tx.clone();
        let mut buf = perf_array.open(cpu_id, None)?;

        task::spawn(async move {
            let mut buffers = (0..64)
                .map(|_| BytesMut::with_capacity(size_of::<Message>()))
                .collect::<Vec<_>>();

            loop {
                match buf.read_events(&mut buffers).await {
                    Ok(events) => {
                        // events.read contains the number of events that have been read,
                        // and is always <= buffers.len()
                        for i in 0..events.read {
                            let event_buf = &mut buffers[i];

                            // each event in the PERF_EVENTS has a Message struct
                            // accompanied with a header of length 4
                            if event_buf.len() != size_of::<Message>() + 4 {
                                eprintln!(
                                    "monitor: unexpected perf event size {} on CPU {} – skipping",
                                    event_buf.len(),
                                    cpu_id
                                );
                                event_buf.clear();
                                continue;
                            }

                            let ptr = event_buf.as_ptr() as *const Message;
                            let msg = unsafe { ptr::read_unaligned(ptr) };

                            // println!("monitor: received event {:?}", msg);
                            match tx.send(msg).await {
                                Ok(_) => {
                                    event_buf.clear();
                                    continue;
                                },
                                Err(_) => break,  // only fails if rx is dropped
                            }
                        }
                    },
                    Err(e) => {
                        eprintln!("monitor: error reading perf buffer on CPU {}: {:?}", cpu_id, e);
                        break;
                    }
                }
            }
            // unreachable but necessary for the
            // compiler to allow the usage of '?'
            #[allow(unreachable_code)]
            Ok::<_, MonitorError>(())
        });
    }

    Ok(EbpfHandle { ebpf, rx })
}
