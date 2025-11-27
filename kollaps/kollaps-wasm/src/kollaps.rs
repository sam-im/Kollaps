use crate::config::Config;
use crate::network::Bridge;
use crate::service::{ActiveService, ReadyService, Service, parse_command, parse_services};

use std::ffi::CString;
use std::fs::File;
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
use tracing::{debug, error, info, warn};

pub struct Kollaps {
    config: Config,
    bridge: Option<Bridge>,
    comms: Option<Child>,
    services: Option<Vec<ActiveService>>,
    ecores: Option<Vec<Child>>,
}

impl Drop for Kollaps {
    fn drop(&mut self) {
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

        self.bridge = Some(self.make_bridge("kollaps")?);
        let services = self.make_ready_services(services)?;

        self.setup_tempdir(&services)?;
        self.comms = Some(self.make_comms(services.len())?);
        self.services = Some(self.make_active_services(services)?);
        self.ecores = Some(self.make_ecores()?);

        // Checks if all emulationcore instances have exited.
        let is_done = |ecores: &mut Vec<Child>| -> bool {
            ecores.iter_mut().map(|e| e.try_wait()).all(|res| {
                if let Ok(result) = res
                    && let Some(exit_status) = result
                {
                    debug!(
                        "An emulationcore instance exited with: {}",
                        exit_status.to_string()
                    );
                    return true;
                }
                false
            })
        };

        // Sets up a signal flag to run destructors before exiting.
        let term = Arc::new(AtomicBool::new(false));
        for sig in signal_hook::consts::TERM_SIGNALS {
            signal_hook::flag::register(*sig, Arc::clone(&term))
                .context("Failed to set signal handlers.")?;
        }

        info!("Ready. Start the dashboard or hit CTRL+C to exit.");

        // Exits if the signal flag is set, or all ecore instances have exited.
        while !term.load(Ordering::Relaxed) {
            sleep(Duration::from_millis(100));
            if is_done(self.ecores.as_mut().unwrap()) {
                info!("All emulationcore instances exited");
                break;
            }
        }

        Ok(())
    }

    fn make_bridge(&mut self, name: &str) -> Result<Bridge> {
        info!("Creating virtual bridge.");
        let bridge = Bridge::new(name, self.config.addr, self.config.subnet);
        bridge
            .create()
            .context(format!("failed to create bridge {}", name))?;

        Ok(bridge)
    }

    fn make_ready_services(&mut self, services: Vec<Service>) -> Result<Vec<ReadyService>> {
        info!("Creating namespaces for services.");
        let mut ready_services = vec![];
        for s in services.into_iter() {
            let ns = self
                .bridge
                .as_mut()
                .unwrap()
                .create_namespace()
                .context("Failed to create a namespace.")?;
            let s = ReadyService::new(s, ns);
            ready_services.push(s);
        }
        Ok(ready_services)
    }

    fn setup_tempdir(&self, services: &[ReadyService]) -> Result<()> {
        info!("Setting up temporary directory.");
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

        let dirs = vec![
            (&self.config.tmp_dir, 0o775),
            (&self.config.pipes_dir, 0o777),
            (&self.config.logs_dir, 0o777),
        ];
        for dir in dirs {
            let _ = fs::create_dir(dir.0);
            fs::set_permissions(dir.0, fs::Permissions::from_mode(dir.1))?;
        }

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

    fn make_comms(&self, service_count: usize) -> Result<Child> {
        info!("Starting communicationmanager.");
        let stdout = File::create(format!(
            "{}.communicationmanager.log",
            &self.config.logs_dir
        ))?;
        let stderr = stdout.try_clone()?;

        let comms = Command::new("./bin/communicationmanager")
            .arg(service_count.to_string())
            .stdout(stdout)
            .stderr(stderr)
            .spawn()
            .context("failed to start communicationmanager")?;

        Ok(comms)
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

                            // Redirects the outputs of the child to a logfile.
                            let log_file =
                                CString::new(format!("{}{}.log", &self.config.logs_dir, s.id()))
                                    .unwrap();
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
                                    s.id(),
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
                            // and the child needs to call `std::process::exit` to avoid
                            // running any destructors.
                            // This can be improved by implementing a flag that is set
                            // by the child to conditionally run destructors.
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

        // If any fails, kill the correct ones and return an error.
        if services.iter().any(|s| s.pid() < 0) {
            services
                .iter()
                .filter(|s| s.pid() > 0)
                .for_each(|child| unsafe {
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
                let (stdout, stderr) = match File::create(format!(
                    "{}.emulationcore_{}.log",
                    &self.config.logs_dir,
                    service.id()
                )) {
                    Ok(stdout) => {
                        let stderr = match stdout.try_clone() {
                            Ok(stderr) => stderr,
                            Err(e) => {
                                acc.push(Err(e));
                                return acc;
                            }
                        };
                        (stdout, stderr)
                    }
                    Err(e) => {
                        acc.push(Err(e));
                        return acc;
                    }
                };

                let child = Command::new("ip")
                    .args(["netns", "exec", service.ns_name()])
                    .args([
                        "./bin/emulationcore",
                        service.id(),
                        &service.pid().to_string(),
                        "wasm",
                        service.veth(),
                    ])
                    .stdout(stdout)
                    .stderr(stderr)
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
