# Workspace Structure
.
├── bootstrapper  ----------> Handles container deployments
├── capnp-schemas  ---------> Shared message type for metadata dissemination
├── controller  ------------> Start/stop experiments for Baremetal deployments
├── communicationcore  -----> Allows dashboard to read metadata
├── communicationmanager  --> Disseminate metadata between emulationcore instances
├── dashboard  -------------> Start/stop/view experiments
├── emulationcore  ---------> Emulates network properties per service
├── kollaps-wasm  ----------> Handles WASI and non-containerized deployments
├── kollaps-wasm-runtime ---> Default WASI runtime
├── monitor  ---------------> eBPF userspace library
├── monitor-common  --------> eBPF shared datastructures
├── monitor-ebpf    --------> eBPF kernel program
├── TCAL  ------------------> tc abstraction layer
└── tools  -----------------> Utilities and language extensions

# Build
## Rust Crates
### Dependencies
The `monitor` and `capnp-schemas` crates require the following extra dependencies.

`monitor`:
- rust nightly toolchain: `rustup toolchain install nightly --component rust-src`
- bpftool: for Debian 12 `apt install bpftool llvm-14-dev libclang-14-dev libpolly-14-dev pkg-config libssl-dev`
- bpf-linker: `cargo install bpf-linker`

`capnp-schemas`:
- capnproto: for Debian 12 `apt install capnproto`

### Build
Run `cargo build --release` in the root of the workspace.
The compiled binaries will be located in `./target/release/`.

## C Libraries
### TCAL
#### Dependencies
Building requires the following packages `build-essential bison flex`.
The package `build-essential` might have different names in other linux distributions.

Example:
- Debian 12: `apt install build-essential bison flex`

#### Build
Run `make` inside `TCAL/`.

### pid1
#### Build
Run `make` inside `tools/pid1/`.

