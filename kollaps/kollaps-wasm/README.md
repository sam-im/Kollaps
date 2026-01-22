# kollaps-wasm
Run single host Kollaps experiments.

## Building
See `../README.md`.

## Installation
Kollaps requires some of the binaries built on the previous step to be available in a directory called `bin` relative to itself.

Specifically the required binaries are:
- `communicationmanager`: required by kollaps
- `emulationcore`: required by kollaps
- `kollaps-wasm-runtime`: required by kollaps-wasm for using the default runtime
- `libTCAL.so`: required by emulationcore

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
To see all of the possible arguments, run `./kollaps-wasm -h`:
``` 
$ ./kollaps-wasm -h
Usage: kollaps-wasm [OPTIONS] <TOPOLOGY>

Arguments:
  <TOPOLOGY>  Specifies a path to a topology file

Options:
      --addr <ADDR>            Sets a custom address in CIDR notation. Defaults to 10.10.10.0
      --subnet <SUBNET>        Sets a custom subnet mask in CIDR notation. Defaults to 24
      --allow-dir <ALLOW_DIR>  Allow the specified directory to be accessible within the default runtime's working directory
  -v, --verbose                Increase verbosity of logs
  -h, --help                   Print help
  -V, --version                Print version

```

### Example
```
sudo ./kollaps-wasm path/to/topology.xml
```

The topology file format is similar to the orchestrated deployments, 
refer to `Kollaps/examples/kollaps-wasm/wasm/topology.xml` for an example and explanations.

Also refer to `Kollaps/examples/kollaps-wasm/native/topology.xml` if you would like to use your own WASI runtimes.

## Limitations
- Single-host deployments only.
