use std::{net::Ipv4Addr, process::Command};

use anyhow::{Context, Result};
use tracing::{error, info};

pub struct Bridge {
    name: String,
    addr: Ipv4Addr,
    subnet: u32,
    namespaces: Vec<Namespace>,
}

impl Bridge {
    /// Creates a new virtual bridge struct.
    /// To create the bridge itself on linux, call `create` on this struct.
    /// To remove it, call `cleanup`.
    ///
    /// Arguments:
    /// - name: identifier to refer to this bridge when using linux `ip`.
    /// - addr: first address of the subnet.
    /// - subnet: number of bits for host addressing in CIDR notation.
    pub fn new(name: &str, addr: Ipv4Addr, subnet: u8) -> Self {
        let name = name.to_string();
        let subnet = match subnet {
            0 => 0,
            n => u32::MAX << (32 - n),
        };
        let namespaces = Vec::new();

        Self {
            name,
            addr,
            subnet,
            namespaces,
        }
    }

    pub fn get_ns(&self, name: &str) -> Option<&Namespace> {
        self.namespaces.iter().find(|ns| ns.name == name)
    }

    pub fn create(&self) -> Result<()> {
        //ip link add name <name> type bridge
        run_ip_cmd(&["link", "add", "name", self.name.as_str(), "type", "bridge"])?;

        // ip addr add <addr>/<subnet> dev <name>
        let cidr = format!("{}/{}", self.addr, self.subnet);
        run_ip_cmd(&["addr", "add", cidr.as_str(), "dev", self.name.as_str()])?;

        // ip link set dev <name> up
        run_ip_cmd(&["link", "set", "dev", self.name.as_str(), "up"])?;

        Ok(())
    }

    fn cleanup(&self) {
        self.namespaces.iter().for_each(|ns| {
            ns.cleanup();
        });
        // ip link set <name> down
        let _ = run_ip_cmd(&["link", "set", self.name.as_str(), "down"]);
        // ip link del <name>
        let _ = run_ip_cmd(&["link", "del", self.name.as_str()]);
    }

    pub fn create_namespace(&mut self) -> Result<Namespace> {
        let ns_count = self.namespaces.len();

        let name = format!("k_ns_{}", ns_count);
        let veth = format!("k_veth_{}", ns_count);
        let addr = match self.namespaces.last() {
            Some(ns) => Ipv4Addr::from_bits(ns.addr.to_bits() + 1),
            None => {
                Ipv4Addr::from_bits(self.addr.to_bits() + 2) // skip the gateway
            }
        };

        // Check if the subnet was full.
        // We add one to the namespace address because even though
        // an address ending with .255 will not change the subnet
        // it still is reserved for subnet broadcast.
        let tmp1 = self.addr.to_bits() & self.subnet;
        let tmp2 = (addr.to_bits() + 1) & self.subnet;
        if tmp1 != tmp2 {
            // TODO: return an error type
            error!("Subnet was full.");
        }

        let ns = Namespace::new(name.clone(), veth, addr);
        self.namespaces.push(ns.clone());
        Ok(ns)
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        info!("Cleaning up the bridge and namespaces.");
        self.cleanup();
    }
}

#[derive(Clone, Debug)]
pub struct Namespace {
    pub name: String,
    pub veth: String,
    veth_peer: String,
    pub addr: Ipv4Addr,
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
        Ok(())
    }
    fn cleanup(&self) {
        // ip netns del <name>
        let _ = run_ip_cmd(&["netns", "del", self.name.as_str()]);
    }
}

/// Helper function that runs the `ip` command on linux.
pub fn run_ip_cmd(args: &[&str]) -> Result<()> {
    let out = Command::new("ip").args(args).output().context(format!(
        "Failed to execute ip command with args: {:?}",
        args
    ))?;

    if !out.status.success() {
        let err_msg = match String::from_utf8(out.stdout) {
            Ok(output) => format!("ip command failed for args {:?} with:\n {}", args, output),
            Err(_) => format!("ip command failed for args {:?}", args),
        };
        error!("{}", err_msg);
        // TODO: check the way you handled errors in usage before returning an error
        // and potentially causing panics.
        // return Err(anyhow::Error::msg(err_msg));
    }
    Ok(())
}
