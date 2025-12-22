# Default WASI runtime for `kollaps-wasm`
This crate implements the default WASI runtime used by `kollaps-wasm` to run WASM modules compiled to `wasi-preview-2`.

By default the runtime is set to allow host-side networking using the flag `inherit-networking` of `wasmtime`.

The only required argument is the path to the WASM module you want to run. 
Run `./kollaps-wasm-runtime -h` to see the available options.

## Example
Run `./kollaps-wasm-runtime path/to/module.wasm` to run the WASM module named `module.wasm`.
