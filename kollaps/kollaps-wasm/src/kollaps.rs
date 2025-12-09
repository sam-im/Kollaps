use crate::comms::CommunicationManager;
use crate::config::Config;
use crate::ecore::EmulationCore;
use crate::network::Bridge;
use crate::service::{ActiveService, ReadyService, Service, parse_services};

use std::fs;
use std::net::Ipv4Addr;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::sleep;
use std::time::Duration;

use anyhow::{Context, Error, Result};
use tracing::{debug, info};

pub struct Kollaps {
    config: Config,
    bridge: Option<Bridge>,
    comms: Option<CommunicationManager>,
    services: Option<Vec<ActiveService>>,
    ecores: Option<Vec<EmulationCore>>,
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

        self.make_tempdir(&services)?;
        self.comms = Some(self.make_comms(services.len())?);
        self.services = Some(self.make_active_services(services)?);
        self.ecores = Some(self.make_ecores()?);

        let is_done = |ecores: &mut Vec<EmulationCore>| -> Option<Vec<bool>> {
            let check = ecores.iter_mut().all(|e| e.try_wait().is_some());
            if check {
                Some(
                    ecores
                        .iter_mut()
                        .map(|e| e.try_wait().unwrap_or(false))
                        .collect(),
                )
            } else {
                None
            }
        };

        // Sets up a signal flag to run destructors before exiting.
        let term = Arc::new(AtomicBool::new(false));
        for sig in signal_hook::consts::TERM_SIGNALS {
            signal_hook::flag::register(*sig, Arc::clone(&term))
                .context("Failed to set signal handlers.")?;
        }

        info!("Ready. Start dashboard or hit CTRL+C to exit.");

        // Exits if the signal flag is set, or all ecore instances have exited.
        while !term.load(Ordering::Relaxed) {
            sleep(Duration::from_millis(100));

            if let Some(return_values) = is_done(self.ecores.as_mut().unwrap()) {
                info!("All emulationcore instances have exited.");
                let err_count = return_values.into_iter().filter(|v| !*v).count();
                debug!("{} instances have returned non-zero exit codes.", err_count);
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

    fn make_tempdir(&self, services: &[ReadyService]) -> Result<()> {
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

        let debug = |path: &PathBuf, content: &str| {
            debug!(
                "\nWrote\n-----\n{}\n-----\nto {}.",
                content,
                path.to_string_lossy(),
            );
        };

        fs::write(&self.config.topoinfo_path, &topoinfo).context(format!(
            "failed to write to {:?}",
            &self.config.topoinfo_path
        ))?;
        debug(&self.config.topoinfo_path, &topoinfo);

        fs::write(&self.config.topoinfodashboard_path, &topoinfodashboard).context(format!(
            "failed to write to {:?}",
            &self.config.topoinfodashboard_path
        ))?;
        debug(&self.config.topoinfodashboard_path, &topoinfodashboard);

        Ok(())
    }

    fn make_comms(&self, service_count: usize) -> Result<CommunicationManager> {
        info!("Starting communicationmanager.");
        let comms = CommunicationManager::try_new(&self.config, service_count)?;
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

        let services = services
            .into_iter()
            .filter(|s| s.name() != "dashboard")
            .map(|s| ActiveService::try_new(&self.config, s, &alist))
            .collect::<Vec<Result<ActiveService>>>();

        if services.iter().any(|s| s.is_err()) {
            return Err(Error::msg("Failed to spawn one or more services."));
        }
        Ok(services.into_iter().map(|s| s.unwrap()).collect())
    }

    /// Spawns an `emulationcore` process for each service in the topology,
    /// and returns them as `Child` objects in a `Vec`.
    fn make_ecores(&self) -> Result<Vec<EmulationCore>> {
        info!("Starting emulationcore instances.");
        let ecores = self
            .services
            .as_ref()
            .unwrap()
            .iter()
            .filter(|s| s.name() != "dashboard")
            .map(|s| EmulationCore::try_new(&self.config, s))
            .collect::<Vec<Result<EmulationCore>>>();

        if ecores.iter().any(|e| e.is_err()) {
            return Err(Error::msg(
                "Failed to create one or more emulationcore instances.",
            ));
        }
        let ecores = ecores.into_iter().map(|e| e.unwrap()).collect();

        Ok(ecores)
    }
}
