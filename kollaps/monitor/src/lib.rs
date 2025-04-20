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

use aya::Ebpf;
use error::MonitorError;

use aya::maps::AsyncPerfEventArray;
use aya::programs::{Xdp, XdpFlags};
use aya::util::online_cpus;
use bytes::BytesMut;
use monitor_common::Message;
use std::mem::size_of;
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
        AsyncPerfEventArray::try_from(ebpf.take_map("PERF_EVENT").ok_or(MonitorError::AyaNone)?)?;

    let (tx, rx) = mpsc::channel::<Message>(512);

    // aya will use a different PerfEventArray for each CPU
    // so we open a seperate buffer, task, etc. for each
    for cpu_id in online_cpus().map_err(|_| MonitorError::CpuId)? {
        let tx = tx.clone();
        let mut buf = perf_array.open(cpu_id, None)?;

        task::spawn(async move {
            let mut buffers = (0..10)
                .map(|_| BytesMut::with_capacity(size_of::<Message>()))
                .collect::<Vec<_>>();

            loop {
                // wait for events
                let events = buf.read_events(&mut buffers).await?;

                // events.read contains the number of events that have been read,
                // and is always <= buffers.len()
                for i in 0..events.read {
                    let buf = &mut buffers[i];
                    let ptr = buf.as_ptr() as *const Message;
                    let msg;
                    // SAFETY:
                    // - `buf` must be properly aligned for `Message`, which is ensured
                    // because it is #[repr(C)] and contains two `u32`s.
                    // - `buf` is a `BytesMut` with the size of `Message` and
                    // should be properly initialized (check `monitor-ebps`).
                    unsafe {
                        msg = std::ptr::read(ptr);
                        tx.send(msg).await?;
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

