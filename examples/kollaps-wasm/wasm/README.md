# Running kollaps-wasm experiments with WASM modules
This topology file imitates the iperf3 example of swarm/docker orchestrator deployments.

You might need to update the image tag's of services in topology.xml to make them point to the appropriate WASM modules.

You can use the already built WASM modules that exists within this directory (simple-client.wasm and simple-server.wasm), 
or if you want to build them yourself, see their respective READMEs.

## Preparation
Read `Kollaps/kollaps/kollaps-wasm/README.md` and prepare a folder with the required build artifacts.

You will also require the dashboard to control the experiment, see its readme at `Kollaps/kollaps/dashboard/README.md`.

## Running
Start kollaps-wasm:
```
sudo ./kollaps-wasm topology.xml
```

Start dashboard:
```
./dashboard/venv/bin/python3 -m kollaps.dashboard.Dashboard wasm topology.xml
```

Open `127.0.0.1:8088` in your browser to access the dashboard to start/stop/view the experiment.
