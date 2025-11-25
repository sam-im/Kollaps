// Licensed to the Apache Software Foundation (ASF) under one or more
// contributor license agreements.  See the NOTICE file distributed with
// this work for additional information regarding copyright ownership.
// The ASF licenses this file to You under the Apache License, Version 2.0
// (the "License"); you may not use this file except in compliance with
// the License.  You may obtain a copy of the License at

//    http://www.apache.org/licenses/LICENSE-2.0

// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
mod aux;
mod select;

use crate::aux::{Service, SetupMessage, get_services, wait_for_file};
use crate::select::Select;

use std::borrow::{Borrow, BorrowMut};
use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io::BufReader;
use std::io::prelude::*;
use std::net::{Ipv4Addr, Shutdown, SocketAddrV4, TcpListener, TcpStream};
use std::os::unix::io::AsRawFd;
use std::str::FromStr;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use capnp::message::{Builder, ReaderOptions};
use capnp::serialize_packed;
use capnp_schemas::message_capnp::message::Reader;
use tracing::{Level, debug, error, info};
use tracing_subscriber::FmtSubscriber;

const REMOTE_IPS_PATH: &str = "/tmp/kollaps/remote_ips.txt";
const LOCAL_IDS_PATH: &str = "/tmp/kollaps/topoinfo";
const DASHBOARD_ID_PATH: &str = "/tmp/kollaps/topoinfodashboard";
const READ_PIPES_PATH: &str = "/tmp/kollaps/pipes/piperead";
const WRITE_PIPES_PATH: &str = "/tmp/kollaps/pipes/pipewrite";
const SETUP_PORT: u16 = 8080;
const EXCHANGE_PORT: u16 = 8081;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn main() -> Result<()> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::DEBUG)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;
    info!("Starting.");

    let total_service_count = env::args()
        .nth(1)
        .expect("Missing positional argument: total service count in topology.")
        .parse::<usize>()
        .expect("Failed to parse total service count.");

    let listen_addr = env::args().nth(2).unwrap_or("0.0.0.0".to_string());
    let listen_addr =
        Ipv4Addr::from_str(&listen_addr).expect("Failed to parse argument: listen address.");

    let remote_ips = wait_for_file(REMOTE_IPS_PATH, None, true)?
        .unwrap()
        .iter()
        .map(|ip| Ipv4Addr::from_bits(ip.parse::<u32>().unwrap()))
        .collect::<Vec<Ipv4Addr>>();
    debug!("Remote addresses are: {:?}", remote_ips);

    let remote_sockets = remote_ips
        .iter()
        .map(|ip| SocketAddrV4::new(*ip, SETUP_PORT))
        .collect::<Vec<SocketAddrV4>>();

    setup(listen_addr, remote_sockets, total_service_count)?;

    let mut local_ids = wait_for_file(LOCAL_IDS_PATH, None, true)?.unwrap();
    let dashboard_ids = wait_for_file(DASHBOARD_ID_PATH, None, true)?.unwrap();

    debug!(
        "Local service IDs are: {:?}, dashboard ID is: {:?}",
        local_ids, dashboard_ids
    );

    dashboard_ids
        .iter()
        .for_each(|id| local_ids.push(id.to_owned()));
    let has_dashboard = !dashboard_ids.is_empty();

    // Wait for each local service's emulationcore instance to create their read/write pipes.
    for id in &local_ids {
        wait_for_file(format!("{}{}", READ_PIPES_PATH, id), None, false)?;
        wait_for_file(format!("{}{}", WRITE_PIPES_PATH, id), None, false)?;
    }

    let services = get_services(&local_ids, READ_PIPES_PATH)?;

    let self_socket = SocketAddrV4::new(listen_addr, EXCHANGE_PORT);
    let remote_sockets = remote_ips
        .iter()
        .map(|ip| SocketAddrV4::new(*ip, EXCHANGE_PORT))
        .collect::<Vec<SocketAddrV4>>();

    info!("Starting metadata exchange.");
    start_remote_producers(self_socket, remote_sockets.clone(), services.clone());
    start_message_exchange(local_ids, remote_sockets, has_dashboard, services)?;

    Ok(())
}

/**********************************************************************************************
*       setup
**********************************************************************************************/

