use std::net::Ipv4Addr;
use std::str::FromStr;

use crate::{config::Config, network::Namespace};
use anyhow::{Context, Result};
use tracing::{debug, info, warn};

/// Represents a service defined in a topology description file.
#[derive(Clone, Debug)]
pub struct Service {
    /// Symbolic name in topology.
    name: String,
    /// Executable name.
    image: String,
    /// Any arguments to be passed to executable.
    command: Option<String>,
}

impl Service {
    /// Each argument corresponds to a tag with the same name in the service
    /// descriptions of a topology description file.
    ///
    /// Arguments:
    /// - `name`: specifies a name that is used to refer to this service.
    /// - `image`: specifies the name of an executable in `PATH`.
    /// - `command`: can be optionally set to pass arguments to the executable
    ///    specified by `image`.
    ///    Arguments in `command` can include variables, see `parse_command`.
    pub fn new(name: String, image: String, command: Option<String>) -> Self {
        Self {
            name,
            image,
            command,
        }
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn image(&self) -> &str {
        &self.image
    }
    pub fn command(&self) -> Option<&str> {
        self.command.as_deref()
    }
}

/// Represents a service and a namespace pair.
#[derive(Clone)]
pub struct ReadyService {
    id: String,
    ns: Namespace,
    service: Service,
}

impl ReadyService {
    pub fn new(service: Service, ns: Namespace) -> Self {
        // Replace '_' by another character as we use it later to parse the id.
        let name = service.name.replace("_", "-");
        let addr = ns.addr().to_string().replace(".", "-");
        let id = format!("wasm_{}_{}", name, addr);
        Self { id, ns, service }
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn name(&self) -> &str {
        self.service.name()
    }
    pub fn image(&self) -> &str {
        self.service.image()
    }
    pub fn command(&self) -> Option<&str> {
        self.service.command()
    }
    pub fn ns(&self) -> &Namespace {
        &self.ns
    }
    pub fn addr(&self) -> &Ipv4Addr {
        &self.ns.addr()
    }
}

pub struct ActiveService {
    pid: i32,
    service: ReadyService,
}

impl ActiveService {
    pub fn new(pid: i32, service: ReadyService) -> Self {
        Self { pid, service }
    }
    pub fn pid(&self) -> i32 {
        self.pid
    }
    pub fn id(&self) -> &str {
        &self.service.id
    }
    pub fn name(&self) -> &str {
        &self.service.service.name
    }
    pub fn veth(&self) -> &str {
        &self.service.ns.veth()
    }
    pub fn ns_name(&self) -> &str {
        &self.service.ns.name()
    }
}

pub fn parse_services(config: &Config) -> Result<Vec<Service>> {
    use emulationcore::xmlgraphparser::XMLGraphParser;
    use std::fs::read_to_string;

    info!("Parsing topology file.");
    let topology = read_to_string(&config.topology_path).context(format!(
        "failed to read the topology file {}",
        &config.topology_path.to_string_lossy()
    ))?;
    let parser = XMLGraphParser::try_new(&topology, "container".to_string())
        .context("failed to parse the topology file")?;

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
    debug!("Services = {:#?}", services);
    Ok(services)
}

// TODO: include an example
/// Parses a command and returns a vector containing arguments to be used as an argv for a service process.
/// An argument that starts with a '$' and is a service name designates a variable to be replaced by the address
/// of the respective service.
/// Such a variable can optionally include a suffix for a replica index number to allow choosing a specific instance
/// of the same service.
pub fn parse_command(command: &str, services: Vec<(String, Ipv4Addr)>) -> Vec<String> {
    command.split_whitespace().fold(vec![], |mut acc, arg| {
        let candidates: Vec<&(String, Ipv4Addr)> = services
            .iter()
            .filter(|(name, _)| {
                let var_name = format!("${}", name);
                arg.starts_with(&var_name)
            })
            .collect();

        if candidates.is_empty() {
            acc.push(arg.to_string());
        } else {
            let replica_id = arg
                .split(&format!("${}", candidates[0].0))
                .find(|s| !s.is_empty())
                .and_then(|s| usize::from_str(s).ok());
            match replica_id {
                Some(i) => {
                    let index = i - 1;
                    let subst = match candidates.get(index) {
                        Some((_, addr)) => addr.to_string(),
                        None => {
                            warn!(
                                "Replica {} not found for service {} in command {}.\nLeaving the argument as is.",
                                i,
                                candidates[0].0,
                                command
                            );
                            arg.to_string()
                        },
                    };
                    debug!("Argument {} in {}'s command is replaced by {}.", candidates[0].0, arg, subst);
                    acc.push(subst);
                },
                None => {
                    let arg = candidates[0].1.to_string();
                    acc.push(arg);
                },
            }
        }
        acc
    })
}
