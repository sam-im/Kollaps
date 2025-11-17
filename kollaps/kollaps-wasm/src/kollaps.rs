use crate::config::Config;
use crate::network::Bridge;
use crate::service::{ActiveService, ReadyService, Service, parse_command, parse_services};

use std::ffi::CString;
use std::fs;
use std::net::Ipv4Addr;
use std::process::{Child, Command};
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::sleep;
use std::time::Duration;

use anyhow::{Context, Error, Result};
use libc::{SIGSTOP, execlp, raise};
use tracing::{debug, error, info};

pub struct Kollaps {
    config: Config,
    bridge: Option<Bridge>,
    comms: Option<Child>,
    services: Option<Vec<ActiveService>>,
    ecores: Option<Vec<Child>>,
}

impl Drop for Kollaps {
    fn drop(&mut self) {
        debug!("Cleaning up spawned processes.");
        if let Some(services) = &self.services {
            services.iter().for_each(|s| unsafe {
                let _ = libc::kill(s.pid(), libc::SIGINT);
            });
        }
        if let Some(ecores) = &mut self.ecores {
            ecores.iter_mut().for_each(|e| {
                let _ = e.kill();
            });
        }
        if let Some(comms) = &mut self.comms {
            let _ = comms.kill();
        }
    }
}

