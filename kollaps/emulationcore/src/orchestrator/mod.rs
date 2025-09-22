pub mod docker;
pub mod kubernetes;

use crate::emulationcore::Result;
use crate::graph::Graph;
use crate::orchestrator::docker::DockerOrchestrator;
use crate::orchestrator::kubernetes::KubernetesOrchestrator;
use tracing::error;

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
            Orchestrator::Docker(_) => start_experiment(id).await,
            Orchestrator::Kubernetes(_) => start_experiment(id).await,
        }
    }

    pub async fn stop_experiment(&self, pid: u32, signal_code: SignalCode) {
        match self {
            Orchestrator::Docker(_) => stop_experiment(pid, signal_code).await,
            Orchestrator::Kubernetes(_) => stop_experiment(pid, signal_code).await,
        }
    }
}

async fn start_experiment(id: &str) {
    use docker_api::Exec;
    use docker_api::opts::{ExecCreateOpts, ExecStartOpts};

    let docker = docker_api::Docker::unix("/var/run/docker.sock");
    let container = docker.containers().get(id);

    let command_string = match get_command_string(id).await {
        Some(cmd_str) => cmd_str.replace('"', "\"").replace("'", "\\'"),
        None => {
            error!("EC: leaving start experiment nothing to do");
            return;
        }
    };

    let mut args = vec![];

    args.push("/bin/sh");
    args.push("-c");

    args.push(&command_string);

    // Create Opts with specified command
    let opts = ExecCreateOpts::builder()
        .command(args)
        .attach_stdout(true)
        .attach_stderr(true)
        .build();

    let exec = Exec::create(docker, &container.id(), &opts).await.unwrap();

    exec.start(&ExecStartOpts::builder().detach(false).build())
        .await
        .unwrap();
}

async fn get_command_string(id: &str) -> Option<String> {
    let docker = docker_api::Docker::unix("/var/run/docker.sock");

    let container = docker.containers().get(id);

    let mut command_string = "".to_string();
    match container.inspect().await {
        Ok(container) => {
            let container_config = container.config.unwrap();

            let image = container_config.image;

            let docker_image = docker.images().get(image.unwrap());

            let mut command = vec![];
            match docker_image.inspect().await {
                Ok(image) => {
                    let image_config = image.config.as_ref();

                    let entrypoint = image_config.unwrap().entrypoint.clone();

                    if entrypoint.is_none() {
                        return None;
                    }

                    if container_config.cmd.is_none() {
                        if !image_config.unwrap().cmd.is_none() {
                            command.extend(image_config.unwrap().cmd.as_ref().unwrap().clone());
                        }
                    } else if container_config.cmd.as_ref().unwrap().len() == 0 {
                        if !image_config.unwrap().cmd.is_none() {
                            command.extend(image_config.unwrap().cmd.as_ref().unwrap().clone());
                        }
                    } else {
                        command.extend(container_config.cmd.as_ref().unwrap().clone());
                    }

                    for string in entrypoint.unwrap() {
                        command_string = format!("{} {}", command_string, string);
                    }
                    for string in command {
                        command_string = format!("{} {}", command_string, string);
                    }
                }
                Err(e) => eprintln!("Error in command string: {}", e),
            }
        }
        Err(e) => eprintln!("Error in command_string: {}", e),
    };

    Some(command_string)
}

pub enum SignalCode {
    SigInt,
    SigKill,
}

// Kill every process in the namespace of the container
async fn stop_experiment(pid: u32, signal_code: SignalCode) {
    use subprocess::Popen;
    use subprocess::PopenConfig;
    use subprocess::Redirection;

    let cmd = match signal_code {
        SignalCode::SigInt => "kill -2 -1",
        SignalCode::SigKill => "kill -9 -1",
    };

    Popen::create(
        &[
            "nsenter",
            "-t",
            &pid.to_string(),
            "-p",
            "-m",
            "/bin/sh",
            "-c",
            cmd,
        ],
        PopenConfig {
            stdout: Redirection::Pipe,
            ..Default::default()
        },
    )
    .unwrap();
}
