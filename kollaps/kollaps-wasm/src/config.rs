// TODO: consider an option to start runtimes with another user
// TODO: give types to arguments

pub struct Config {
    pub tmp_dir: String,
    pub logs_dir: String,
    pub pipes_dir: String,
    pub remote_ips_path: String,
    pub topoinfo_path: String,
    pub topoinfodashboard_path: String,
    pub topology_path: String,
    pub addr: String,
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
        let topology_path = "topology.xml".to_owned();
        let addr = "10.10.10.0".to_owned();
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
