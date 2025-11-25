use crate::aux::{get_own_ip, print_message};
use crate::communication::Communication;
use crate::eventscheduler::EventScheduler;
use crate::orchestrator::{Orchestrator, SignalCode};
use crate::state::State;
use crate::xmlgraphparser::XMLGraphParser;

use monitor;
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::Arc;
use std::time::{Duration, Instant};
use subprocess::Popen;
use subprocess::PopenConfig;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

const TOPOLOGY_PATH: &str = "topology.xml";

pub struct EmulationCore {
    id: String,
    ip: u32,
    name: String,
    state: Arc<Mutex<State>>,
    pid: u32,
    comms: Communication,
    lasttime: Option<Instant>,
    usages: Arc<Mutex<HashMap<u32, u32>>>,
    link_count: u32,
    orchestrator: Option<Orchestrator>,
    cm_file: String,
    topology_file: String,
    networkdevice: Option<String>,
    shutdown: Arc<Mutex<bool>>,
    start: Arc<Mutex<bool>>,
    scheduler: Arc<Mutex<EventScheduler>>,
    shortest_path_type: String,
    pool_period: f32,
    max_age: u32,
}

impl EmulationCore {
    pub fn new(
        id: String,
        pid: u32,
        orchestrator: Option<Orchestrator>,
        networkdevice: Option<String>,
    ) -> Self {
        let state = Arc::new(Mutex::new(State::new(id.clone())));
        let eventscheduler = Arc::new(Mutex::new(EventScheduler::new(
            state.clone(),
            orchestrator,
            pid,
        )));
        let communication = Communication::new(id.clone());

        // TODO consider replacing `"".to_string()` fields with Option::None instead
        Self {
            id: id,
            ip: 0,
            name: "".to_string(),
            state,
            pid: pid,
            comms: communication,
            lasttime: None,
            usages: Arc::new(Mutex::new(HashMap::new())),
            pool_period: 0.05,
            link_count: 0,
            orchestrator,
            cm_file: "".to_string(),
            topology_file: "".to_string(),
            networkdevice: networkdevice,
            shutdown: Arc::new(Mutex::new(false)),
            scheduler: eventscheduler,
            start: Arc::new(Mutex::new(false)),
            shortest_path_type: "hop".to_string(),
            max_age: 2,
        }
    }

    pub fn set_cm_file(&mut self, cm_file: String) {
        self.cm_file = cm_file;
    }

    pub fn set_topology_file(&mut self, topology_file: String) {
        self.topology_file = topology_file;
    }

    pub fn set_network_device(&mut self, networkdevice: String) {
        self.networkdevice = Some(networkdevice);
    }

    pub async fn init_baremetal(&mut self) {
        info!("EC {}: started boostrapping EC", self.name);
        self.state.lock().await.name = self.name.clone();

        let text = std::fs::read_to_string(self.topology_file.clone()).unwrap();
        let parser = XMLGraphParser::try_new(&text, "baremetal".to_string())
            .expect("topology file must be valid xml");

        let (config, mut initial_graph) = parser.fill_graph().await;

        self.shortest_path_type = config.shortest_path_type.to_string();
        self.pool_period = config.pool_period;
        self.max_age = config.max_age;

        let self_addr = get_own_ip(self.networkdevice.clone());
        initial_graph
            .set_graph_root(self_addr)
            .await
            .expect("failed to set graph root");

        initial_graph
            .recompute_properties(&self.shortest_path_type)
            .await;
        self.name = initial_graph.get_name().await;
        self.scheduler.lock().await.name = self.name.clone();
        self.state.lock().await.name = self.name.clone();

        let (graphs, events) = parser.parse_schedule(initial_graph, &config).await;

        // Collect parsed graphs and events
        self.state.lock().await.graphs = graphs
            .iter()
            .map(|g| Arc::new(Mutex::new(g.clone())))
            .collect();
        self.state.lock().await.graph_counter = graphs.len();
        self.scheduler.lock().await.events = events;

        let service_count = self
            .state
            .lock()
            .await
            .get_current_graph()
            .lock()
            .await
            .services
            .keys()
            .len();

        // Get own ip with the provided network device
        self.ip = self_addr;

        // id is the same as the ip in baremetal
        self.id = self.ip.to_string();

        // Start the CM process
        let process = self.start_cm(service_count).await;

        self.scheduler.lock().await.pid = self.pid; // self.start_cm modifies the pid

        self.scheduler.lock().await.script = self
            .state
            .lock()
            .await
            .get_current_graph()
            .lock()
            .await
            .graph_root
            .as_ref()
            .unwrap()
            .lock()
            .await
            .script
            .clone();

        // Get how many links we have in the experiment if >255 use u16 else u8
        let removed_links_len = self
            .state
            .lock()
            .await
            .get_current_graph()
            .lock()
            .await
            .removed_links
            .len();
        self.link_count = (self
            .state
            .lock()
            .await
            .get_current_graph()
            .lock()
            .await
            .links
            .keys()
            .len()
            + removed_links_len) as u32;
        self.state
            .lock()
            .await
            .set_link_count(self.link_count)
            .await;

        print_message(self.name.clone(), "STARTING TC".to_string());
        self.state
            .lock()
            .await
            .init(self.ip)
            .await
            .map_err(|err| println!("{:?}", err))
            .ok();

        self.comms.init(self.state.clone()).await;

        // create variables to send to thread
        let scheduler = self.scheduler.clone();
        let shutdown = self.shutdown.clone();

        tokio::spawn(async move {
            accept_loop_baremetal(scheduler, shutdown, Arc::new(Mutex::new(process))).await
        });
    }

