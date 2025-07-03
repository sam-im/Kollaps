# Building from source
## Install dependencies
The `monitor` and `capnp-schemas` crates require the following dependencies to build.

### For monitor
- rust nightly toolchain: `rustup toolchain install nightly --component rust-src`
- bpftool: for Debian 12 `apt install bpftool llvm-14-dev libclang-14-dev libpolly-14-dev pkg-config libssl-dev`
- bpf-linker: `cargo install bpf-linker`

### For capnp-schemas
- capnproto: for Debian 12 `apt install capnproto`

## Build
Run `cargo build --release` in the root of the workspace.
The compiled binaries will be located in `./target/release/`.

# Running an example
After the build to run an example you can simply follow the instructions in the README file of the root of this project.
