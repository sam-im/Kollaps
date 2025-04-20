use monitor_common::Message;

use aya::maps::perf::PerfBufferError;
use aya::maps::MapError as AyaMapErr;
use aya::programs::ProgramError as AyaProgramErr;
use aya::EbpfError as AyaEbpfErr;
use thiserror::Error;
use tokio::sync::mpsc::error::SendError;

#[derive(Error, Debug)]
pub enum MonitorError {
    #[error("failed to retrieve CPU IDs")]
    CpuId,
    #[error("a required option returned None")]
    AyaNone,
    #[error("failed to parse the eBPF program")]
    AyaEbpf(#[from] AyaEbpfErr),
    #[error("failed to load the eBPF program")]
    AyaProgram(#[from] AyaProgramErr),
    #[error("failed to take map")]
    AyaMap(#[from] AyaMapErr),
    #[error("failed to open perf buffers")]
    AyaPerfBuffer(#[from] PerfBufferError),
    #[error("failed to send as the rx is dropped")]
    TokioSend(#[from] SendError<Message>),
}
