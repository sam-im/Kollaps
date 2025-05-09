use monitor;
use capnp_schemas::message_capnp;
use capnp::message::{Builder, HeapAllocator};

use std::sync::{Arc, Mutex};
use crate::xmlgraphparser::XMLGraphParser;
use crate::communication::Communication;
use crate::eventscheduler::EventScheduler;
use crate::state::State;
use std::time::{Duration, Instant};
use std::collections::HashMap;
use std::ptr;
use std::thread;
use std::net::{TcpStream,TcpListener};
use std::time;
use crate::aux::{print_message,get_own_ip};
use crate::docker::stop_experiment;
use kube::{Client, api::{Api, ResourceExt, ListParams}};
use k8s_openapi::api::core::v1::Pod;
use std::net::Ipv4Addr;
use crate::graph::Graph;
use crate::aux::convert_to_int;
use std::net::IpAddr;
use std::str::FromStr;
use std::env;
use subprocess::PopenConfig;
use subprocess::Popen;
use std::fs::OpenOptions;
use std::io::Write;
use std::io::Read;
use std::io;
use tokio::runtime;
use tokio::runtime::Handle;
use roxmltree::Document;


pub struct EmulationCore{
    id:String,
    ip:u32,
    name:String,
    state:Arc<Mutex<State>>,
    pid:u32,
    comms:Arc<Mutex<Communication>>,
    lasttime:Option<Instant>,
    usages:Arc<Mutex<HashMap<u32,u32>>>,
    link_count:u32,
    orchestrator:String,
    cm_file:String,
    topology_file:String,
    networkdevice:String,
    shutdown:Arc<Mutex<bool>>,
    start:Arc<Mutex<bool>>,
    scheduler:Arc<Mutex<EventScheduler>>,
    shortest_path_type:String,
    pool_period:f32,
    max_age:u32
}

impl EmulationCore{
    pub fn new(id:String,pid:u32,orchestrator:String) -> EmulationCore{
        let state = Arc::new(Mutex::new(State::new(id.clone())));
        let eventscheduler = Arc::new(Mutex::new(EventScheduler::new(state.clone(),orchestrator.clone())));
        let mut communication = Communication::new(id.clone());

        //If it is baremetal we can not start pipes here because they block waiting for CM
        if orchestrator != "baremetal"{
            communication.init();
        }
        EmulationCore{
            id:id.clone(),
            ip:0,
            name:"".to_string(),
            state:state,
            pid:pid,
            comms:Arc::new(Mutex::new(communication)),
            lasttime:None,
            usages:Arc::new(Mutex::new(HashMap::new())),
            pool_period:0.05,
            link_count:0,
            orchestrator:orchestrator,
            cm_file:"".to_string(),
            topology_file:"".to_string(),
            networkdevice:"".to_string(),
            shutdown:Arc::new(Mutex::new(false)),
            scheduler:eventscheduler,
            start:Arc::new(Mutex::new(false)),
            shortest_path_type:"hop".to_string(),
            max_age:2

        }
    }

    pub fn set_cm_file(&mut self, cm_file:String){
        self.cm_file = cm_file;
    }
    
    pub fn set_topology_file(&mut self, topology_file:String){
        self.topology_file = topology_file;
    } 
    
    pub fn set_network_device(&mut self, networkdevice:String){
        self.networkdevice = networkdevice;
    }
    
