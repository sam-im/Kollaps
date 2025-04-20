#![no_std]
#![no_main]

use monitor_common::{Message, SocketAddr};
use network_types::{ip::Ipv4Hdr, eth::{EthHdr, EtherType}};

use aya_ebpf::{
    macros::{map, xdp},
    maps::{HashMap, PerfEventArray},
    programs::XdpContext,
    helpers::bpf_ktime_get_ns,
    bindings::xdp_action
};

use core::mem;

// Map to hold perf_events
#[map(name="test")]
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
        Ok(_)  => xdp_action::XDP_PASS,
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
    let dst = SocketAddr::new(
        u32::from_be_bytes(unsafe { (*ipv4hdr).dst_addr })
    );
    let len: u32 = (ctx.data_end() - ctx.data()) as u32;

    unsafe {
        let now = bpf_ktime_get_ns();

        match TIME.get(&dst.addr) {
            None => {
                TIME.insert(&dst.addr, &now, 0)?;
            },
            Some(time) => {
                if now - time > 5_000_000 {
                    match USAGE.get(&dst.addr) {
                        None => {
                            USAGE.insert(&dst.addr, &len, 0)?;
                        },
                        Some(value) => {
                            let new_len = value + len;
                            USAGE.insert(&dst.addr, &new_len, 0)?;
                            let msg = Message { dst: dst.addr, bytes: new_len };
                            PERF_EVENTS.output(&ctx, &msg, 0);
                            TIME.insert(&dst.addr, &now, 0)?;
                        }
                    }
                } else {
                    match USAGE.get(&dst.addr) {
                        None => {
                            USAGE.insert(&dst.addr, &len, 0)?;
                        },
                        Some(value) => {
                            let newvalue = value + len;
                            USAGE.insert(&dst.addr, &newvalue, 0)?;
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
unsafe fn ptr_at<T>(
    ctx: &XdpContext, offset: usize
) -> Result<*const T, i64> {
    let start = ctx.data();
    let end = ctx.data_end();
    let len = mem::size_of::<T>();

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
static LICENSE: [u8; 13] = *b"Dual MIT/GPL\0";
