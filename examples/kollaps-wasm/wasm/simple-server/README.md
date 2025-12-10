# Simple Server
This is an example program to test `kollaps-wasm`.

## Build
Building requires the `wasm32-wasip2` target, install it using `rustup`.

Run `cargo build --target wasm32-wasip2` to build and use with `kollaps-wasm`.
Find the produced WASM module in `target/wasm32-wasip2/debug/simple-server.wasm`.

## Usage
This module requires a port number as first argument, see `../topology.xml`.