    pub fn init_baremetal(&mut self){
        
        print_message(self.name.clone(),"STARTED BOOTSTRAPPING EC".to_string());

        //Create the initial graph
        self.state.lock().unwrap().insert_graph();
        self.state.lock().unwrap().name = self.name.clone();

        let mut parser = XMLGraphParser::new(self.state.clone(),"baremetal".to_string());

        let text = std::fs::read_to_string(self.topology_file.clone()).unwrap();
        
        let doc = Document::parse(&text).unwrap();

        let root = doc.root().first_child().unwrap();

        //Parses topology
        parser.fill_graph(root.clone());

        //Collect config properties

        self.shortest_path_type = parser.shortest_path_type.to_string();

        self.pool_period = parser.pool_period;

        self.max_age = parser.max_age;

        let service_count = self.state.lock().unwrap().get_current_graph().lock().unwrap().services.keys().len();

        //Get own ip with the provided network device
        self.ip = get_own_ip(Some(self.networkdevice.clone()));

        //id is the same as the ip in baremetal
        self.id = self.ip.to_string();

        //Start the CM process
        let process = self.start_cm(service_count);


        //Graph related operation
        self.state.lock().unwrap().get_current_graph().lock().unwrap().set_graph_root_baremetal(self.networkdevice.clone());

        self.calculate_paths();

        //Parse dynamic events
        
        self.scheduler.lock().unwrap().shortest_path_type = self.shortest_path_type.clone();
        
        parser.parse_schedule(self.scheduler.clone(),root);
        self.scheduler.lock().unwrap().sort_events();
        self.scheduler.lock().unwrap().pid = self.pid;

        self.scheduler.lock().unwrap().script = self.state.lock().unwrap().get_current_graph().lock().unwrap().graph_root.as_ref().unwrap().lock().unwrap().script.clone();

        //Get how many links we have in the experiment if >255 use u16 else u8
        let removed_links_len = self.state.lock().unwrap().get_current_graph().lock().unwrap().removed_links.len();
        self.link_count = (self.state.lock().unwrap().get_current_graph().lock().unwrap().links.keys().len() + removed_links_len) as u32;
        self.state.lock().unwrap().set_link_count(self.link_count);

        print_message(self.name.clone(),"STARTING TC".to_string());
        self.state.lock().unwrap().init(self.ip).map_err(|err| println!("{:?}", err)).ok();
        

        self.comms.lock().unwrap().ip = self.ip.clone();
        self.comms.lock().unwrap().id = self.ip.clone().to_string();
        self.comms.lock().unwrap().init();
        self.comms.lock().unwrap().start_polling(self.state.clone(),self.link_count);
        
        
        //create variables to send to thread
        let scheduler = self.scheduler.clone();
        let shutdown = self.shutdown.clone();

        thread::spawn(move || {accept_loop_baremetal(scheduler,shutdown,Arc::new(Mutex::new(process)))});

    }
    

    pub fn init(&mut self){

        print_message(self.name.clone(),format!("Started EC with ID {}",self.id));

        //Parse the topology
        //self.state.lock().unwrap().name = self.name.clone();
        self.state.lock().unwrap().insert_graph();

        let mut parser = XMLGraphParser::new(self.state.clone(),"container".to_string());
        
        let text = std::fs::read_to_string("/topology.xml".to_string()).unwrap();

        let doc = Document::parse(&text).unwrap();

        let root = doc.root().first_child().unwrap();

        parser.fill_graph(root.clone());

        //Collect config properties

        self.shortest_path_type = parser.shortest_path_type.to_string();

        self.pool_period = parser.pool_period;

        self.max_age = parser.max_age;

        let sleeptime = time::Duration::from_millis(2000);
        thread::sleep(sleeptime);

        //Get ips of all containers
        print_message(self.name.clone(),"Looking for ips".to_string());
        if self.orchestrator == "docker"{
            self.state.lock().unwrap().get_current_graph().lock().unwrap().resolve_hostnames_docker();
        }else{
            //Need new runtime because kubernetes library uses block_on
            let basic_rt = runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
            let graph = self.state.lock().unwrap().get_current_graph().clone();
            basic_rt.block_on(async {resolve_hostnames_kubernetes(graph).await}).map_err(|err| println!("{:?}", err)).ok();
        }
        print_message(self.name.clone(),"Got ips".to_string());
        self.state.lock().unwrap().get_current_graph().lock().unwrap().set_graph_root();
        self.calculate_paths();

        //Set name for debug
        self.scheduler.lock().unwrap().name = self.name.clone();

        self.scheduler.lock().unwrap().shortest_path_type = self.shortest_path_type.clone();

        self.state.lock().unwrap().emulation.lock().unwrap().name = self.name.clone();


        //Parse dynamic events
        parser.parse_schedule(self.scheduler.clone(),root.clone());
        self.scheduler.lock().unwrap().sort_events();

        self.state.lock().unwrap().shrink_maps();
        self.scheduler.lock().unwrap().pid = self.pid;

        //Get how many links we have in the experiment if >255 use u16 else u8
        let removed_links_len = self.state.lock().unwrap().get_current_graph().lock().unwrap().removed_links.len();
        self.link_count = (self.state.lock().unwrap().get_current_graph().lock().unwrap().links.keys().len() + removed_links_len) as u32;


        //Start communication
        self.comms.lock().unwrap().start_polling(self.state.clone(),self.link_count);

        self.state.lock().unwrap().set_link_count(self.link_count);

        let pid = self.pid.clone();

        let scheduler = self.scheduler.clone();

        let start = self.start.clone();

        let shutdown = self.shutdown.clone();

        self.ip = get_own_ip(None);

        //Start TC structures
        self.state.lock().unwrap().init(self.ip).map_err(|err| println!("{:?}", err)).ok();

        self.comms.lock().unwrap().ip = self.ip.clone();
        self.comms.lock().unwrap().name = self.name.clone();

        //self.state.lock().unwrap().get_current_graph().lock().unwrap().print_graph(self.name.clone());

        thread::spawn(move || {accept_loop(scheduler,pid,start,shutdown)});
        
        print_message(self.name.clone(),format!("Started with defaults values: pool_period={},max_age={},shortest_path={}",self.pool_period,self.max_age,self.shortest_path_type));


        print_message(self.name.clone(),format!("EC with ID {} is now ON",self.id));
    }