    pub async fn init(&mut self) {
        // Parse the topology
        let text = std::fs::read_to_string(TOPOLOGY_PATH).unwrap();
        let parser = XMLGraphParser::try_new(&text, "container".to_string())
            .expect("topology must be a valid xml file");
        let (config, mut initial_graph) = parser.fill_graph().await;

        self.shortest_path_type = config.shortest_path_type.clone();
        self.pool_period = config.pool_period;
        self.max_age = config.max_age;

        // Get ips of all containers
        tokio::time::sleep(Duration::from_secs(2)).await;
        if let Some(o) = &self.orchestrator {
            let res = o.resolve_hostnames(&mut initial_graph).await;
            debug!(
                "resolved {} addresses, returned result was {:?}",
                initial_graph.services.keys().len(),
                res
            );
        }

        let self_addr = get_own_ip(self.networkdevice.clone());
        initial_graph
            .set_graph_root(self_addr)
            .await
            .expect("failed to set graph root");

        initial_graph
            .recompute_properties(&self.shortest_path_type)
            .await;
        self.name = initial_graph.get_name().await;
        self.state.lock().await.name = self.name.clone();
        self.scheduler.lock().await.name = self.name.clone();

        let (graphs, events) = parser.parse_schedule(initial_graph, &config).await;

        // Collect parsed graph and properties
        self.state.lock().await.graphs = graphs
            .iter()
            .map(|g| Arc::new(Mutex::new(g.clone())))
            .collect();
        self.state.lock().await.graph_counter = graphs.len();
        self.scheduler.lock().await.events = events;

        self.state.lock().await.shrink_maps().await;

        // Get how many links we have in the experiment if >255 use u16 else u8
        let removed_links_len = self
            .state
            .lock()
            .await
            .get_current_graph()
            .lock()
            .await
            .removed_links
            .len();
        let links_len = self
            .state
            .lock()
            .await
            .get_current_graph()
            .lock()
            .await
            .links
            .keys()
            .len();
        self.link_count = (links_len + removed_links_len) as u32;

        // Start communication
        self.comms.init(self.state.clone()).await;

        self.state
            .lock()
            .await
            .set_link_count(self.link_count)
            .await;

        self.ip = get_own_ip(None);

        // Start TC structures
        self.state
            .lock()
            .await
            .init(self.ip)
            .await
            .map_err(|e| error!("{:?}", e))
            .ok();

        let pid = self.pid.clone();
        let scheduler = self.scheduler.clone();
        let start = self.start.clone();
        let shutdown = self.shutdown.clone();
        let orchestrator = self.orchestrator.clone();

        tokio::spawn(
            async move { accept_loop(scheduler, orchestrator, pid, start, shutdown).await },
        );

        info!(
            self.pool_period,
            self.max_age,
            self.shortest_path_type,
            "EC {} with ID {} is now online",
            self.name,
            self.id
        );
    }

