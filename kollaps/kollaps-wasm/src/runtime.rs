use anyhow::Result;

enum Runtime {
    Wasmtime,
    Wasmer,
}

impl Runtime {
    fn create(&self, ns_name: &str) -> Result<()> {
        match self {
            Runtime::Wasmtime => todo!(),
            Runtime::Wasmer => todo!(),
        }
    }
    fn cleanup(&self, ns_name: &str) -> Result<()> {
        match self {
            Runtime::Wasmtime => todo!(),
            Runtime::Wasmer => todo!(),
        }
    }
}

// pub struct Runtime {
//     cmd: String,
//     args: Vec<String>,
// }
//
// impl Runtime {
//     fn create(&self, ns: &str, wasm: &str) -> Result<()> {
//         use crate::network::run_ip_cmd;
//         // ip netns exec ns-wasm1 wasmtime ./module.wasm
//         run_ip_cmd(&["netns", "exec", ns, self.cmd.as_str(), wasm])?;
//         // ip netns exec ns-wasm1 ./emulationcore (don't implement this here)
//         Ok(())
//     }
// }