impl Kollaps {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            bridge: None,
            services: None,
            comms: None,
            ecores: None,
        }
    }

    pub fn run(&mut self) -> Result<()> {
        let services = parse_services(&self.config)?;

        self.bridge = Some(self.make_bridge("k_wasm")?);

        let services = self.make_ready_services(services)?;

        self.setup_tempdir(&services)?;

        info!("Starting communicationmanager.");
        let comms = Command::new("./communicationmanager")
            .arg(services.len().to_string())
            .spawn()
            .context("failed to start communicationmanager")?;
        self.comms = Some(comms);

        self.services = Some(self.make_active_services(services)?);

        self.ecores = Some(self.make_ecores()?);

        // Checks if all emulationcore instances have exited.
        let is_done = |ecores: &mut Vec<Child>| -> bool {
            ecores.iter_mut().map(|e| e.try_wait()).all(|res| {
                if let Ok(result) = res
                    && let Some(exit_status) = result
                {
                    info!(
                        "An emulationcore instance exited with: {}",
                        exit_status.to_string()
                    );
                    return true;
                }
                false
            })
        };

        // Sets up a signal flag to run RAII guards before exiting.
        let term = Arc::new(AtomicBool::new(false));
        for sig in signal_hook::consts::TERM_SIGNALS {
            signal_hook::flag::register(*sig, Arc::clone(&term))
                .context("Failed to set signal handlers.")?;
        }

        // Exits if the signal flag is set, or all ecore instances have exited.
        while !term.load(Ordering::Relaxed) {
            sleep(Duration::from_millis(100));
            if is_done(self.ecores.as_mut().unwrap()) {
                break;
            }
        }

        Ok(())
    }

    fn make_bridge(&mut self, name: &str) -> Result<Bridge> {
        info!("Creating a virtual bridge.");
        let addr = Ipv4Addr::from_str(&self.config.addr)
            .context(format!("failed to parse address {}", &self.config.addr))?;
        let bridge = Bridge::new(name, addr, self.config.subnet);
        bridge
            .create()
            .context(format!("failed to create bridge {}", name))?;

        Ok(bridge)
    }

    fn make_ready_services(&mut self, services: Vec<Service>) -> Result<Vec<ReadyService>> {
        info!("Creating a namespace for each service.");
        let mut ready_services = vec![];
        for s in services {
            let ns = self
                .bridge
                .as_mut()
                .unwrap()
                .create_namespace()
                .context("Failed to created a namespace.")?;
            let s = ReadyService::new(s, ns);
            ready_services.push(s);
        }
        Ok(ready_services)
    }

    fn setup_tempdir(&self, services: &[ReadyService]) -> Result<()> {
        info!("Setting up temporary directories.");
        let mut topoinfo = String::new();
        let mut topoinfodashboard = String::new();
        services.iter().for_each(|s| {
            if s.service.name() == "dashboard" {
                topoinfodashboard.push_str(&s.id);
                topoinfodashboard.push('\n');
            } else {
                topoinfo.push_str(&s.id);
                topoinfo.push('\n');
            }
        });

        let _ = fs::create_dir(&self.config.tmp_dir);
        fs::File::create(&self.config.remote_ips_path)
            .context("failed to create empty remote_ips file in temp dir")?;

        let debug = |path: &str, content: &str| {
            debug!("\nWrote\n-----\n{}\n-----\nto {}.", path, content);
        };

        fs::write(&self.config.topoinfo_path, &topoinfo)
            .context(format!("failed to write to {}", &self.config.topoinfo_path))?;
        debug(&topoinfo, &self.config.topoinfo_path);

        fs::write(&self.config.topoinfodashboard_path, &topoinfodashboard).context(format!(
            "failed to write to {}",
            &self.config.topoinfodashboard_path
        ))?;
        debug(&topoinfodashboard, &self.config.topoinfodashboard_path);

        Ok(())
    }

    /// For each service forks a process that raises SIGSTOP immediately.
    /// The forked process runs the specified process name with arguments when receiving a SIGCONT.
    /// Any service name starting with '$' and optionally followed by its replica name is replaced by its
    /// address on the bridge.
    fn make_active_services(&mut self, services: Vec<ReadyService>) -> Result<Vec<ActiveService>> {
        info!("Starting services.");
        let alist = services
            .iter()
            .map(|s| (s.name().to_owned(), s.addr().to_owned()))
            .collect::<Vec<(String, Ipv4Addr)>>();

        let services: Vec<ActiveService> = services
            .into_iter()
            .filter(|s| s.service.name() != "dashboard")
            .map(|s| {
                let mut args = vec![
                    "netns".to_owned(),
                    "exec".to_owned(),
                    s.ns.name.clone(),
                    s.service.image().to_string(),
                ];
                let service_args = match s.service.command() {
                    Some(args) => parse_command(args, alist.clone()),
                    None => vec![],
                };
                args.extend(service_args);
                let args = args.join(" ");

                // Using `libc::fork` we can create a child for each service that raises SIGSTOP immediately.
                // This way we can defer the starting of runtimes to emulationcore and still have each
                // runtime's PID beforehand.
                let pid;
                let program = CString::new("ip").unwrap();
                let args = CString::new(args).unwrap();

                unsafe {
                    pid = libc::fork();
                    match pid {
                        0 => {
                            debug!("Child for {} raising SIGSTOP.", s.id);
                            raise(SIGSTOP);

                            debug!("Child for service {} received SIGCONT.", s.id);
                            let res = execlp(program.as_ptr(), args.as_ptr());

                            error!(
                                "Child for service {} failed to run {} with error code {}",
                                s.id,
                                program.to_string_lossy(),
                                res
                            );
                            // Exit without executing child's RAII guards inherited from parent.
                            std::process::exit(-1);
                        }
                        pid if pid < 0 => {
                            panic!("Failed to fork service handler, error code: {}", pid);
                        }
                        pid => {
                            debug!(
                                "Forked a paused process for service {} with pid: {}.",
                                s.id, pid
                            );
                        }
                    }
                }
                ActiveService::new(pid, s)
            })
            .collect();
        Ok(services)
    }

    /// Spawns an `emulationcore` process for each service in the topology,
    /// and returns them as `Child` objects in a `Vec`.
    fn make_ecores(&self) -> Result<Vec<Child>> {
        info!("Starting emulationcore instances.");
        let mut ecores = self
            .services
            .as_ref()
            .unwrap()
            .iter()
            .filter(|service| service.name() != "dashboard")
            .fold(vec![], |mut acc, service| {
                let child = Command::new("ip")
                    .args(["netns", "exec", service.ns_name()])
                    .args([
                        "./emulationcore",
                        service.id(),
                        &service.pid().to_string(),
                        "wasm",
                        service.veth(),
                    ])
                    .spawn();
                acc.push(child);
                acc
            });

        if ecores.iter().any(|e| e.is_err()) {
            ecores.iter_mut().filter(|e| e.is_ok()).for_each(|e| {
                let _ = e.as_mut().unwrap().kill();
            });
            return Err(Error::msg(
                "Failed to create one or more emulationcore instances.",
            ));
        }
        let ecores = ecores.into_iter().map(|e| e.unwrap()).collect();

        Ok(ecores)
    }
}