    pub async fn start_cm(&mut self, service_count: usize) -> Popen {
        // Create auxiliary files, CM reads from these files, dashboard is not relevant we just create an empty file
        OpenOptions::new()
            .write(true)
            .create(true)
            .append(true)
            .open("/tmp/topoinfodashboard")
            .unwrap();

        let mut topoinfo = OpenOptions::new()
            .write(true)
            .create(true)
            .append(true)
            .open("/tmp/topoinfo")
            .unwrap();

        let mut remote_ips = OpenOptions::new()
            .write(true)
            .create(true)
            .append(true)
            .open("/remote_ips.txt")
            .unwrap();

        for (ip, _) in self
            .state
            .lock()
            .await
            .get_current_graph()
            .lock()
            .await
            .services
            .iter()
        {
            if ip != &self.ip {
                let string = format!("{}\n", ip);
                remote_ips.write_all(string.to_string().as_bytes()).unwrap();
            }
        }

        topoinfo
            .write_all(self.ip.to_string().as_bytes())
            .map_err(|err| println!("{:?}", err))
            .ok();
        let mut string_command = vec![];

        string_command.push(format!("./{}", self.cm_file.clone().to_string()));
        string_command.push(service_count.to_string());
        string_command.push("0.0.0.0".to_string());

        let process = Popen::create(&string_command, PopenConfig::default()).unwrap();
        self.pid = process.pid().unwrap();

        info!(self.pid, "EC {}: got PID", self.name);

        return process;
    }

    pub async fn setup_ebpf(&mut self) {
        let iface = match self.networkdevice.clone() {
            Some(s) => s,
            None => "eth0".to_string(),
        };
        let usages = self.usages.clone();
        tokio::spawn(async move { get_local_usage(&iface, usages).await });
    }

    pub async fn emulation_loop(&mut self) {
        self.setup_ebpf().await;

        self.lasttime = Some(Instant::now());
        self.check_active_flows().await;

        loop {
            if *self.shutdown.lock().await {
                info!("EC {} Shutdown", self.name);
                break;
            }
            if !*self.start.lock().await {
                tokio::time::sleep(Duration::from_secs_f32(0.0001)).await;
                continue;
            }

            let sleeptime =
                self.pool_period - (self.lasttime.unwrap().elapsed().as_millis() as f32) / 1000.0;

            if sleeptime > 0.0 {
                tokio::time::sleep(Duration::from_secs_f32(sleeptime)).await;
            }

            self.state.lock().await.clear_paths();

            self.check_active_flows().await;

            self.broadcast_flows().await;

            self.state.lock().await.calculate_bandwidth().await;
        }
    }

    // Read from local usages map
    async fn check_active_flows(&mut self) {
        let usages = self.usages.lock().await.clone();

        for (ip, bytes) in usages.iter() {
            let mut state = self.state.lock().await;
            let current_graph_handle = state.get_current_graph();
            let mut current_graph = current_graph_handle.lock().await;

            let last_bytes = current_graph.get_lastbytes(&ip).await;
            current_graph.set_lastbytes(&ip, *bytes).await;

            let delta_bytes;
            if last_bytes > *bytes {
                delta_bytes = *bytes;
            } else {
                delta_bytes = *bytes - last_bytes;
            }

            //print_message(self.name.clone(),format!("delta bytes is {}",delta_bytes.clone()));
            let delta_time = self.lasttime.unwrap().elapsed().as_millis() as f32;

            //let bits:u128 = (delta_bytes * 8).into();
            let bits = delta_bytes * 8;
            let throughput = bits as f32 / (delta_time / 1000.0);

            let useful = current_graph.process_usage(*ip, throughput).await;

            // insert_active_path_id(...) below also requires a lock on current_graph
            drop(current_graph);

            if useful {
                state.insert_active_path_id(*ip).await;
                //print_message(self.name.clone(),format!("throughput is {}",throughput.clone()));
            }
        }
        self.lasttime = Some(Instant::now());
    }

    // Sends metadata to CM
    async fn broadcast_flows(&mut self) {
        use crate::communication::PathFlowData;

        let active_paths = self.state.lock().await.get_active_paths().await.clone();

        if !(active_paths.is_empty()) {
            let ec_cycle = self.state.lock().await.ec_cycle as u32;
            let mut flows = Vec::new();
            for p in active_paths {
                let path = p.lock().await;
                let bandwidth = path.used_bandwidth as u32;
                let links = path.links.clone();
                flows.push(PathFlowData { bandwidth, links });
            }
            self.comms.send_flows(ec_cycle, flows).await;
        }
    }
}

/// Inserts received message data from monitor's eBPF PerfEventMap into
/// `usages` hashmap.
async fn get_local_usage(iface: &str, usages_handle: Arc<Mutex<HashMap<u32, u32>>>) {
    let mut ebpf_handle = monitor::run(iface).await.unwrap();

    while let Some(msg) = ebpf_handle.rx.recv().await {
        usages_handle.lock().await.insert(msg.dst, msg.bytes);
    }
}

