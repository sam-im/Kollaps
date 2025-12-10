use crate::Args;

use tracing::{debug, warn};
use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::*;
use wasmtime_wasi::p2::bindings::sync::Command;
use wasmtime_wasi::{DirPerms, FilePerms, WasiCtx, WasiCtxView, WasiView};

pub struct Runtime {
    args: Args,
}

impl From<Args> for Runtime {
    fn from(value: Args) -> Self {
        Self { args: value }
    }
}

impl Runtime {
    /// Initializes a wasmtime runtime and runs the wasm module with it.`.
    pub fn run(&self) -> Result<()> {
        let prog_name = &self
            .args
            .wasm_path
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("wasm-module");

        // Define the WASI functions globally on the `Config`.
        let engine = Engine::default();
        let mut linker = Linker::new(&engine);
        wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;

        // Create a WASI context and put it in a Store; all instances in the store
        // share this context. `WasiCtx` provides a number of ways to
        // configure what the target program will have access to.
        let mut wasi_builder = WasiCtx::builder();
        wasi_builder
            .inherit_stdio()
            .inherit_network()
            .arg(prog_name)
            .args(&self.args.wasm_args);

        if let Some(path) = &self.args.opt_dir {
            debug!(
                "Opening directory {} with default permissions.",
                path.to_string_lossy()
            );
            wasi_builder.preopened_dir(path, ".", DirPerms::all(), FilePerms::all())?;
        }

        let wasi_ctx = wasi_builder.build();

        let state = ComponentRunStates {
            wasi_ctx,
            resource_table: ResourceTable::new(),
        };
        let mut store = Store::new(&engine, state);

        // Instantiate our component with the imports we've created, and run it.
        let component = Component::from_file(&engine, &self.args.wasm_path)?;
        let command = Command::instantiate(&mut store, &component, &linker)?;
        let program_result = command.wasi_cli_run().call_run(&mut store)?;
        if program_result.is_err() {
            warn!("{} exited with an error.", prog_name);
        }
        Ok(())
    }
}

struct ComponentRunStates {
    // These two are required basically as a standard way to enable the impl of IoView and
    // WasiView.
    // impl of WasiView is required by [`wasmtime_wasi::p2::add_to_linker_sync`]
    pub wasi_ctx: WasiCtx,
    pub resource_table: ResourceTable,
    // You can add other custom host states if needed
}

impl WasiView for ComponentRunStates {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi_ctx,
            table: &mut self.resource_table,
        }
    }
}
