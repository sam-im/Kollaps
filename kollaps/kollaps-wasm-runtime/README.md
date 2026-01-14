# Default WASI runtime for `kollaps-wasm`
This crate implements the default WASI runtime used by `kollaps-wasm` to run WASM modules compiled to `wasi-preview-2`.

By default the runtime is set to allow host-side networking using the flag `inherit-networking` of `wasmtime`.

The only required argument is the path to the WASM module you want to run. 
Run `./kollaps-wasm-runtime -h` to see the available options.

```
$ ./kollaps-wasm-runtime -h
Usage: kollaps-wasm-runtime [OPTIONS] <WASM_PATH> [WASM_ARGS]...

Arguments:
  <WASM_PATH>     Specifies a path to a WASM module
  [WASM_ARGS]...  List of arguments that will be passed to the WASM module

Options:
      --opt-dir <OPT_DIR>  Optionally map a directory in the host as the current directory in the runtime
  -h, --help               Print help
```

## Example
Run `./kollaps-wasm-runtime path/to/module.wasm` to run the WASM module named `module.wasm`.
