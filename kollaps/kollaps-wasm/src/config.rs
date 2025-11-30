use crate::Args;

use std::{net::Ipv4Addr, path::PathBuf};

pub struct Config {
    pub tmp_dir: String,
    pub logs_dir: String,
    pub pipes_dir: String,
    pub remote_ips_path: String,
    pub topoinfo_path: String,
    pub topoinfodashboard_path: String,
    pub topology_path: PathBuf,
    pub addr: Ipv4Addr,
    pub subnet: u8,
}

impl Default for Config {
    fn default() -> Self {
        let tmp_dir = "/tmp/kollaps/".to_owned();
        let logs_dir = format!("{}logs/", tmp_dir);
        let pipes_dir = format!("{}pipes/", tmp_dir);
        let remote_ips_path = format!("{}remote_ips.txt", tmp_dir);
        let topoinfo_path = format!("{}topoinfo", tmp_dir);
        let topoinfodashboard_path = format!("{}topoinfodashboard", tmp_dir);
        let topology_path = PathBuf::from("topology.xml");
        let addr = Ipv4Addr::new(10, 10, 10, 0);
        let subnet = 24;

        Self {
            tmp_dir,
            logs_dir,
            pipes_dir,
            remote_ips_path,
            topoinfo_path,
            topoinfodashboard_path,
            topology_path,
            addr,
            subnet,
        }
    }
}

impl From<Args> for Config {
    fn from(args: Args) -> Self {
        let mut config = Config {
            topology_path: args.topology,
            ..Default::default()
        };

        if let Some(addr) = args.addr {
            config.addr = addr;
        }
        if let Some(subnet) = args.subnet {
            config.subnet = subnet;
        }

        config
    }
}
