pub mod docker;
pub mod kubernetes;

use crate::emulationcore::Result;
use crate::graph::Graph;
use crate::orchestrator::docker::DockerOrchestrator;
use crate::orchestrator::kubernetes::KubernetesOrchestrator;

#[derive(Copy, Clone)]
pub enum Orchestrator {
    Docker(DockerOrchestrator),
    Kubernetes(KubernetesOrchestrator),
}

impl Orchestrator {
    pub async fn resolve_hostnames(&self, graph: &mut Graph) -> Result<()> {
        match self {
            Orchestrator::Docker(o) => o.resolve_hostnames(graph).await,
            Orchestrator::Kubernetes(o) => o.resolve_hostnames(graph).await,
        }
    }

    pub async fn start_experiment(&self, id: &str) {
        match self {
            Orchestrator::Docker(o) => o.start_experiment(id).await,
            Orchestrator::Kubernetes(o) => o.start_experiment(id).await,
        }
    }

    // Kill every process in the namespace of the container
    pub async fn stop_experiment(&self, pid: u32, signal_code: SignalCode) {
        match self {
            Orchestrator::Docker(o) => o.stop_experiment(pid, signal_code).await,
            Orchestrator::Kubernetes(o) => o.stop_experiment(pid, signal_code).await,
        }
    }
}

pub enum SignalCode {
    SigInt,
    SigKill,
}
