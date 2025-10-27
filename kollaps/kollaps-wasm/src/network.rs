use std::{net::Ipv4Addr, process::Command};

use anyhow::{Context, Result};

pub struct Bridge {
    name: String,
    addr: Ipv4Addr,
    subnet: u8,
    namespaces: Vec<Namespace>
}

impl Bridge {
    fn new(name: String, addr: Ipv4Addr, subnet: u8) -> Self {
        let namespaces = Vec::new();
        Self {
            name,
            addr,
            subnet,
            namespaces
        }
    }

    fn create(&self) -> Result<()> {
        //ip link add name <name> type bridge
        run_ip_cmd(&["link", "add", "name", self.name.as_str(), "type", "bridge"])?;

        // ip addr add <addr>/<subnet> dev <name>
        let cidr = format!("{}/{}", self.addr, self.subnet);
        run_ip_cmd(&["addr", "add", cidr.as_str(), "dev", self.name.as_str()])?;

        // ip link set dev <name> up
        run_ip_cmd(&["link", "set", "dev", self.name.as_str(), "up"])?;

        Ok(())
    }

    fn cleanup(&self) -> Result<()> {
        self.namespaces.iter().for_each(|ns| { let _ = ns.cleanup(); });

        // ip link set <name> down
        let _ = run_ip_cmd(&["link", "set", self.name.as_str(), "down"]);
        // ip link del <name>
        let _ = run_ip_cmd(&["link", "del", self.name.as_str()]);

        Ok(())
    }

    fn create_namespace(&self) -> Result<()> {
        let ns_count = self.namespaces.len();
        let name = format!("k_ns_{}", ns_count);
        let veth = format!("k_veth_{}", ns_count);
        
        // TODO:
        // define next available addr
        //   next_addr := last_addr + 1
        //   check := (self_addr AND subnet) == (next_addr AND subnet)
        //   if check is not true, then the subnet was full
        // create new namespace
        // push it to self.namespaces

        todo!()
    }
}

pub struct Namespace {
    name: String,
    veth: String,
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
        run_ip_cmd(&["link", "add", self.veth.as_str(), "type", "veth", "peer", "name", self.veth_peer.as_str()])?;
        // ip link set <veth> netns <name>
        run_ip_cmd(&["link", "set", self.veth.as_str(), "netns", self.name.as_str()])?;
        // ip link set <veth_peer> master <bridge>
        run_ip_cmd(&["link", "set", self.veth_peer.as_str(), "master", bridge])?;
        // ip link set <veth_peer> up
        run_ip_cmd(&["link", "set", self.veth_peer.as_str(), "up"])?;
        // ip netns exec <name> ip addr add <addr> dev <veth>
        run_ip_cmd(&["netns", "exec", self.name.as_str(), "ip", "addr", "add", &format!("{}/{}", self.addr.to_string(), subnet), "dev", self.veth.as_str()])?;
        // ip netns exec <name> ip link set <veth> up
        run_ip_cmd(&["netns", "exec", self.name.as_str(), "ip", "link", "set", self.veth.as_str(), "up"])?;
        // ip netns exec <name> ip link set lo up
        run_ip_cmd(&["netns", "exec", self.name.as_str(), "ip", "link", "set", "lo", "up"])?;
        Ok(())
    }
    fn cleanup(&self) -> Result<()> {
        // ip netns del <name>
        run_ip_cmd(&["netns", "del", self.name.as_str()])?;
        Ok(())
    }
}

/// Helper function that runs the `ip` command.
pub fn run_ip_cmd(args: &[&str]) -> Result<()> {
    let out = Command::new("ip")
        .args(args)
        .output()
        .context(format!("Failed to execute ip with args: {:?}", args))?;

    if !out.status.success() {
        todo!(); // TODO: handle failure
    }
    Ok(())
}

