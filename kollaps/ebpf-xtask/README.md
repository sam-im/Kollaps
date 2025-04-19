# Why does this crate exists?
This crate's purpose is to automate generating bindings for the kernel structures that we need such as `iphdr` such as `vmlinux.h`.

These bindings are used in `ebpfs-progs` crate, more specifically in `../ebpfs-progs/src/bindings.rs`.

Note that it is *not* necessary to add this step into the compilation process, 
as the generated bindings doesn't require changes unless we want to use other kernel
structures in the `ebpfs-progs` crate.

# Dependencies
- `bpf-linker`: run `cargo install bpf-linker` to install
- `bindgen-cli`: run `cargo install bindgen-cli` to install
- `bpftool`: use your system package manager, e.g. for pacman: `pacman -S bpf`

# Generating new bindings
Run the following in the root of the workspace to generate bindings:
`cargo run --package ebpf-xtask -- codegen ebpf-progs/src/bindings.rs`

