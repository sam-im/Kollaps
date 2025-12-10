#! /bin/sh

#CHANGE THIS ACCORDING TO THE NAME OF THE NETWORK DEVICE
networkdevice="eth0"

# TODO: If the emulationcore binary is updated, remove the following line:
sudo cp bin/libTCAL.so /usr/local/bin/libTCAL.so

# TODO: If the emulationcore binary is updated, update the line below with:
# sudo ./emulationcore $1 -i $networkdevice baremetal communicationmanager
sudo ./emulationcore $1 communicationmanager $networkdevice baremetal
