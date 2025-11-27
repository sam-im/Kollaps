#! /bin/sh

#CHANGE THIS ACCORDING TO THE NAME OF THE NETWORK DEVICE
networkdevice="eth0"

# TODO: If you update the binaries inside the baremetal directory at any point,
# you can delete the following as emulationcore checks ./bin/libTCAL.so.
sudo cp bin/libTCAL.so /usr/local/bin/libTCAL.so

sudo ./emulationcore $1 communicationmanager $networkdevice baremetal
