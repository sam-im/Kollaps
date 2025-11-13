use crate::config::Config;
use crate::network::Bridge;
use crate::service::{ActiveService, ReadyService, Service, parse_services};

use std::ffi::CString;
use std::fs;
use std::net::Ipv4Addr;
use std::process::{Child, Command};
use std::str::FromStr;

use anyhow::{Context, Result};
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
        info!("Cleaning up spawned processes.");
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
        let services = parse_services(&self.config).context(format!(
            "Failed to parse services from {}.",
            &self.config.topology_path
        ))?;

        self.make_bridge().context(format!(
            "Failed to create a bridge with {}.",
            &self.config.addr
        ))?;

        let services = self
            .make_ready_services(services)
            .context("Failed to create namespaces for each service.")?;

        self.setup_tempdir(&services)
            .context("Failed to create temporary directories.")?;

        info!("Starting communicationmanager.");
        let comms = Command::new("communicationmanager")
            .arg(services.len().to_string())
            .spawn()?;
        self.comms = Some(comms);

        let services = self
            .make_active_services(&services)
            .context("Failed to create one or more service.")?;
        self.services = Some(services);

        let ecores = self
            .make_ecores()
            .context("Failed to create one or more emulationcore instances.")?;
        self.ecores = Some(ecores);

        info!("Waiting emulationcore instances to finish.");
        self.ecores
            .as_mut()
            .unwrap()
            .iter_mut()
            .for_each(|e| match e.wait() {
                Ok(status) => info!(
                    "An emulationcore instance exited with: {}.",
                    status.to_string()
                ),
                Err(_) => info!("An emutioncore instance exited with an unknown exit status."),
            });

        Ok(())
    }

    fn make_bridge(&mut self) -> Result<()> {
        info!("Creating a bridge.");
        let addr = Ipv4Addr::from_str(&self.config.addr)?;
        self.bridge = Some(Bridge::new("k_wasm", addr, 24));
        Ok(())
    }

    fn make_ready_services(&mut self, services: Vec<Service>) -> Result<Vec<ReadyService>> {
        info!("Creating namespaces for each service.");
        let mut ready_services = vec![];
        for s in services {
            let ns = self.bridge.as_mut().unwrap().create_namespace()?;
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
            if s.service.name == "dashboard" {
                topoinfodashboard.push_str(&s.id);
                topoinfodashboard.push('\n');
            } else {
                topoinfo.push_str(&s.id);
                topoinfo.push('\n');
            }
        });

        let _ = fs::create_dir(&self.config.tmp_dir);
        // let _ = fs::create_dir(format!("{}{}", &config.tmp_dir, "pipes"));
        fs::File::create(format!(
            "{}{}",
            &self.config.tmp_dir, &self.config.remote_ips_path
        ))?;
        fs::write(
            format!("{}{}", &self.config.tmp_dir, &self.config.topoinfo_path),
            topoinfo,
        )?;
        fs::write(
            format!(
                "{}{}",
                &self.config.tmp_dir, &self.config.topoinfodashboard_path
            ),
            topoinfodashboard,
        )?;
        Ok(())
    }

    fn make_active_services(&mut self, services: &[ReadyService]) -> Result<Vec<ActiveService>> {
        info!("Starting services.");
        let services: Vec<ActiveService> = services
            .iter()
            .filter(|s| s.service.name != "dashboard") // consider also starting the dashboard
            .map(|s| {
                let program = s.service.image.clone();
                let args = match &s.service.command {
                    Some(args) => args.split_ascii_whitespace().fold(vec![], |mut acc, a| {
                        let candidates = services
                            .iter()
                            .filter(|s| s.service.name.starts_with(&format!("${}", a)))
                            .collect::<Vec<&ReadyService>>();
                        if candidates.is_empty() {
                            acc.push(a.to_string());
                        } else {
                            let tmp: Vec<&str> = a
                                .split(&format!("${}", candidates[0].service.name))
                                .collect();
                            if let Some(str) = tmp.get(1)
                                && let Ok(i) = usize::from_str(str)
                            {
                                let c = candidates.get(i).unwrap_or(&candidates[0]);
                                acc.push(c.ns.addr.to_string());
                            }
                        }
                        acc
                    }),
                    None => vec![],
                };

                // Using `libc::fork` we can create a child for each service that raises SIGSTOP immediately.
                // This way we can defer the starting of runtimes to emulationcore and still have each
                // runtime's PID beforehand.
                let pid;
                let program = CString::new(program).unwrap();
                let args = CString::new(args.join(" ")).unwrap();

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
                                s.id, &self.config.runtime_name, res
                            );
                            // Exit without executing any RAII guards in the child
                            // if replacing the child with the service fails.
                            std::process::exit(-1);
                        }
                        pid if pid < 0 => {
                            panic!("Failed to fork runtime handler, error code: {}", pid);
                        }
                        pid => {
                            info!("Forked a paused service process with pid: {}.", pid);
                        }
                    }
                }
                ActiveService::new(pid, s.clone())
            })
            .collect();
        Ok(services)
    }

    fn make_ecores(&self) -> Result<Vec<Child>> {
        let mut ecores = vec![];
        for s in self
            .services
            .as_ref()
            .unwrap()
            .iter()
            .filter(|s| s.name() != "dashboard")
        {
            let child = Command::new("emulationcore")
                .arg(s.id())
                .arg(s.pid().to_string())
                .arg("wasm")
                .arg(s.veth())
                .spawn()?;
            ecores.push(child);
        }
        Ok(ecores)
    }
}
