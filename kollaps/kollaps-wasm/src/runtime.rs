use anyhow::Result;

use crate::network::run_ip_cmd;

pub struct Runtime {
    cmd: String,
    args: Vec<String>,
}

impl Runtime {
    fn create(&self, ns: &str, wasm: &str) -> Result<()> {
        // ip netns exec ns-wasm1 wasmtime ./module.wasm
        run_ip_cmd(&["netns", "exec", ns, self.cmd.as_str(), wasm])?;
        // ip netns exec ns-wasm1 ./emulationcore (don't implement this here)
        Ok(())
    }
}
