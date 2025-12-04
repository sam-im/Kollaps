use crate::Args;

use std::path::PathBuf;

use tracing::{debug, warn};
use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::*;
use wasmtime_wasi::p2::bindings::sync::Command;
use wasmtime_wasi::{DirPerms, FilePerms, WasiCtx, WasiCtxView, WasiView};

pub struct Runtime {
    /// Path to the WASM module.
    module: PathBuf,
    /// Optionnaly allow a directory on the host to be accessible
    /// at the current directory in the WASM module.
    dir: Option<PathBuf>,
}

impl From<Args> for Runtime {
    fn from(value: Args) -> Self {
        Self {
            module: value.path,
            dir: value.dir,
        }
    }
}

impl Runtime {
    /// Initialize a wasmtime runtime with networking enabled and run the `module`.
    /// Optionally allowing a directory to be accessible as well.
    pub fn run(&self) -> Result<()> {
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
            .inherit_args()
            .inherit_network();
        if let Some(path) = &self.dir {
            debug!("Opening directory {} with default permissions.", path.to_string_lossy());
            wasi_builder.preopened_dir(path, ".", DirPerms::all(), FilePerms::all())?;
        }
        let wasi = wasi_builder.build();

        let state = ComponentRunStates {
            wasi_ctx: wasi,
            resource_table: ResourceTable::new(),
        };
        let mut store = Store::new(&engine, state);

        // Instantiate our component with the imports we've created, and run it.
        let component = Component::from_file(&engine, &self.module)?;
        let command = Command::instantiate(&mut store, &component, &linker)?;
        let program_result = command.wasi_cli_run().call_run(&mut store)?;
        if program_result.is_err() {
            warn!("WASM module exited with an error.");
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
