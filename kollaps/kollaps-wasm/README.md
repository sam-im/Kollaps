# kollaps-wasm
Run single host Kollaps experiments.

## Building
See `../README.md`.

## Installation
Kollaps requires some of the binaries built on the previous step to be available in a directory called `dir` relative to itself.
Specifically the required binaries are `communicationmanager`, `emulationcore`, `kollaps-wasm-runtime`, and `libTCAL.so`.

This is the required structure to run `kollaps-wasm`.
``` text
.
├── kollaps-wasm
└── bin/
    ├── communicationmanager
    ├── emulationcore
    ├── kollaps-wasm-runtime
    └── libTCAL.so
```

### Example
``` sh
mkdir new-dir new-dir/bin/
mv Kollaps/kollaps/target/release/kollaps-wasm new-folder/
mv Kollaps/kollaps/target/release/communicationmanager new-folder/bin/
mv Kollaps/kollaps/target/release/emulationcore new-folder/bin/
mv Kollaps/kollaps/target/release/kollaps-wasm-runtime new-folder/bin/
mv Kollaps/kollaps/TCAL/libTCAL.so new-folder/bin/
```

## Usage
Running `kollaps-wasm` requires root privileges, as it needs to be able to use linux eBPF and `ip`.

The only required argument is the topology file.
To see all of the possible arguments, run `./kollaps-wasm -h`.

### Example
```
sudo ./kollaps-wasm path/to/topology.xml
```

The topology file format is similar to the orchestrated deployments, 
refer to `Kollaps/examples/kollaps-wasm/wasm/topology.xml` for examples and explanations.

Also refer to `Kollaps/examples/kollaps-wasm/native/topology.xml` if you would like to use your own runtime.

## Limitations
- Single-host deployments only.
