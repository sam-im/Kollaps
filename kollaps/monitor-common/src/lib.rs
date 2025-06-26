//! This library crate contains the shared data structures for both monitor-*
//! and it's consumers.

#![no_std]

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct Message {
    pub dst: u32,
    pub bytes: u32,
}

#[cfg(feature = "user")]
unsafe impl aya::Pod for Message {}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SocketAddr {
    pub addr: u32,
}

#[cfg(feature = "user")]
unsafe impl aya::Pod for SocketAddr {}
