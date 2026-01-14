use crate::Args;

use std::{
    env,
    net::Ipv4Addr,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

pub struct Config {
    pub tmp_dir: PathBuf,
    pub logs_dir: PathBuf,
    pub pipes_dir: PathBuf,
    pub remote_ips_path: PathBuf,
    pub topoinfo_path: PathBuf,
    pub topoinfodashboard_path: PathBuf,
    pub hosts_path: PathBuf,
    pub executables_dir: PathBuf,
    pub topology_path: PathBuf,
    pub allow_dir: Option<PathBuf>,
    pub addr: Ipv4Addr,
    pub subnet: u8,
    pub verbose: bool,
}

impl Default for Config {
    fn default() -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let tmp_dir = env::temp_dir().join("kollaps");
        let logs_dir = tmp_dir.join(format!("logs_{}", timestamp));

        let pipes_dir = tmp_dir.join("pipes");
        let remote_ips_path = tmp_dir.join("remote_ips.txt");
        let topoinfo_path = tmp_dir.join("topoinfo");
        let topoinfodashboard_path = tmp_dir.join("topoinfodashboard");

        let hosts_path = PathBuf::from("/etc/hosts");
        let executables_dir = env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .map(|p| p.join("bin"))
            .unwrap_or_else(|| PathBuf::from("./bin"));
        let topology_path = PathBuf::from("topology.xml");

        let allow_dir = None;
        let addr = Ipv4Addr::new(10, 10, 10, 0);
        let subnet = 24;
        let verbose = false;

        Self {
            tmp_dir,
            logs_dir,
            pipes_dir,
            remote_ips_path,
            topoinfo_path,
            topoinfodashboard_path,
            hosts_path,
            executables_dir,
            topology_path,
            allow_dir,
            addr,
            subnet,
            verbose,
        }
    }
}

impl From<Args> for Config {
    fn from(args: Args) -> Self {
        let mut config = Config {
            topology_path: args.topology,
            allow_dir: args.allow_dir,
            ..Default::default()
        };

        if let Some(addr) = args.addr {
            config.addr = addr;
        }
        if let Some(subnet) = args.subnet {
            config.subnet = subnet;
        }
        config.verbose = args.verbose;

        config
    }
}
