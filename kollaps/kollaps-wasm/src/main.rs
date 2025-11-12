mod config;
mod network;
mod runtime;
mod service;

use crate::config::Config;
use crate::service::parse_services;

use libc::{SIGSTOP, execlp, raise};
use network::Bridge;
use service::{ActiveService, ReadyService};

use std::ffi::CString;
use std::fs;
use std::net::Ipv4Addr;
use std::process::{Child, Command};
use std::str::FromStr;

use anyhow::Result;
use tracing::{Level, debug, error, info, subscriber};
use tracing_subscriber::FmtSubscriber;

fn main() -> Result<()> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::DEBUG)
        .finish();
    subscriber::set_global_default(subscriber)?;

    // TODO: consider setting `setpgid` and sending sigkill to all childs as cleanup (check pid1.c)

    // TODO: Parse arguments
    let config = Config::default();

    let services = parse_services(&config)?;

    info!("Creating virtual bridge and namespaces.");
    let mut bridge = Bridge::new("k_wasm", Ipv4Addr::from_str(&config.addr)?, 24);
    let services = services
        .into_iter()
        .map(|s| -> Result<ReadyService> {
            let ns = bridge.create_namespace()?;
            Ok(ReadyService::new(s, ns))
        })
        .map(|s| s.expect("Failed to create network namespaces for services."))
        .collect::<Vec<ReadyService>>();

    setup_tempdir(&config, &services)?;

    info!("Starting communicationmanager.");
    let _commanager = Command::new("communicationmanager")
        .arg(services.len().to_string())
        .spawn()?;

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
                        if let Some(str) = tmp.get(1) {
                            if let Ok(i) = usize::from_str(str) {
                                let c = candidates.get(i).unwrap_or(&candidates[0]);
                                acc.push(c.ns.addr.to_string());
                            }
                        }
                    }
                    acc
                }),
                None => vec![],
            };

            // Using `libc::fork` we can create a child for each service that raises SIGSTOP immediately.
            // This way we can defer the starting of runtimes to emulationcore and still have each runtime's PID.
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
                            s.id, config.runtime_name, res
                        );
                        // Exit without executing RAII guards in child.
                        std::process::exit(-1);
                    }
                    pid if pid < 0 => {
                        panic!("Failed to fork runtime handler, error code: {}", pid);
                    }
                    pid => {
                        info!("Forked a runtime handler, pid: {}.", pid);
                    }
                }
            }
            ActiveService::new(pid, s.clone())
        })
        .collect();

    info!("Starting emulationcore instances.");
    let _ecores: Vec<Child> = services
        .iter()
        .filter(|s| s.name() != "dashboard")
        .map(|s| {
            Command::new("emulationcore")
                .arg(s.id())
                .arg(s.pid().to_string())
                .arg("wasm")
                .arg(s.veth())
                .spawn()
                .expect("Failed to start an emulationcore instance.")
        })
        .collect();

    // TODO: poll the status emulationcores and clean up (send kill to all processes)

    info!("Exiting.");
    Ok(())
}

fn setup_tempdir(config: &Config, services: &Vec<ReadyService>) -> Result<()> {
    info!("Setting up temporary directory.");
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

    let _ = fs::create_dir(&config.tmp_dir);
    let _ = fs::create_dir(format!("{}{}", &config.tmp_dir, "pipes"));
    fs::File::create(format!("{}{}", &config.tmp_dir, &config.remote_ips_path))?;
    fs::write(
        format!("{}{}", &config.tmp_dir, &config.topoinfo_path),
        topoinfo,
    )?;
    fs::write(
        format!("{}{}", &config.tmp_dir, &config.topoinfodashboard_path),
        topoinfodashboard,
    )?;
    Ok(())
}
