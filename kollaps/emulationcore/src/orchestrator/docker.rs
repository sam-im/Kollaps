use super::SignalCode;
use crate::emulationcore::Result;
use crate::graph::Graph;

use tracing::{error, info};

#[derive(Copy, Clone)]
pub struct DockerOrchestrator;

impl DockerOrchestrator {
    pub async fn resolve_hostnames(&self, graph: &mut Graph) -> Result<()> {
        use crate::aux::convert_to_int;
        use hickory_client::client::{Client, ClientHandle};
        use hickory_client::proto::{
            rr::{DNSClass, Name, RecordType},
            runtime::TokioRuntimeProvider,
            tcp::TcpClientStream,
        };
        use std::{
            env,
            net::{IpAddr, Ipv4Addr, SocketAddr},
            str::FromStr,
            sync::Arc,
        };
        use tokio::time::{Duration, sleep};

        let sleeptime = Duration::from_millis(500);
        let (stream, sender) = TcpClientStream::new(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 11)), 53),
            None,
            None,
            TokioRuntimeProvider::new(),
        );
        let client = Client::new(stream, sender, None);

        let (mut client, bg) = client.await.expect("connection failed");
        let bg_handle = tokio::spawn(bg);

        for (name, services) in graph.services_by_name.iter_mut() {
            let mut ips: Vec<Ipv4Addr>;
            loop {
                ips = vec![];

                let key = "KOLLAPS_UUID";
                let uuid = match env::var(key) {
                    Ok(val) => Some(val),
                    Err(_e) => None,
                };

                let query = client.query(
                    Name::from_str(format!("{}-{}", name, uuid.unwrap()).as_str()).unwrap(),
                    DNSClass::IN,
                    RecordType::A,
                );

                let response = query.await;

                match response {
                    Ok(res) => {
                        res.answers()
                            .iter()
                            .map(|res| res.data().ip_addr())
                            .filter_map(|ip| {
                                if let Some(IpAddr::V4(ipv4)) = ip {
                                    info!("Address is {}", ipv4);
                                    Some(ipv4)
                                } else {
                                    None
                                }
                            })
                            .for_each(|ipv4| ips.push(ipv4));
                    }
                    Err(e) => {
                        error!("Error: {}", e);
                        sleep(sleeptime).await;
                    }
                };
                info!(
                    "IPS len is {} and services len is {} for name {}",
                    ips.len(),
                    services.len(),
                    name.clone()
                );

                if ips.len() == services.len() {
                    break;
                }
                sleep(sleeptime).await;
            }
            ips.sort();

            for (i, service) in services.iter().enumerate() {
                let int_ip = convert_to_int(ips[i].octets());
                service.lock().await.ip = int_ip;
                service.lock().await.replica_id = i;
                graph.services.insert(int_ip, Arc::clone(service));
                graph.ips.push(int_ip);
            }
        }
        bg_handle.abort();
        Ok(())
    }

    pub async fn start_experiment(&self, id: &str) {
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

    pub async fn stop_experiment(&self, pid: u32, signal_code: SignalCode) {
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
