# kollaps-wasm
Run single host Kollaps experiments.

## Building
See `../README.md`.

## Installation
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
mkdir --parents new-folder/bin/
# after building
mv Kollaps/kollaps/release/kollaps-wasm new-folder/
mv Kollaps/kollaps/release/communicationmanager new-folder/bin/
mv Kollaps/kollaps/release/emulationcore new-folder/bin/
mv Kollaps/kollaps/release/kollaps-wasm-runtime new-folder/bin/
mv Kollaps/kollaps/TCAL/libTCAL.so new-folder/bin/
```

## Usage
Run `kollaps-wasm` as root.

You will need to provide your topology file as the first argument. 
To see all of the possible arguments, run `./kollaps-wasm -h`.

### Example
```
sudo ./kollaps-wasm /path/to/topology.xml
```
