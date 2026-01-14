# EmulationCore
## Usage
```
$ ./emulationcore -h
Usage: emulationcore [OPTIONS] <TOPOLOGY> <COMMAND>

Commands:
  baremetal
  docker
  kubernetes
  wasm
  help        Print this message or the help of the given subcommand(s)

Arguments:
  <TOPOLOGY>  Specify a path to a topology file

Options:
  -i, --ifname <IFNAME>  Set the interface name to use. Defaults to "eth0"
  -h, --help             Print help
  -V, --version          Print version
```

Each deployment (baremetal, docker, kubernetes, wasm) may require further arguments,  to see them run `./emulationcore help <deployment-name>`.
