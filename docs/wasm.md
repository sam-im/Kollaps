# WebAssembly Deployments

## Prerequisites
1. The Dashboard requires to following packages:
   - `python3`
   - `python3-dev`
   - `python3-venv`
   Installing them on Debian 12 is as follows:
   ```
   sudo apt install python3 python3-dev python3-venv
   ```

## Installation
1. Clone the repository:
```
git clone https://github.com/sam-im/Kollaps
```

2. Install Dashboard:
```
mkdir installation-dir && cd installation-dir
mkdir dashboard && cd dashboard

# create a python env
python3 -m venv venv
source venv/bin/activate

# install dependencencies
pip install wheel dnspython flask docker kubernetes netifaces openssh_wrapper netaddr requests==2.31.0

curl -L -O "https://github.com/sam-im/Kollaps/releases/download/2.1.0/kollaps-binaries.tar.gz"
tar -xzf kollaps-binaries.tar.gz
mv kollaps-binaries/libcommunicationcore.so ../../Kollaps/kollaps/dashboard/

pip wheel --no-deps -w . ../../Kollaps/
pip install kollaps-2.0-py3-none-any.whl 

rm -r kollaps-binaries kollaps-binaries.tar.gz kollaps-2.0-py3-none-any.whl
```

3. Install `kollaps-wasm`
```
# return to installation-dir
cd ..

curl -L -O "https://github.com/sam-im/Kollaps/releases/download/2.1.0/kollaps-wasm.tar.gz"
tar -xzf kollaps-wasm.tar.gz

rm kollaps-wasm.tar.gz
```

If everything went well, you should have the following folder:
```
installation-dir/
├── dashboard/
│   └── venv/...
└── kollaps-wasm/
    ├── bin/...
    └── kollaps-wasm
```

## Usage
1. Start `kollaps-wasm` with the example WebAssembly topology in one terminal:
```
sudo ./kollaps-wasm/kollaps-wasm ../Kollaps/examples/kollaps-wasm/wasm/topology.xml
```

2. When indicated by `kollaps-wasm`'s output, start the dashboard in a second shell:
```
./dashboard/venv/bin/python3 -m kollaps.dashboard.Dashboard wasm ../Kollaps/examples/kollaps-wasm/wasm/topology.xml 
```

3. Access the Dashboard from your browser at `127.0.0.1:8088`.
