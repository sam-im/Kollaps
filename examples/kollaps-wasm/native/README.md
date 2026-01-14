# Running kollaps-wasm experiments with native programs
The topology file imitates the iperf3 example of swarm/docker orchestrator deployments.

It requires an `iperf3` executable to be available in PATH.

## Preparation
See `Kollaps/kollaps/kollaps-wasm/README.md` and prepare a folder with the required build artifacts.

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
