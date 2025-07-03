//! This library contains the userspace part of the monitor* crates.
//!
//! # Usage
//! Call the `run` function to receive an `EbpfHandle` struct which
//! contains a `tokio::sync::mpsc::Receiver<Message>`.
//!
//! Poll the receiver to receive a `Message` that has the sum of
//! current bytes sent to some destination address.
//!
//! ## Example
//! ```
//! let mut monitor_handle = monitor::run("eth0").await.unwrap();
//! while let Some(msg) = monitor_handle.rx.recv().await {
//!     // do something with `msg`
//! }
//! ```

mod error;

use error::MonitorError;
use monitor_common::Message;

use aya::maps::AsyncPerfEventArray;
use aya::programs::SocketFilter;
use aya::util::online_cpus;
use aya::Ebpf;
use bytes::BytesMut;
use std::mem::{self, size_of};
use std::os::fd::{FromRawFd, OwnedFd};
use std::ptr;
use tokio::sync::mpsc;
use tokio::task;
use tracing::{error, info, warn};

/// Struct holding the receiver. Poll `rx` to receive `Message`s from the kernel.
/// Dropping this struct will perform the necessary clean-up, i.e.
/// unloading the `socket_filter` program and closing it's socket.
pub struct EbpfHandle {
    _socket: OwnedFd,
    _ebpf: Ebpf,
    pub rx: tokio::sync::mpsc::Receiver<Message>,
}

// For logging purposes only.
impl Drop for EbpfHandle {
    fn drop(&mut self) {
        info!("EbpfHandle dropped, unloading eBPF and closing socket.");
    }
}

pub async fn run(iface: &str) -> Result<EbpfHandle, MonitorError> {
    // Included by aya-rs project template for compatibility with older kernels.
    bump_memlock_rlimit();

    // Include monitor-ebpf's eBPF object file
    // as raw bytes at compile-time and load it at runtime
    let mut _ebpf = aya::Ebpf::load(aya::include_bytes_aligned!(concat!(
        env!("OUT_DIR"),
        "/monitor"
    )))?;

    let program: &mut SocketFilter = _ebpf
        .program_mut("monitor")
        .ok_or(MonitorError::AyaNone)?
        .try_into()?;

    // Create a raw socket bound to an interface to be used
    // by the socket_filter program.
    let _socket = create_raw_socket(iface)?;
    program.load()?;
    program.attach(&_socket)?;

    // Try to convert PerfEventArray to it's async equivalent
    let mut perf_array =
        AsyncPerfEventArray::try_from(_ebpf.take_map("PERF_EVENTS").ok_or(MonitorError::AyaNone)?)?;

    let (tx, rx) = mpsc::channel::<Message>(256);

    // aya-rs will use a different PerfEventArray for each CPU
    // so open a seperate buffer and task for each
    for cpu_id in online_cpus().map_err(|_| MonitorError::CpuId)? {
        let tx = tx.clone();
        let mut perf_buffer = perf_array.open(cpu_id, None)?;

        task::spawn(async move {
            let mut buffer = (0..64)
                .map(|_| BytesMut::with_capacity(size_of::<Message>()))
                .collect::<Vec<_>>();

            loop {
                match perf_buffer.read_events(&mut buffer).await {
                    Ok(events) => {
                        if events.lost > 0 {
                            warn!(
                                events.lost,
                                "Events are being generated faster than they are consumed."
                            );
                        }
                        // events.read contains the number of events that have been read,
                        // and is always <= buffers.len()
                        for i in 0..events.read {
                            let event_buf = &mut buffer[i];

                            // each event in the PERF_EVENTS has a Message struct
                            // accompanied with a header of length 4
                            if event_buf.len() != size_of::<Message>() + 4 {
                                warn!(
                                    "monitor: unexpected perf event size {} on CPU {} – skipping",
                                    event_buf.len(),
                                    cpu_id
                                );
                                event_buf.clear();
                                continue;
                            }

                            let ptr = event_buf.as_ptr() as *const Message;
                            let msg = unsafe { ptr::read_unaligned(ptr) };

                            match tx.send(msg).await {
                                Ok(_) => {
                                    event_buf.clear();
                                    continue;
                                }
                                Err(_) => break, // only fails if rx is dropped
                            }
                        }
                    }
                    Err(e) => {
                        error!("error while reading perf array on CPU {}: {:?}", cpu_id, e);
                        break;
                    }
                }
            }
            // unreachable but necessary for the compiler to allow the usage of '?'
            #[allow(unreachable_code)]
            Ok::<_, MonitorError>(())
        });
    }

    Ok(EbpfHandle { rx, _ebpf, _socket })
}

/// Creates and return the file descriptor for the raw socket bound to the network interface `iface`.
/// Dropping the returned `OwnedFd` struct will close the socket.
fn create_raw_socket(iface: &str) -> Result<OwnedFd, MonitorError> {
    use libc::{
        bind, c_int, htons, if_nametoindex, sockaddr, sockaddr_ll, socket, AF_PACKET, ETH_P_ALL,
        SOCK_RAW,
    };
    use std::ffi::CString;

    let sock_fd = unsafe { socket(AF_PACKET, SOCK_RAW, htons(ETH_P_ALL as u16) as c_int) };
    if sock_fd < 0 {
        return Err(MonitorError::RawSocket);
    }

    let if_name = CString::new(iface).ok().ok_or(MonitorError::RawSocket)?;
    let if_index = unsafe { if_nametoindex(if_name.as_ptr()) };

    if if_index == 0 {
        return Err(MonitorError::RawSocket);
    }

    let mut sll: sockaddr_ll = unsafe { mem::zeroed() };
    sll.sll_family = AF_PACKET as u16;
    sll.sll_ifindex = if_index as i32;
    sll.sll_protocol = htons(ETH_P_ALL as u16);

    let res = unsafe {
        bind(
            sock_fd,
            &sll as *const sockaddr_ll as *const sockaddr,
            mem::size_of::<sockaddr_ll>() as u32,
        )
    };

    if res < 0 {
        return Err(MonitorError::RawSocket);
    }

    Ok(unsafe { OwnedFd::from_raw_fd(sock_fd) })
}

/// Bump the memlock rlimit. This is needed for older kernels that don't use the
/// new memcg based accounting, see https://lwn.net/Articles/837122/
/// Included by aya-rs project template.
fn bump_memlock_rlimit() {
    let rlim = libc::rlimit {
        rlim_cur: libc::RLIM_INFINITY,
        rlim_max: libc::RLIM_INFINITY,
    };
    let ret = unsafe { libc::setrlimit(libc::RLIMIT_MEMLOCK, &rlim) };
    if ret != 0 {
        warn!(
            "remove limit on locked memory failed, return value is: {}",
            ret
        );
    }
}
