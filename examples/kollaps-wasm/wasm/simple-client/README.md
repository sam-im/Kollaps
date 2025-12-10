# Simple Client
This is an example program to test `kollaps-wasm`.

## Build
Building requires the `wasm32-wasip2` target, install it using `rustup`.

Run `cargo build --target wasm32-wasip2` to build and use with `kollaps-wasm`.
Find the built module in `target/wasm32-wasip2/debug/simple-client.wasm`.

## Usage
This module requires an address and a port number as first and second arguments, see `../topology.xml`.
