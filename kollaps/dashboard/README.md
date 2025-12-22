# Kollaps Dashboard
## Installation and Running
### Using a python virtual environment
#### Installing
Required packages for Debian 12:
- `python3`
- `python3-dev`
- `python3-venv`

``` shell
# create a directory with a virtual environment in it
mkdir dashboard
cd dashboard
python3 -m venv ./venv
source venv/bin/activate

# install dependencies
pip install dnspython flask docker kubernetes netifaces openssh_wrapper netaddr requests==2.31.0

# dashboard depends on communicationcore crate
# see Kollaps/kollaps/README.md for building communicationcore
mv path/to/libcommunicationcore.so . 

pip wheel --no-deps -w ../Kollaps/ ../Kollaps/
pip install kollaps-2.0-py3-none-any.whl 
```

#### Running
You can run the dashboard using:
``` shell
./dashboard/venv/bin/python3 -m kollaps.dashboard.Dashboard <deployment> topology.xml
```
where `<deployment>` is one of `wasm`, `container`, and `baremetal`.
