use super::SignalCode;
use crate::{emulationcore::Result, graph::Graph};

use std::fs;
use std::str::FromStr;
use std::{net::Ipv4Addr, process::Command};

const KOLLAPS_DIR: &str = "/tmp/kollaps";

#[derive(Copy, Clone)]
pub struct WasmOrchestrator;

impl WasmOrchestrator {
    /// Retrieve the address of each service by parsing it from their `id`.
    pub fn resolve_hostnames(&self, graph: &mut Graph) -> Result<()> {
        let parse_name = |id: &str| -> Option<String> {
            id.split('_').nth_back(1).map_or(None, |s| Some(s.to_string()))
        };
        let parse_addr = |id: &str| -> Option<Ipv4Addr> {
            if let Some(addr) = id.split('_').last() {
                Ipv4Addr::from_str(&addr.replace("-", ".")).ok()
            } else {
                None
            }
        };

        let mut service_ids = fs::read_to_string(format!("{}{}", KOLLAPS_DIR, "topoinfo"))?;
        let dashboard_id = fs::read_to_string(format!("{}{}", KOLLAPS_DIR, "topoinfodashboard"))?;
        service_ids.push_str(&dashboard_id);

        let service_ids = service_ids.split(|c| c == '\n').collect::<Vec<&str>>();
        let name_addr_pair: Vec<(String, Ipv4Addr)> = service_ids
            .iter()
            .filter_map(|id| match id.is_empty() {
                true => None,
                false => Some(id.to_string()),
            })
            .map(|id| {
                let name = parse_name(&id).expect("failed to parse name of service");
                let addr = parse_addr(&id).expect("failed to parse address of service");
                (name, addr)
            })
            .collect();

        for (name, services) in &graph.services_by_name {
            let addrs = &name_addr_pair
                .iter()
                .filter(|(n, _)| n == name)
                .map(|(_, a)| a.clone())
                .collect::<Vec<Ipv4Addr>>();

            assert_eq!(addrs.len(), services.len());
            services.iter().zip(addrs).enumerate().for_each(|(i, (serv, addr))| {
                let addr_u32 = u32::from_be_bytes(addr.octets());
                serv.blocking_lock().ip = addr_u32;
                serv.blocking_lock().replica_id = i;
                graph.services.insert(addr_u32, serv.clone());
                graph.ips.push(addr_u32);
            });
        }
        Ok(())
    }
    pub fn start_experiment(&self, id: &str) {
        // TODO: send sigcont to runtime
        todo!()
    }
    pub fn stop_experiment(&self, pid: u32, signal: SignalCode) {
        let cmd = match signal {
            SignalCode::SigInt => "-2",
            SignalCode::SigKill => "-9",
        };

        let _ = Command::new("kill").arg(cmd).arg(pid.to_string()).spawn();
    }
}
