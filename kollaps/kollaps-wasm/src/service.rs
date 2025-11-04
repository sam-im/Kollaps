use std::process::Child;

use crate::network::Namespace;

#[derive(Debug)]
pub struct Service {
    pub name: String,
    pub image: String,
    pub command: Option<String>,
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

pub struct ReadyService {
    pub id: String,
    pub ns: Namespace,
    pub service: Service,
}

impl ReadyService {
    pub fn new(service: Service, ns: Namespace) -> Self {
        // Replace '_' by another character as we use to parse the id.
        let name = service.name.replace("_", "-");
        let addr = ns.addr.to_string().replace(".", "-");
        let id = format!("wasm_{}_{}", name, addr);
        Self { id, ns, service }
    }
}

pub struct ActiveService {
    pub handle: Child,
    pub service: ReadyService,
}

impl ActiveService {
    pub fn new(handle: Child, service: ReadyService) -> Self {
        Self { handle, service }
    }
    pub fn pid(&self) -> u32 {
        self.handle.id()
    }
    pub fn id(&self) -> &String {
        &self.service.id
    }
    pub fn name(&self) -> &String {
        &self.service.service.name
    }
    pub fn veth(&self) -> &String {
        &self.service.ns.veth
    }
}
