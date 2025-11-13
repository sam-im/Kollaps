// enum Runtime {
//     Wasmtime,
//     Wasmer,
// }
//
// impl Runtime {
//     fn create(&self, ns_name: &str) -> Result<()> {
//         match self {
//             Runtime::Wasmtime => todo!(),
//             Runtime::Wasmer => todo!(),
//         }
//     }
//     fn cleanup(&self, ns_name: &str) -> Result<()> {
//         match self {
//             Runtime::Wasmtime => todo!(),
//             Runtime::Wasmer => todo!(),
//         }
//     }
// }

//trait Runtime {
//    fn program(&self) -> String;
//    fn args(&self) -> String;
//}
//
//pub struct Wasmtime {
//    program: String,
//    args: String,
//}
//impl Runtime for Wasmtime {
//    fn new(image: &str, command: &str) -> Self {
//        let program = "wasmtime".to_string();
//        let args = command.to_string();
//        Self { program, args }
//    }
//    fn program(&self) -> String {}
//
//    fn args(tag: &str) -> String {}
//}
//
//pub struct Wasmer;
//impl Runtime for Wasmer {
//    fn program(tag: &str) -> String {
//        todo!()
//    }
//
//    fn args(tag: &str) -> String {
//        todo!()
//    }
//}
//
//pub struct Custom;
//impl Runtime for Custom {
//    fn program(tag: &str) -> String {
//        todo!()
//    }
//
//    fn args(tag: &str) -> String {
//        todo!()
//    }
//}

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
