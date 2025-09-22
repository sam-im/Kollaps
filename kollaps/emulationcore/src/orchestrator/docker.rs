use crate::aux::convert_to_int;
use crate::emulationcore::Result;
use crate::graph::Graph;

use tracing::{error, info};

#[derive(Copy, Clone)]
pub struct DockerOrchestrator;

impl DockerOrchestrator {
    pub async fn resolve_hostnames(&self, graph: &mut Graph) -> Result<()> {
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
}
