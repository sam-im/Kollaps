use crate::{config::Config, network::Namespace};

use std::ffi::CString;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::ptr;
use std::str::FromStr;

use anyhow::{Context, Error, Result};
use libc::{SIGSTOP, execvp, raise};
use tracing::{debug, error, info, warn};

/// Represents a service defined in a topology description file.
#[derive(Clone, Debug)]
pub struct Service {
    /// Symbolic name in topology.
    name: String,
    /// Executable name in PATH or a WASM module.
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
    /// - `image`: specifies the name of an executable in `PATH` or a WASM module.
    /// - `command`: can be optionally set to pass arguments to the executable
    ///   specified by `image`.
    ///   Arguments in `command` can include variables, see `parse_command`.
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
        let id = format!("kollaps_{}_{}", name, addr);
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
        self.ns.addr()
    }
}

/// A struct that represents a spawned service process.
/// Dropping this struct will kill the respective process.
pub struct ActiveService {
    pid: i32,
    service: ReadyService,
}

impl ActiveService {
    /// Try and create a new `ActiveService` from a `ReadyService`.
    /// The argument `alist` is an association list of all the service names and addresses,
    /// used to parse variables, if any, from the command of a `service`.
    pub fn try_new(
        config: &Config,
        service: ReadyService,
        alist: &[(String, Ipv4Addr)],
    ) -> Result<Self> {
        let to_cstr = |str: &str| -> Result<CString> { Ok(CString::from_str(str)?) };

        let mut cstrings = vec![
            to_cstr("ip")?,
            to_cstr("netns")?,
            to_cstr("exec")?,
            to_cstr(service.ns().name())?,
        ];

        // If the image tag is a WASM module, run it with the default runtime.
        if service.image().ends_with(".wasm") {
            cstrings.push(to_cstr(
                &config
                    .executables_dir
                    .canonicalize()?
                    .join("kollaps-wasm-runtime")
                    .to_string_lossy(),
            )?);
            if let Some(allow_dir) = &config.allow_dir {
                cstrings.push(to_cstr("--dir")?);
                cstrings.push(to_cstr(&allow_dir.to_string_lossy())?);
            }
        }

        let image_path = PathBuf::from(service.image());
        if image_path.is_absolute() {
            cstrings.push(to_cstr(&image_path.to_string_lossy())?);
        } else {
            // If a user provides a relative path for a service's image,
            // it should be better to canonicalize it relative to the topology file,
            // as this seems to be the most intuitive behaviour one expects.
            let topology_path = &config.topology_path.canonicalize()?;
            let topology_dir = match topology_path.parent() {
                Some(p) => PathBuf::from(p),
                None => PathBuf::from("/"),
            };
            let image_path = match topology_dir.join(image_path).canonicalize() {
                // Either it's a valid path to an WASM module or executable,
                Ok(p) => p,
                // or an executable name from $PATH.
                Err(_) => service.image().into(),
            };
            cstrings.push(to_cstr(&image_path.to_string_lossy())?);
        }

        let service_args = match service.command() {
            Some(args) => parse_command(args, alist),
            None => vec![],
        };
        for a in service_args {
            cstrings.push(to_cstr(&a)?)
        }
        let mut argv = cstrings.iter().map(|c| c.as_ptr()).collect::<Vec<_>>();
        argv.push(ptr::null_mut());

        // Using `libc::fork` we can create a child for each service that raises SIGSTOP immediately.
        // This way we can defer the starting of runtimes to emulationcore and still have each
        // runtime's PID beforehand.
        let pid;

        unsafe {
            pid = libc::fork();
            match pid {
                0 => {
                    debug!("{} raising SIGSTOP.", service.id());
                    raise(SIGSTOP);
                    debug!("{} received SIGCONT.", service.id());

                    // TODO: consider running the service as non-root
                    // 1. implement argument and config changes for run-as-user
                    // 2. find uid of arg. provided username
                    // 3. setuid to it

                    // Redirects the outputs of the child to a logfile.
                    let log_file = CString::new(
                        config
                            .logs_dir
                            .join(format!("{}.txt", service.id()))
                            .to_string_lossy()
                            .as_bytes(),
                    )?;
                    let fd = libc::open(
                        log_file.as_ptr(),
                        libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC,
                        0o644,
                    );
                    if libc::dup2(fd, libc::STDOUT_FILENO) < 0
                        || libc::dup2(fd, libc::STDERR_FILENO) < 0
                    {
                        let err = std::io::Error::last_os_error();
                        warn!(
                            "Failed to redirect output of {} to logfile {}.\nError:\n{}",
                            service.id(),
                            log_file.to_string_lossy(),
                            err
                        );
                    } else {
                        libc::fcntl(libc::STDOUT_FILENO, libc::F_SETFD, 0);
                        libc::fcntl(libc::STDERR_FILENO, libc::F_SETFD, 0);
                        if fd > libc::STDERR_FILENO {
                            libc::close(fd);
                        }
                    }

                    // Replaces the current process
                    execvp(argv[0], argv.as_ptr());

                    // If the following code executes, then `exec` have failed,
                    // then the child needs to call `std::process::exit` to avoid
                    // running any destructors.
                    // This can be improved by implementing a flag that is set
                    // by the child to conditionally run destructors.
                    let err = std::io::Error::last_os_error();
                    error!(
                        "Service {} failed to run {}.\nError:\n{}.",
                        service.id(),
                        cstrings
                            .iter()
                            .map(|s| s.to_string_lossy())
                            .collect::<Vec<_>>()
                            .join(" "),
                        err,
                    );
                    std::process::exit(1);
                }
                pid if pid < 0 => {
                    error!("Failed to fork service handler, error code: {}", pid);
                    return Err(Error::msg("Failed to fork service handler."));
                }
                pid => {
                    debug!(
                        "Forked a paused process for service {} with pid: {}.",
                        service.id(),
                        pid
                    );
                }
            }
        }
        Ok(ActiveService { pid, service })
    }
    pub fn pid(&self) -> i32 {
        self.pid
    }
    pub fn id(&self) -> &str {
        self.service.id()
    }
    pub fn name(&self) -> &str {
        self.service.service.name()
    }
    pub fn veth(&self) -> &str {
        self.service.ns().veth()
    }
    pub fn ns_name(&self) -> &str {
        self.service.ns().name()
    }
}

impl Drop for ActiveService {
    fn drop(&mut self) {
        unsafe {
            let res = libc::kill(self.pid(), libc::SIGINT);
            // Handle the case where the experiment is never started and the services
            // are still paused forks of this process that waits to spawn the actual services.
            // Processes paused with SIGSTOP will receive SIGINT only after they are restarted.
            let _ = libc::kill(self.pid(), libc::SIGCONT);
            debug!("Sent SIGINT to service {}, result was {}", self.name(), res);
        }
    }
}

/// Uses emulationcore::XMLGraphParser to parse a topology file, returning a vector of `Service`s.
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

/// Parses a command and returns a vector containing arguments to be used as an argv for a service process.
/// An argument that starts with a '$' and is a service name designates a variable to be replaced by the address
/// of the respective service.
/// Such a variable can optionally include a suffix for a replica index number to allow choosing a specific instance
/// of the replicated service.
/// Example: The arguments "-c $server" may be returned as "-c 10.10.10.5".
fn parse_command(command: &str, services: &[(String, Ipv4Addr)]) -> Vec<String> {
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
                    debug!("Argument {} in command {} is replaced by {}.", arg, command, subst);
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
