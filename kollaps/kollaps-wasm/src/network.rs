use std::{net::Ipv4Addr, process::Command};

use anyhow::{Context, Error, Result};
use tracing::{debug, error, warn};

pub struct Bridge {
    name: String,
    addr: Ipv4Addr,
    subnet: u8,
    /// Addresses leased to namespaces.
    ns_addr: Vec<Ipv4Addr>,
}

impl Bridge {
    /// Creates a new virtual bridge using the linux `ip` command.
    ///
    /// Arguments:
    /// - name: identifier to refer to this bridge when using linux `ip`.
    /// - addr: first address of the subnet.
    /// - subnet: number of bits for host addressing in CIDR notation.
    pub fn try_new(name: &str, addr: Ipv4Addr, subnet: u8) -> Result<Self> {
        //ip link add name <name> type bridge
        let create = || run_ip_cmd(&["link", "add", "name", name, "type", "bridge"]);
        if let Err(e) = create() {
            // if it fails, try removing it and try again
            warn!(
                "Failed to create namespace {}.\nError: {}.\nTrying again.",
                name, e
            );

            // ip link set <name> down
            let _ = run_ip_cmd(&["link", "set", name, "down"]);
            // ip link del <name>
            let _ = run_ip_cmd(&["link", "del", name]);

            create()?;
        }

        // ip addr add <addr>/<subnet> dev <name>
        let cidr = format!("{}/{}", addr, subnet);
        run_ip_cmd(&["addr", "add", &cidr, "dev", name])?;

        // ip link set dev <name> up
        run_ip_cmd(&["link", "set", "dev", name, "up"])?;
        debug!("Created bridge {}.", name);

        let name = name.to_string();
        let ns_addr = Vec::new();
        Ok(Self {
            name,
            addr,
            subnet,
            ns_addr,
        })
    }

    pub fn create_namespace(&mut self) -> Result<Namespace> {
        let ns_count = self.ns_addr.len();

        let name = format!("k_ns_{}", ns_count);
        let veth = format!("k_veth_{}", ns_count);
        let addr = match self.ns_addr.last() {
            Some(addr) => Ipv4Addr::from_bits(addr.to_bits() + 1),
            None => {
                Ipv4Addr::from_bits(self.addr.to_bits() + 2) // skip the gateway
            }
        };
        let subnet_mask = match self.subnet {
            0 => 0,
            n => u32::MAX << (32 - n),
        };
        // Check if the subnet was full.
        // We add one to the namespace address because even though
        // an address ending with .255 will not change the subnet
        // it still is reserved for subnet broadcast.
        let tmp1 = self.addr.to_bits() & subnet_mask;
        let tmp2 = (addr.to_bits() + 1) & subnet_mask;
        if tmp1 != tmp2 {
            error!("Subnet was full.");
            return Err(Error::msg("Failed to create a namespace.")
                .context("Subnet is full, try again with larger subnet."));
        }

        let ns = Namespace::try_new(name, veth, addr, self.subnet, &self.name)?;
        self.ns_addr.push(ns.addr);
        Ok(ns)
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        // ip link set <name> down
        let _ = run_ip_cmd(&["link", "set", self.name.as_str(), "down"]);
        // ip link del <name>
        let _ = run_ip_cmd(&["link", "del", self.name.as_str()]);
        debug!("Removed bridge {}.", self.name);
    }
}

#[derive(Clone, Debug)]
pub struct Namespace {
    name: String,
    veth: String,
    veth_peer: String,
    addr: Ipv4Addr,
}

impl Namespace {
    fn try_new(
        name: String,
        veth: String,
        addr: Ipv4Addr,
        subnet: u8,
        bridge_name: &str,
    ) -> Result<Self> {
        let veth_peer = format!("{}_p", &veth);

        // ip netns add <name>
        let create = || run_ip_cmd(&["netns", "add", name.as_str()]);
        if let Err(e) = create() {
            // if it fails, try removing it and try again
            warn!(
                "Failed to create namespace {}.\nError: {}.\nTrying again.",
                name, e
            );
            run_ip_cmd(&["netns", "del", name.as_str()])?;
            create()?;
        }
        // ip link add <veth> type veth peer name <veth_peer>
        run_ip_cmd(&[
            "link",
            "add",
            veth.as_str(),
            "type",
            "veth",
            "peer",
            "name",
            veth_peer.as_str(),
        ])?;
        // ip link set <veth> netns <name>
        run_ip_cmd(&["link", "set", veth.as_str(), "netns", name.as_str()])?;
        // ip link set <veth_peer> master <bridge>
        run_ip_cmd(&["link", "set", veth_peer.as_str(), "master", bridge_name])?;
        // ip link set <veth_peer> up
        run_ip_cmd(&["link", "set", veth_peer.as_str(), "up"])?;
        // ip netns exec <name> ip addr add <addr> dev <veth>
        run_ip_cmd(&[
            "netns",
            "exec",
            name.as_str(),
            "ip",
            "addr",
            "add",
            &format!("{}/{}", addr, subnet),
            "dev",
            veth.as_str(),
        ])?;
        // ip netns exec <name> ip link set <veth> up
        run_ip_cmd(&[
            "netns",
            "exec",
            name.as_str(),
            "ip",
            "link",
            "set",
            veth.as_str(),
            "up",
        ])?;
        // ip netns exec <name> ip link set lo up
        run_ip_cmd(&[
            "netns",
            "exec",
            name.as_str(),
            "ip",
            "link",
            "set",
            "lo",
            "up",
        ])?;
        debug!("Created namespace {}.", name);

        Ok(Self {
            name,
            veth,
            veth_peer,
            addr,
        })
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn veth(&self) -> &str {
        &self.veth
    }
    pub fn addr(&self) -> &Ipv4Addr {
        &self.addr
    }
}

impl Drop for Namespace {
    fn drop(&mut self) {
        // ip netns del <name>
        let _ = run_ip_cmd(&["netns", "del", self.name.as_str()]);
        debug!("Removed namespace {}.", self.name);
    }
}

/// Helper function that runs the `ip` command on linux.
pub fn run_ip_cmd(args: &[&str]) -> Result<()> {
    let out = Command::new("ip").args(args).output().context(format!(
        "Failed to execute ip command with args: {:?}",
        args
    ))?;

    if !out.status.success() {
        let err_msg = match String::from_utf8(out.stderr) {
            Ok(output) => format!("ip command failed for args {:?} with:\n {}", args, output),
            Err(_) => format!("ip command failed for args {:?}", args),
        };
        warn!("{}", err_msg);
        return Err(anyhow::Error::msg(err_msg));
    }
    Ok(())
}
