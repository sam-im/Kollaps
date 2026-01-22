# Kollaps Dashboard
## Installation and Running
### Installing
Required packages for Debian 12:
- `python3`
- `python3-dev`
- `python3-venv`

``` shell
# create a directory with a virtual environment in it
mkdir dashboard && cd dashboard
python3 -m venv venv
source venv/bin/activate

# install dependencies
pip install wheel dnspython flask docker kubernetes netifaces openssh_wrapper netaddr requests==2.31.0

# Dashboard depends on the communicationcore crate,
# see Kollaps/kollaps/README.md for building it,
# or download it from the releases page.
mv path/to/libcommunicationcore.so ../Kollaps/kollaps/dashboard/

pip wheel --no-deps -w . ../Kollaps/
pip install kollaps-2.0-py3-none-any.whl 
```

### Running
You can run the dashboard using:
``` shell
./dashboard/venv/bin/python3 -m kollaps.dashboard.Dashboard <deployment> path/to/topology.xml
```
where `<deployment>` is one of `wasm`, `container`, and `baremetal`.
