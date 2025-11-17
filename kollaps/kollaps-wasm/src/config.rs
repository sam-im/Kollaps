// TODO: consider an option to start runtimes with another user
// TODO: give arguments types

pub struct Config {
    pub tmp_dir: String,
    pub remote_ips_path: String,
    pub topoinfo_path: String,
    pub topoinfodashboard_path: String,
    pub topology_path: String,
    pub addr: String,
    pub subnet: u8,
    // pub if_name: String,
}

impl Default for Config {
    fn default() -> Self {
        let tmp_dir = "/tmp/kollaps/".to_owned();
        let remote_ips_path = format!("{}remote_ips.txt", tmp_dir);
        let topoinfo_path = format!("{}topoinfo", tmp_dir);
        let topoinfodashboard_path = format!("{}topoinfodashboard", tmp_dir);
        let topology_path = "topology.xml".to_owned();
        // let if_name = "eth0".to_owned();
        let addr = "10.10.10.0".to_owned();
        let subnet = 24;

        Self {
            tmp_dir,
            remote_ips_path,
            topoinfo_path,
            topoinfodashboard_path,
            // if_name,
            topology_path,
            addr,
            subnet,
        }
    }
}
