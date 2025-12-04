# Default WASI runtime for `kollaps-wasm`
This crate implements the default WASI runtime for `kollaps-wasm` using `wasmtime` with `wasi-preview-2` and networking allowed by default.

## Usage
- Run `./kollaps-wasm-runtime -h` to see the available options.
- Run `./kollaps-wasm-runtime module.wasm` to run the WASM module specified by the path `module.wasm`.
