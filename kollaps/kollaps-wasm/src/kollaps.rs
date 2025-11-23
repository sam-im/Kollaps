use crate::config::Config;
use crate::network::Bridge;
use crate::service::{ActiveService, ReadyService, Service, parse_command, parse_services};

use std::ffi::CString;
use std::net::Ipv4Addr;
use std::os::unix::fs::PermissionsExt;
use std::process::{Child, Command};
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::sleep;
use std::time::Duration;
use std::{fs, ptr};

use anyhow::{Context, Error, Result};
use libc::{SIGSTOP, execvp, raise};
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
        debug!("Cleaning up processes.");
        if let Some(services) = &self.services {
            services.iter().for_each(|s| unsafe {
                let res = libc::kill(s.pid(), libc::SIGINT);
                // Handle the case where the experiment is never started and the services
                // are still paused forks of this process that waits to spawn the actual services.
                // Processes paused with SIGSTOP will receive SIGINT only after they are restarted.
                let _ = libc::kill(s.pid(), libc::SIGCONT);
                debug!("Sent SIGINT to service {}, result was {}", s.name(), res);
            });
        }
        if let Some(ecores) = &mut self.ecores {
            ecores.iter_mut().for_each(|e| {
                let res = e.kill();
                debug!(
                    "Sent SIGINT to an emulationcore instance ({}), result was {:?}",
                    e.id(),
                    res
                );
            });
        }
        if let Some(comms) = &mut self.comms {
            let res = comms.kill();
            debug!(
                "Sent SIGINT to communicationmanager ({}), result was {:?}",
                comms.id(),
                res
            );
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
            if s.name() == "dashboard" {
                topoinfodashboard.push_str(s.id());
                topoinfodashboard.push('\n');
            } else {
                topoinfo.push_str(s.id());
                topoinfo.push('\n');
            }
        });

        let _ = fs::create_dir(&self.config.tmp_dir);
        let perms = fs::Permissions::from_mode(0o777); // TODO: is this safe?
        fs::set_permissions(&self.config.tmp_dir, perms)?;

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

    /// For each service forks a child process that raises SIGSTOP immediately and,
    /// after receiving a SIGCONT, runs the program specified by its service.
    ///
    /// Any name of a service in the arguments that starts with '$' and is optionally
    /// followed by a replica index is replaced by its address on the network bridge.
    fn make_active_services(&mut self, services: Vec<ReadyService>) -> Result<Vec<ActiveService>> {
        info!("Starting services.");
        let alist = services
            .iter()
            .map(|s| (s.name().to_owned(), s.addr().to_owned()))
            .collect::<Vec<(String, Ipv4Addr)>>();

        let to_cstr = |str: &str| -> CString { CString::from_str(str).unwrap() };
        let services: Vec<ActiveService> = services
            .into_iter()
            .filter(|s| s.name() != "dashboard")
            .map(|s| {
                let mut cstrings = vec![
                    to_cstr("ip"),
                    to_cstr("netns"),
                    to_cstr("exec"),
                    to_cstr(s.ns().name()),
                    to_cstr(s.image()),
                ];
                let service_args = match s.command() {
                    Some(args) => parse_command(args, alist.clone()),
                    None => vec![],
                };
                for a in service_args {
                    cstrings.push(to_cstr(&a))
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
                            debug!("{} raising SIGSTOP.", s.id());
                            raise(SIGSTOP);

                            debug!("{} received SIGCONT.", s.id());
                            execvp(argv[0], argv.as_ptr());
                            // If the following code executes, then `exec` have failed,
                            // and the child needs to call `std::process::exit` to avoid
                            // running any destructors.
                            // This can be improved by implementing a flag that a child
                            // can set and running the destructors conditionally.
                            let err = std::io::Error::last_os_error();
                            error!(
                                "Service {} failed to run {}.\nError:\n{}.",
                                s.id(),
                                cstrings
                                    .iter()
                                    .map(|s| s.to_string_lossy())
                                    .collect::<Vec<_>>()
                                    .join(" "),
                                err,
                            );
                            std::process::exit(-1);
                        }
                        pid if pid < 0 => {
                            error!("Failed to fork service handler, error code: {}", pid);
                        }
                        pid => {
                            debug!(
                                "Forked a paused process for service {} with pid: {}.",
                                s.id(),
                                pid
                            );
                        }
                    }
                }
                ActiveService::new(pid, s)
            })
            .collect();
        // If any of the forks have failed, clean up and return an error instead.
        if services.iter().any(|s| s.pid() < 0) {
            services.iter().for_each(|child| unsafe {
                libc::kill(child.pid(), libc::SIGINT);
            });
            return Err(Error::msg("Failed to spawn one or more services."));
        }
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