    pub fn start_cm(&mut self,service_count:usize) -> Popen{
        //Create auxiliary files, CM reads from these files, dashboard is not relevant we just create an empty file

        OpenOptions::new().write(true).create(true).append(true).open("/tmp/topoinfodashboard").unwrap();
        
        let mut topoinfo = OpenOptions::new().write(true).create(true).append(true).open("/tmp/topoinfo").unwrap();

        let mut remote_ips = OpenOptions::new().write(true).create(true).append(true).open("/remote_ips.txt").unwrap();

        for (ip,_) in self.state.lock().unwrap().get_current_graph().lock().unwrap().services.iter(){
            if ip != &self.ip{
                let string = format!("{}\n",ip);
                remote_ips.write_all(string.to_string().as_bytes()).unwrap();
            }
        }

        topoinfo.write_all(self.ip.to_string().as_bytes()).map_err(|err| println!("{:?}", err)).ok();
        let mut string_command = vec![];

        string_command.push(format!("./{}",self.cm_file.clone().to_string()));
        string_command.push(service_count.to_string());
        string_command.push("0.0.0.0".to_string());

        let process = Popen::create(&string_command, PopenConfig::default()).unwrap();
        self.pid = process.pid().unwrap();
        print_message(self.name.clone(),"GOT PID".to_string());
        return process;
    }

    pub fn calculate_paths(&mut self){
        let mut state = self.state.lock().unwrap();
        {
            if self.shortest_path_type.eq("hop"){
                state.get_current_graph().lock().unwrap().calculate_shortest_paths();
            }
            if self.shortest_path_type.eq("latency"){
                state.get_current_graph().lock().unwrap().calculate_shortest_paths_latency();
            }

            state.get_current_graph().lock().unwrap().calculate_properties();
            self.name = state.get_current_graph().lock().unwrap().get_name().clone();
            state.name = self.name.clone();
        }   
    }

    pub async fn setup_ebpf(&mut self){
        let usages = self.usages.clone();
        tokio::spawn(async {get_local_usage(usages).await});
    }

    
    pub async fn emulation_loop(&mut self){

        self.setup_ebpf().await;

        let handle = Handle::current();

        self.scheduler.lock().unwrap().tokiohandler = Some(handle);
        self.lasttime = Some(Instant::now());
        self.check_active_flows();

        loop{
            //wait for the experiment to start, not relevant if not debugging
            if *self.shutdown.lock().unwrap(){
                println!("SHUTDOWN");
                break;
            }
            if !*self.start.lock().unwrap(){
                let sleeptime = 0.0001;
                thread::sleep(Duration::from_secs_f32(sleeptime));
                continue;
            }
            let sleeptime = self.pool_period - (self.lasttime.unwrap().elapsed().as_millis() as f32)/1000.0;

            if sleeptime > 0.0{
                thread::sleep(Duration::from_secs_f32(sleeptime));
            }

            
            self.state.lock().unwrap().clear_paths();

            self.check_active_flows();

            self.broadcast_flows();

            self.state.lock().unwrap().calculate_bandwidth();



        }
    }
    

