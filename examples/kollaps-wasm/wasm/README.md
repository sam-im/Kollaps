# Running kollaps-wasm experiments with WASM modules
This topology file imitates the iperf3 example of swarm/docker orchestrator deployments.

You can use the already built WASM modules that exists within this directory (simple-client.wasm and simple-server.wasm), 
or if you want to build them yourself, see their respective READMEs.

## Prerequisites
Running this example requires `kollaps-wasm` and `dashboard`.
The file `Kollaps/docs/wasm.md` contains step-by-step instructions to install them both.

To build it yourself, see `Kollaps/kollaps/kollaps-wasm/README.md` and `Kollaps/kollaps/dashboard/README.md`.

## Running
Start kollaps-wasm:
```
sudo ./kollaps-wasm topology.xml
```

When indicated by `kollaps-wasm`'s output, start the dashboard in a second shell:
```
./dashboard/venv/bin/python3 -m kollaps.dashboard.Dashboard wasm topology.xml
```

Open `127.0.0.1:8088` in your browser to access the dashboard to start/stop/view the experiment.
