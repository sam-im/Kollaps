use crate::{config::Config, network::Namespace};
use anyhow::{Context, Result};
use tracing::debug;

#[derive(Debug)]
pub struct Service {
    /// Symbolic name in topology.
    pub name: String,
    /// Executable name.
    pub image: String,
    /// Any arguments to be passed to executable.
    pub command: Option<String>,
}

impl Service {
    pub fn new(name: String, image: String, command: Option<String>) -> Self {
        Self {
            name,
            image,
            command,
        }
    }
}

#[derive(Clone)]
pub struct ReadyService {
    pub id: String,
    pub ns: Namespace,
    pub service: Service,
}

impl ReadyService {
    pub fn new(service: Service, ns: Namespace) -> Self {
        // Replace '_' by another character as we use it later to parse the id.
        let name = service.name.replace("_", "-");
        let addr = ns.addr.to_string().replace(".", "-");
        let id = format!("wasm_{}_{}", name, addr);
        Self { id, ns, service }
    }
}

pub struct ActiveService {
    pub pid: i32,
    pub service: ReadyService,
}

impl ActiveService {
    pub fn new(pid: i32, service: ReadyService) -> Self {
        Self { pid, service }
    }
    pub fn pid(&self) -> i32 {
        self.pid
    }
    pub fn id(&self) -> &String {
        &self.service.id
    }
    pub fn name(&self) -> &String {
        &self.service.service.name
    }
    pub fn veth(&self) -> &String {
        &self.service.ns.veth
    }
}

pub fn parse_services(config: &Config) -> Result<Vec<Service>> {
    use emulationcore::xmlgraphparser::XMLGraphParser;
    use std::fs::read_to_string;
    use tracing::info;

    info!("Parsing topology file.");
    let topology =
        read_to_string(&config.topology_path).context("Failed to read the topology file.")?;
    let parser = XMLGraphParser::try_new(&topology, "container".to_string())
        .context("Failed to parse the topology file.")?;

    info!("Extracting services.");
    let services = {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let (_config, graph) = rt.block_on(parser.fill_graph());

        graph
            .services_by_name
            .into_iter()
            .flat_map(|(name, vec)| {
                let name = name.to_ascii_lowercase();
                vec.iter()
                    .map(|e| {
                        let s = e.blocking_lock();
                        let image = s.image.clone().unwrap();
                        let command = s.command.clone();
                        Service::new(name.clone(), image, command)
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<Service>>()
    };
    debug!("Services = {:?}", services);
    Ok(services)
}