    //Read from local usages map
    fn check_active_flows(&mut self){
        let usage = self.usages.lock().unwrap().clone();
        let iter = usage.iter();
        for (ip,bytes) in iter{
        
            let last_bytes  = self.state.lock().unwrap().get_current_graph().lock().unwrap().get_lastbytes(&ip).clone();

            self.state.lock().unwrap().get_current_graph().lock().unwrap().set_lastbytes(&ip,*bytes);

            let delta_bytes;
            if last_bytes > *bytes{
                delta_bytes = *bytes;
            }else{
                delta_bytes = *bytes - last_bytes;
            }

            //print_message(self.name.clone(),format!("delta bytes is {}",delta_bytes.clone()));
            let delta_time = self.lasttime.unwrap().elapsed().as_millis() as f32;

            
            //let bits:u128 = (delta_bytes * 8).into();
            let bits = delta_bytes * 8;
            let throughput = bits as f32 / (delta_time/1000.0);
            
            let useful = self.state.lock().unwrap().get_current_graph().lock().unwrap().process_usage(*ip,throughput).clone();

            if useful{ 
                self.state.lock().unwrap().insert_active_path_id(*ip);
                //print_message(self.name.clone(),format!("throughput is {}",throughput.clone()));
            }
            
        }

        self.lasttime = Some(Instant::now());
        
    }

    //Sends metadata to CM
    fn broadcast_flows(&mut self){

        //Retrieve paths the node sent bytes to
        let active_paths = self.state.lock().unwrap().get_active_paths().clone();

        let active_paths_len = active_paths.len().clone();
        if active_paths_len > 0{

            let mut message: Builder<HeapAllocator> = Builder::new_default();
            let mut msg: message_capnp::message::Builder<'_> = message.init_root::<message_capnp::message::Builder>();

            self.comms.lock().unwrap().init_message(msg.reborrow(), self.state.lock().unwrap().ec_cycle.clone() as u32, active_paths_len as u32);


            let mut flow_number = 0;
            let mut total_bw = 0;            
            for path in active_paths{
                
                let path = path.lock().unwrap();

                {
                    //TODO add bw and links
                    let bandwidth = path.used_bandwidth.clone() as u32;
                    total_bw= total_bw + bandwidth;
                    let len_links = path.links.len().clone() as u32;

                    if len_links == 0 || len_links > 254{
                        print_message(self.name.clone(),"Len links is 0 or bigger than 254\n".to_string())
                    }

                    let links = path.links.clone();

                    self.comms.lock().unwrap().add_flow(msg.reborrow(), bandwidth, len_links, links,flow_number);


                    flow_number+=1;
                }

            }
            self.comms.lock().unwrap().send_message(message);

        }
    
    }
}


/// Insert received message data from monitor's eBPF PerfEventMap into `usages`
async fn get_local_usage(usages: Arc<Mutex<HashMap<u32,u32>>>) {
    let iface = "eth0";
    tokio::task::spawn(async move {
        let mut ebpf_handle = monitor::run(iface).await.unwrap();

        while let Some(msg) = ebpf_handle.rx.recv().await {
            usages.lock().unwrap().insert(msg.dst, msg.bytes);
        }
    });
}

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

//Waits to accept dashboard connection
fn accept_loop(eventscheduler:Arc<Mutex<EventScheduler>>,pid:u32,start:Arc<Mutex<bool>>,shutdown:Arc<Mutex<bool>>) -> Result<()> {
    let port:u32 = 7073;
    let listener = TcpListener::bind(format!("{}{}","0.0.0.0:",port.to_string()))?;
    for stream in listener.incoming() {
        receive_commands(stream?,eventscheduler.clone(),pid,start.clone(),shutdown.clone()).map_err(|err| println!("{:?}", err)).ok();
    }
    Ok(())
}

//Receives commands from dashboard
fn receive_commands(mut stream:TcpStream,eventscheduler:Arc<Mutex<EventScheduler>>,pid:u32,start:Arc<Mutex<bool>>,shutdown:Arc<Mutex<bool>>)-> Result<()>{
    let shutdown_command:u8 = 2;
    let ready_command:u8 = 3;
    let start_command:u8 = 4;
    let ack:u8 = 120;
    let mut buf = [0;1];
    
    loop{
        match stream.read(&mut buf) {
            Ok(_) => {
                if buf[0] == shutdown_command{
                    
                    *shutdown.lock().unwrap() = true;
                    stop_experiment(pid,3);
                    
                    break;
                }
                if buf[0] == ready_command{
                    stream.write_all(&[ack]).map_err(|err| println!("{:?}", err)).ok();
        
                }
                if buf[0] == start_command{
                    let ev = eventscheduler.clone();
                    thread::spawn(move || start_events(ev));
                    *start.lock().unwrap() = true;  
                    
                }
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                continue;
            }
            Err(e) => {
                return Err(e.into());
            }
        }
    }

    Ok(())
}

