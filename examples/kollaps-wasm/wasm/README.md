# Running kollaps-wasm experiments with WASM modules
This topology file imitates the iperf3 example for swarm/docker orchestrator deployments.

## Preparation
Read `../kollaps/kollaps-wasm/README.md` and prepare a folder with the required build artifacts.

You might need to update the image tag's of services in topology.xml to make them point to the appropriate WASM modules.
You can use the already built WASM modules that exists within this directory (simple-client.wasm and simple-server.wasm),
or if you want to build them yourself, see their respective READMEs.

