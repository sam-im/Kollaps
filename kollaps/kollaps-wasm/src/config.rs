// TODO: consider an option to start runtimes with another user

pub struct Config {
    pub tmp_dir: String,
    pub remote_ips_path: String,
    pub topoinfo_path: String,
    pub topoinfodashboard_path: String,
    pub addr: String,
    pub wasm_dir: String,
    pub runtime_name: String,
    pub if_name: String,
    pub topology_path: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            tmp_dir: "/tmp/kollaps/".to_string(),
            remote_ips_path: "remote_ips".to_string(),
            topoinfo_path: "topoinfo".to_string(),
            topoinfodashboard_path: "topoinfodashboard".to_string(),
            wasm_dir: "wasm/".to_string(),
            runtime_name: "wasmtime".to_string(),
            if_name: "eth0".to_string(),
            topology_path: "topology.xml".to_string(),
            addr: "10.10.10.0".to_string(),
        }
    }
}
