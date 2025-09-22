use crate::emulationcore::Result;
use crate::{aux::convert_to_int, graph::Graph};

#[derive(Clone, Copy)]
pub struct KubernetesOrchestrator;

impl KubernetesOrchestrator {
    pub async fn resolve_hostnames(&self, graph: &mut Graph) -> Result<()> {
        use k8s_openapi::api::core::v1::Pod;
        use kube::{
            Client,
            api::{Api, ListParams, ResourceExt},
        };
        use std::{
            net::{IpAddr, Ipv4Addr},
            str::FromStr,
            sync::Arc,
        };

        let mut services_by_name = graph.services_by_name.clone();
        for (name, services) in services_by_name.iter_mut() {
            let mut ips: Vec<Ipv4Addr>;
            loop {
                ips = vec![];
                let client = Client::try_default().await?;

                let pods: Api<Pod> = Api::default_namespaced(client);
                for p in pods.list(&ListParams::default()).await? {
                    let key = "KOLLAPS_UUID";
                    let uuid = match std::env::var(key) {
                        Ok(val) => Some(val),
                        Err(_e) => None,
                    };
                    let name = format!("{}-{}", name, uuid.as_ref().unwrap());
                    if p.name().starts_with(&name) {
                        let ip_string = p.status.unwrap().pod_ip;

                        if ip_string.is_none() {
                            continue;
                        }
                        let ip_string = ip_string.unwrap();

                        let ip = IpAddr::from_str(&ip_string).unwrap();
                        match ip {
                            IpAddr::V4(ipv4) => {
                                ips.push(ipv4);
                            }
                            IpAddr::V6(_ipv6) => break,
                        }
                    }
                }
                if ips.len() == services.len() {
                    break;
                }

                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            }
            ips.sort();
            for (i, service) in services.iter().enumerate() {
                let int_ip = convert_to_int(ips[i].octets());
                service.lock().await.ip = int_ip;
                service.lock().await.replica_id = i;
                graph.services.insert(int_ip, Arc::clone(service));
            }
        }

        Ok(())
    }
}
