use crate::network::Namespace;

#[derive(Debug)]
pub struct Service {
    name: String,
    image: String,
    command: Option<String>,
}

impl Service {
    pub fn new(name: String, image: String, command: Option<String>) -> Self {
        Self {
            name,
            image,
            command,
        }
    }
}

struct ActiveService {
    id: String,
    ns_name: String,
    service: Service,
}

impl ActiveService {
    fn new(service: Service, ns: &Namespace) -> Self {
        // Encoding address into the ID.
        let addr = ns.addr.to_string().replace(".", "-");
        let id = format!("kollaps_wasm_{}", addr);
        let ns_name = ns.name.clone();
        Self {
            id,
            ns_name,
            service,
        }
    }
}