// Waits to accept dashboard connection
async fn accept_loop(
    eventscheduler: Arc<Mutex<EventScheduler>>,
    orchestrator: Option<Orchestrator>,
    pid: u32,
    start: Arc<Mutex<bool>>,
    shutdown: Arc<Mutex<bool>>,
) -> Result<()> {
    info!("EC with pid {}: starting accept loop", pid);

    let addr = format!("0.0.0.0:{}", 7073);
    let listener = tokio::net::TcpListener::bind(addr).await?;

    loop {
        let (stream, _) = listener.accept().await?;
        let _ = receive_commands(
            stream,
            eventscheduler.clone(),
            orchestrator,
            pid,
            start.clone(),
            shutdown.clone(),
        )
        .await
        .map_err(|e| warn!("error while receiving command: {:?}", e));
    }
}

// Receives commands from dashboard
async fn receive_commands(
    mut stream: tokio::net::TcpStream,
    eventscheduler: Arc<tokio::sync::Mutex<EventScheduler>>,
    orchestrator: Option<Orchestrator>,
    pid: u32,
    start: Arc<Mutex<bool>>,
    shutdown: Arc<Mutex<bool>>,
) -> Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const SHUTDOWN_COMMAND: u8 = 2;
    const READY_COMMAND: u8 = 3;
    const START_COMMAND: u8 = 4;
    const ACK: u8 = 120;

    let mut buf = [0; 1];

    loop {
        match stream.read_exact(&mut buf).await {
            Ok(_) => match buf.first() {
                // TODO consider calling teardown() on tcal_client
                Some(cmd) if SHUTDOWN_COMMAND.eq(cmd) => {
                    *shutdown.lock().await = true;
                    orchestrator
                        .unwrap()
                        .stop_experiment(pid, SignalCode::SigInt)
                        .await;
                }
                Some(cmd) if READY_COMMAND.eq(cmd) => {
                    let _ = stream.write_all(&[ACK]).await.map_err(|e| warn!("{:?}", e));
                }
                Some(cmd) if START_COMMAND.eq(cmd) => {
                    let es = eventscheduler.clone();
                    tokio::spawn(async move { es.lock().await.start().await });
                    *start.lock().await = true;
                }
                Some(bytes) => warn!("Unknown bytes from stream: {:?}", bytes),
                None => warn!("Read empty buffer from stream"),
            },
            Err(e) => {
                warn!("error while reading from stream: {:?}", e);
                break;
            }
        }
    }
    Ok(())
}

// Waits to accept dashboard connection
async fn accept_loop_baremetal(
    eventscheduler: Arc<Mutex<EventScheduler>>,
    shutdown: Arc<Mutex<bool>>,
    cm_process: Arc<Mutex<Popen>>,
) -> Result<()> {
    let port: u32 = 7073;
    let listener =
        tokio::net::TcpListener::bind(format!("{}:{}", "0.0.0.0", port.to_string())).await?;
    loop {
        let (stream, _) = listener.accept().await?;
        let _ = receive_commands_baremetal(
            stream,
            eventscheduler.clone(),
            shutdown.clone(),
            cm_process.clone(),
        )
        .await;
    }
}

// Receives commands from dashboard
async fn receive_commands_baremetal(
    mut stream: tokio::net::TcpStream,
    eventscheduler: Arc<Mutex<EventScheduler>>,
    shutdown: Arc<Mutex<bool>>,
    cm_process: Arc<Mutex<Popen>>,
) -> Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const SHUTDOWN_COMMAND: u8 = 2;
    const READY_COMMAND: u8 = 3;
    const START_COMMAND: u8 = 4;
    const ACK: u8 = 120;

    let mut buf = [0; 1];

    match stream.read_exact(&mut buf).await {
        Ok(_) => match buf.first() {
            Some(cmd) if SHUTDOWN_COMMAND.eq(cmd) => {
                let _ = cm_process.lock().await.kill();
                info!("Sent SIGKILL to CM process");
                eventscheduler
                    .lock()
                    .await
                    .state
                    .lock()
                    .await
                    .tcal_client
                    .teardown()
                    .await;
                info!("Called teardown() on TCAL");
                *shutdown.lock().await = true;
                info!("Shutdown flag enabled");
            }
            Some(cmd) if READY_COMMAND.eq(cmd) => {
                let _ = stream
                    .write_all(&[ACK])
                    .await
                    .map_err(|e| error!("error while writing to stream: {:?}", e));
            }
            Some(cmd) if START_COMMAND.eq(cmd) => {
                tokio::spawn(async move { eventscheduler.lock().await.start().await });
            }
            Some(byte) => warn!("unkown command byte: {:?}", byte),
            None => warn!("read empty buffer from stream"),
        },
        Err(e) => warn!("error while reading stream: {:?}", e),
    }

    Ok(())
}