// exchanges information with other machines to assure all machines started their containers
fn setup(
    addr: Ipv4Addr,
    remote_sockets: Vec<SocketAddrV4>,
    total_service_count: usize,
) -> Result<()> {
    info!(
        "Starting setup with a total service count of {}.",
        total_service_count
    );

    let mut senders = vec![];
    let mut receivers = vec![];

    for _ in remote_sockets.iter() {
        let (tx, rx) = channel::<u16>();
        senders.push(tx);
        receivers.push(rx);
    }

    if !remote_sockets.is_empty() {
        let len = remote_sockets.len();
        thread::spawn(move || setup_accept_loop(addr, senders, len));
    }

    let sleeptime = Duration::from_millis(1000);

    // connect to other machines
    let mut streams = vec![];
    let mut ips_connected = vec![];
    while ips_connected.len() != remote_sockets.len() {
        thread::sleep(sleeptime);

        for (i, remote_ip) in remote_sockets.iter().enumerate() {
            if !(ips_connected.contains(&i)) {
                let stream = TcpStream::connect(remote_ip);
                match stream {
                    Ok(stream) => {
                        debug!("Connected to remote host at {}", remote_ip);
                        streams.push(stream);
                        ips_connected.push(i);
                    }
                    Err(e) => error!("Failed to connect to {} with error: {}", remote_ip, e),
                };
            }
        }
    }

    let _ = wait_for_services(streams, total_service_count, receivers).map_err(|e| error!("{}", e));

    info!("Setup ended.");
    Ok(())
}

// exchange number of containers started with other machines
fn wait_for_services(
    streams: Vec<TcpStream>,
    total_service_count: usize,
    receivers: Vec<Receiver<u16>>,
) -> Result<()> {
    info!("Waiting for all services to start.");

    loop {
        let local_ids = wait_for_file(LOCAL_IDS_PATH, None, true)?.unwrap();
        let dashboard_ids = wait_for_file(DASHBOARD_ID_PATH, None, true)?.unwrap();

        let local_service_count = local_ids.len() + dashboard_ids.len();

        let msg = SetupMessage::ServiceCount(local_service_count as u16);
        let bytes: [u8; 3] = msg.into();

        for mut stream in &streams {
            let peer_addr = stream.peer_addr()?;

            match stream.write_all(&bytes) {
                Ok(_) => {
                    let _ = stream.flush();
                    debug!(
                        "Sent service count ({}) to {}",
                        local_service_count, peer_addr
                    );
                }
                Err(e) => {
                    error!(
                        "Failed to send service count to {} with error: {}",
                        peer_addr, e
                    );
                }
            };
        }

        // receives from other threads(other machines)
        let mut remote_services = 0;
        for rx in receivers.iter() {
            if let Ok(n) = rx.recv() {
                remote_services += n as usize;
            }
        }

        // check if all machines started if yes, send message to end the setup
        if remote_services + local_service_count == total_service_count {
            let msg = SetupMessage::Terminate;
            let bytes: [u8; 3] = msg.into();
            for mut stream in &streams {
                match stream.write_all(&bytes) {
                    Ok(_) => (),
                    Err(e) => error!("Socket error: {e}"),
                }
            }
            break;
        }

        thread::sleep(Duration::from_secs(1));
    }

    for stream in &streams {
        let _ = stream.shutdown(Shutdown::Both);
    }

    info!("All services have started");
    Ok(())
}

fn setup_accept_loop(
    addr: Ipv4Addr,
    senders: Vec<Sender<u16>>,
    number_of_remotes: usize,
) -> Result<()> {
    let socket_addr = SocketAddrV4::new(addr, 8080);
    let listener = TcpListener::bind(socket_addr)?;

    let mut remotecount = 0;
    let mut thread_handles = vec![];

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let peer_addr = stream.peer_addr()?;
                debug!("Accepted connection from {}", peer_addr);

                let tx = senders[remotecount].clone();

                let join_handle = thread::spawn(move || recv_service_count(stream, tx));
                thread_handles.push(join_handle);

                remotecount += 1;

                if remotecount == number_of_remotes {
                    break;
                }
            }
            Err(e) => {
                error!("{}", e);
            }
        };
    }

    for handle in thread_handles {
        let _ = handle.join();
    }

    Ok(())
}

// reads from the other hosts how many containers they started
fn recv_service_count(mut stream: TcpStream, tx: Sender<u16>) {
    let mut buffer = [0; 3];
    loop {
        match stream.read_exact(&mut buffer) {
            Ok(_) => {
                let msg = SetupMessage::from(buffer);
                let n = match msg {
                    SetupMessage::ServiceCount(n) => n,
                    SetupMessage::Terminate => break,
                };
                if let Ok(peer) = stream.peer_addr() {
                    debug!(
                        "Received remote service count of {n} from remove host {}",
                        peer
                    );
                }
                match tx.send(n) {
                    Ok(_) => (),
                    Err(_) => break, // rx is dropped
                }
            }
            Err(e) => {
                error!("Socket error: {}", e);
                break;
            }
        }
    }
}

/**********************************************************************************************
*       start of exchange mechanisms
**********************************************************************************************/

