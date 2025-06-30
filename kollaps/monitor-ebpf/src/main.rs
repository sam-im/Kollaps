#![no_std]
#![no_main]

use monitor_common::{Message, SocketAddr};
use network_types::{
    eth::{EthHdr, EtherType},
    ip::Ipv4Hdr,
};

use aya_ebpf::{
    bindings::xdp_action,
    helpers::bpf_ktime_get_ns,
    macros::{map, xdp},
    maps::{HashMap, PerfEventArray},
    programs::XdpContext,
};

// Map to hold perf_events
#[map]
static PERF_EVENTS: PerfEventArray<Message> = PerfEventArray::new(0);

// Map to accumulate bytes per dst IP
#[map]
static USAGE: HashMap<u32, u32> = HashMap::with_max_entries(4096, 0);

// Map to track last update time per dst IP
#[map]
static TIME: HashMap<u32, u64> = HashMap::with_max_entries(4096, 0);

#[xdp]
pub fn monitor(ctx: XdpContext) -> u32 {
    match try_measure_tcp_lifetime(ctx) {
        Ok(_) => xdp_action::XDP_PASS,
        Err(_) => xdp_action::XDP_PASS,
    }
}

fn try_measure_tcp_lifetime(ctx: XdpContext) -> Result<(), i64> {
    let ethhdr: *const EthHdr = unsafe { ptr_at(&ctx, 0)? };
    match unsafe { (*ethhdr).ether_type } {
        EtherType::Ipv4 => {}
        _ => return Ok(()),
    }

    let ipv4hdr: *const Ipv4Hdr = unsafe { ptr_at(&ctx, EthHdr::LEN)? };
    // While rewriting monitor, I used `xdp` instead of `socket_filter` for the performance benefits.
    // But since `xdp` gets attached to the virtual ethernet interface of a container,
    // the source and target addresses of packets are inversed.
    // TODO Switch xdp back to socket_filter (should require minimal change).
    let dst = SocketAddr {
        addr: u32::from_be_bytes(unsafe { (*ipv4hdr).src_addr }),
    };

    let len: u32 = (ctx.data_end() - ctx.data()) as u32;

    unsafe {
        let time = bpf_ktime_get_ns();

        match TIME.get(&dst.addr) {
            None => {
                TIME.insert(&dst.addr, &time, 0)?;
            }
            Some(prev_time) => {
                match USAGE.get(&dst.addr) {
                    None => {
                        USAGE.insert(&dst.addr, &len, 0)?;
                    }
                    Some(prev_len) => {
                        let new_len = prev_len + len;

                        if time - prev_time > 5_000_000 {
                            let msg = Message {
                                dst: dst.addr,
                                bytes: new_len,
                            };
                            PERF_EVENTS.output(&ctx, &msg, 0);
                            // TODO consider changing 'accumulating bytes' to
                            // 'bytes since last send' to the PERF_EVENTS and reset it to zero.
                            // As far as I understand, currently there is a chance
                            // that the USAGE's u32 will overflow after receiving 4GB of packet lengths.
                            // This will potentially require changes to the logic in emulationcore
                            USAGE.insert(&dst.addr, &new_len, 0)?;
                            TIME.insert(&dst.addr, &time, 0)?;
                        } else {
                            USAGE.insert(&dst.addr, &new_len, 0)?;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

// Provides safe access to a generic type T within an XdpContext at a specified offset.
// It performs bounds checking by comparing the desired memory range
// (start + offset + len) against the end of the data (end).
#[inline(always)]
unsafe fn ptr_at<T>(ctx: &XdpContext, offset: usize) -> Result<*const T, i64> {
    let start = ctx.data();
    let end = ctx.data_end();
    let len = core::mem::size_of::<T>();

    if start + offset + len > end {
        return Err(-1);
    }

    let ptr = (start + offset) as *const T;
    Ok(&*ptr)
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[link_section = "license"]
#[no_mangle]
static LICENSE: [u8; 13] = *b"Dual MIT/GPL\0";  // TODO correct licence
