#![no_std]
#![no_main]

use monitor_common::{Message, SocketAddr};
use network_types::{
    eth::{EthHdr, EtherType},
    ip::Ipv4Hdr,
};

use aya_ebpf::{
    helpers::bpf_ktime_get_ns,
    macros::{map, socket_filter},
    maps::{HashMap, PerfEventArray},
    programs::SkBuffContext,
};

// Shares usage information with the userspace
#[map]
static PERF_EVENTS: PerfEventArray<Message> = PerfEventArray::new(0);

// Accumulates bytes per destination address
#[map]
static USAGE: HashMap<u32, u32> = HashMap::with_max_entries(4096, 0);

// Tracks the last update time per destination address
#[map]
static TIME: HashMap<u32, u64> = HashMap::with_max_entries(4096, 0);

/// Inspects and reports usage per destination address of outgoing IPv4 packets.
#[socket_filter]
pub fn monitor(ctx: SkBuffContext) -> i64 {
    match try_monitor(&ctx) {
        Ok(_) => 0,
        Err(_) => 0,
    }
}

fn try_monitor(ctx: &SkBuffContext) -> Result<(), i64> {
    let ethhdr: EthHdr = ctx.load(0)?;
    match ethhdr.ether_type {
        EtherType::Ipv4 => {}
        _ => return Ok(()),
    }
    let ipv4hdr: Ipv4Hdr = ctx.load(core::mem::size_of::<EthHdr>())?;

    let dst = SocketAddr {
        addr: u32::from_be_bytes(ipv4hdr.dst_addr),
    };
    let len: u32 = ctx.len();

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
                            PERF_EVENTS.output(ctx, &msg, 0);
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

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[link_section = "license"]
#[no_mangle]
static LICENSE: [u8; 13] = *b"Dual MIT/GPL\0";