// starts the exchange of  metadata
fn start_remote_producers(
    self_addr: SocketAddrV4,
    remote_ips: Vec<SocketAddrV4>,
    services: Vec<Arc<Mutex<Service>>>,
) {
    // Start accept loop that will start threads to receive metadata from other hosts
    if !remote_ips.is_empty() {
        thread::spawn(move || accept_loop(remote_ips.len(), self_addr, services));
    }
    thread::sleep(Duration::from_millis(1000));
}

// accepts connections from other hosts
fn accept_loop(count: usize, addr: SocketAddrV4, services: Vec<Arc<Mutex<Service>>>) -> Result<()> {
    let listener = TcpListener::bind(addr).unwrap();

    let mut peers = 0;
    for stream in listener.incoming() {
        let stream = stream?;

        let services = services.clone();
        thread::spawn(move || remote_producer(stream, services));

        peers += 1;
        if peers == count {
            break;
        }
    }
    info!("Established {} connections to remote hosts.", count);
    Ok(())
}

// receives metadata from other hosts
fn remote_producer(stream: TcpStream, services: Vec<Arc<Mutex<Service>>>) {
    let mut buf_reader = BufReader::new(stream);

    loop {
        let message_reader =
            serialize_packed::read_message(buf_reader.borrow_mut(), ReaderOptions::new()).unwrap();

        let mut message_to_send = Builder::new_default();

        message_to_send
            .set_root(message_reader.get_root::<Reader>().unwrap())
            .expect("Failed to set root");
        services
            .iter()
            .for_each(|pipe| pipe.lock().unwrap().write(&message_to_send));
    }
}

/**********************************************************************************************
*       start of information exchange
**********************************************************************************************/

fn start_message_exchange(
    local_ids: Vec<String>,
    remote_sockets: Vec<SocketAddrV4>,
    dashboard: bool,
    services: Vec<Arc<Mutex<Service>>>,
) -> Result<()> {
    // hold pipes to read from
    let mut read_pipes = vec![];

    // position of pipe in vector (key-> fd_raw, value-> position)
    let mut pipes_position = HashMap::new();

    // hold pipes to write to (we want to write to information every container but ourselfs so we create a vector for each container that containers every pipe but the pipe to themselfs)
    let mut vector_pipes_id = vec![];

    for (i, id) in local_ids.iter().enumerate() {
        // open pipe
        let path = format!("{}{}", WRITE_PIPES_PATH, id);
        let file = File::open(path).unwrap();
        let buf_reader = BufReader::new(file);

        // insert in hashmap
        pipes_position.insert(buf_reader.get_ref().as_raw_fd(), i);
        // insert in vector
        read_pipes.push(buf_reader);

        // create a list of containers excluding the one with `id`
        let pipes: Vec<Arc<Mutex<Service>>> = services
            .iter()
            .filter(|c| c.lock().unwrap().id.ne(id))
            .cloned()
            .collect();

        vector_pipes_id.push(pipes);
    }

    // if we are the host with the dashboard remove it because the dashboard does not send information
    if dashboard {
        read_pipes.remove(read_pipes.len() - 1); // If dashboard cease to be the last element in read_pipes, this will cause bugs
    }

    // hold references to remote hosts
    let mut streams = vec![];
    let mut ips_connected = vec![];

    let sleeptime = Duration::from_millis(1000);
    // open connections
    while ips_connected.len() != remote_sockets.len() {
        for (i, remote_ip) in remote_sockets.iter().enumerate() {
            if !(ips_connected.contains(&i)) {
                let stream = TcpStream::connect(remote_ip);
                match stream {
                    Ok(stream) => {
                        streams.push(stream);
                        ips_connected.push(i);
                    }
                    Err(e) => println!("{}", e),
                };
            }
        }
        thread::sleep(sleeptime);
    }

    let fds = read_pipes
        .iter()
        .map(|pipe| pipe.get_ref().as_raw_fd())
        .collect();

    let mut select = Select::new(fds);

    loop {
        let read_ready_fds = select.select();
        read_ready_fds.iter().for_each(|fd| {
            let position = pipes_position.get(fd).unwrap();
            let buf_reader = read_pipes.get_mut(*position).unwrap();

            let msg_reader =
                serialize_packed::read_message(buf_reader, ReaderOptions::new()).unwrap();
            let mut msg = Builder::new_default();
            msg.set_root(msg_reader.get_root::<Reader>().unwrap())
                .unwrap();

            // send message/metadata to other local services
            let other_services = &vector_pipes_id[*position];
            for pipe in other_services {
                pipe.lock().unwrap().write(msg.borrow());
            }
            // send metadata to other remote hosts
            for mut stream in &streams {
                serialize_packed::write_message(stream, msg.borrow()).unwrap();
                stream.flush().expect("Failed to flush to stream");
            }
        });
    }
}
