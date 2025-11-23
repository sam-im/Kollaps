use std::{net::Ipv4Addr, process::Command};

use anyhow::{Context, Error, Result};
use tracing::{debug, error};

pub struct Bridge {
    name: String,
    addr: Ipv4Addr,
    subnet: u8,
    /// Addresses leased to namespaces.
    ns_addr: Vec<Ipv4Addr>,
}

impl Bridge {
    /// Creates a new virtual bridge struct.
    /// To create the associated bridge on linux, call `create` on this struct.
    ///
    /// Arguments:
    /// - name: identifier to refer to this bridge when using linux `ip`.
    /// - addr: first address of the subnet.
    /// - subnet: number of bits for host addressing in CIDR notation.
    pub fn new(name: &str, addr: Ipv4Addr, subnet: u8) -> Self {
        let name = name.to_string();
        let ns_addr = Vec::new();

        Self {
            name,
            addr,
            subnet,
            ns_addr,
        }
    }

    /// Create a virtual on the host using `ip`.
    pub fn create(&self) -> Result<()> {
        //ip link add name <name> type bridge
        run_ip_cmd(&["link", "add", "name", self.name.as_str(), "type", "bridge"])?;

        // ip addr add <addr>/<subnet> dev <name>
        let cidr = format!("{}/{}", self.addr, self.subnet);
        run_ip_cmd(&["addr", "add", cidr.as_str(), "dev", self.name.as_str()])?;

        // ip link set dev <name> up
        run_ip_cmd(&["link", "set", "dev", self.name.as_str(), "up"])?;

        debug!("Created bridge {}.", self.name);
        Ok(())
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

        let ns = Namespace::new(name.clone(), veth, addr);
        ns.create(&self.name, self.subnet)?;
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
    fn new(name: String, veth: String, addr: Ipv4Addr) -> Self {
        let veth_peer = format!("{}_p", &veth);
        Self {
            name,
            veth,
            veth_peer,
            addr,
        }
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
    fn create(&self, bridge: &str, subnet: u8) -> Result<()> {
        // ip netns add <name>
        run_ip_cmd(&["netns", "add", self.name.as_str()])?;
        // ip link add <veth> type veth peer name <veth_peer>
        run_ip_cmd(&[
            "link",
            "add",
            self.veth.as_str(),
            "type",
            "veth",
            "peer",
            "name",
            self.veth_peer.as_str(),
        ])?;
        // ip link set <veth> netns <name>
        run_ip_cmd(&[
            "link",
            "set",
            self.veth.as_str(),
            "netns",
            self.name.as_str(),
        ])?;
        // ip link set <veth_peer> master <bridge>
        run_ip_cmd(&["link", "set", self.veth_peer.as_str(), "master", bridge])?;
        // ip link set <veth_peer> up
        run_ip_cmd(&["link", "set", self.veth_peer.as_str(), "up"])?;
        // ip netns exec <name> ip addr add <addr> dev <veth>
        run_ip_cmd(&[
            "netns",
            "exec",
            self.name.as_str(),
            "ip",
            "addr",
            "add",
            &format!("{}/{}", self.addr, subnet),
            "dev",
            self.veth.as_str(),
        ])?;
        // ip netns exec <name> ip link set <veth> up
        run_ip_cmd(&[
            "netns",
            "exec",
            self.name.as_str(),
            "ip",
            "link",
            "set",
            self.veth.as_str(),
            "up",
        ])?;
        // ip netns exec <name> ip link set lo up
        run_ip_cmd(&[
            "netns",
            "exec",
            self.name.as_str(),
            "ip",
            "link",
            "set",
            "lo",
            "up",
        ])?;
        debug!("Created namespace {}.", self.name);
        Ok(())
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
        error!("{}", err_msg);
        return Err(anyhow::Error::msg(err_msg));
    }
    Ok(())
}