//Start the experiment
fn start_events(eventscheduler:Arc<Mutex<EventScheduler>>){
    eventscheduler.lock().unwrap().start();
}

pub async fn resolve_hostnames_kubernetes(graph:Arc<Mutex<Graph>>) -> Result<()>{

    let mut services_by_name = graph.lock().unwrap().services_by_name.clone();
    for (name,services) in services_by_name.iter_mut(){


        let mut ips:Vec<Ipv4Addr>;
        loop{
            ips = vec![];
            let client = Client::try_default().await?;


            let pods: Api<Pod> = Api::default_namespaced(client);
            for p in pods.list(&ListParams::default()).await? {
                let key = "KOLLAPS_UUID";
                let uuid = match env::var(key) {
                    Ok(val) => Some(val),
                    Err(_e) => None,
                };
                let name = format!("{}-{}",name,uuid.as_ref().unwrap());
                if p.name().starts_with(&name){
                    let ip_string = p.status.unwrap().pod_ip;

                    if ip_string.is_none(){
                        continue;
                    }
                    let ip_string = ip_string.unwrap();


                    let ip = IpAddr::from_str(&ip_string).unwrap();
                    match ip{
                        IpAddr::V4(ipv4)=>{
                            ips.push(ipv4);
                        },
                        IpAddr::V6(_ipv6) => break,
                    }
                }
            }
            if ips.len() == services.len(){
                break;
            }

            let sleeptime = time::Duration::from_millis(1000);
            thread::sleep(sleeptime);

        }
        ips.sort();
        for (i,service) in services.iter().enumerate(){
            let int_ip = convert_to_int(ips[i].octets());
            service.lock().unwrap().ip = int_ip;
            service.lock().unwrap().replica_id = i;
            graph.lock().unwrap().services.insert(int_ip,Arc::clone(service));
        }
    }

    Ok(())


    
}

//Waits to accept dashboard connection
fn accept_loop_baremetal(eventscheduler:Arc<Mutex<EventScheduler>>,shutdown:Arc<Mutex<bool>>,cm_process:Arc<Mutex<Popen>>) -> Result<()> {
    let port:u32 = 7073;
    let listener = TcpListener::bind(format!("{}{}","0.0.0.0:",port.to_string()))?;
    for stream in listener.incoming() {
        receive_commands_baremetal(stream?,eventscheduler.clone(),shutdown.clone(),cm_process.clone()).map_err(|err| println!("{:?}", err)).ok(); 
    }
    Ok(())
}

//Receives commands from dashboard
fn receive_commands_baremetal(mut stream:TcpStream,eventscheduler:Arc<Mutex<EventScheduler>>,shutdown:Arc<Mutex<bool>>,cm_process:Arc<Mutex<Popen>>)-> Result<()>{
    let shutdown_command:u8 = 2;
    let ready_command:u8 = 3;
    let start_command:u8 = 4;
    let ack:u8 = 120;
    let mut buf = [0;1];
    
    stream.set_read_timeout(None).map_err(|err| println!("{:?}", err)).ok();
    stream.read(&mut buf).map_err(|err| println!("{:?}", err)).ok();
    if buf[0] == shutdown_command{
        
        cm_process.lock().unwrap().kill().map_err(|err| println!("{:?}", err)).ok();
        print!("SHUTDOWN CM");
        eventscheduler.lock().unwrap().state.lock().unwrap().emulation.lock().unwrap().tear_down();
        print!("TEARDOWN");
        *shutdown.lock().unwrap() = true;
        print!("SHUTDOWN FLAG ON");

    }
    if buf[0] == ready_command{
        stream.write_all(&[ack]).map_err(|err| println!("{:?}", err)).ok();

    }
    if buf[0] == start_command{
        let ev = eventscheduler.clone();
        thread::spawn(move || start_events(ev));
        
    
    }

    Ok(())
}
