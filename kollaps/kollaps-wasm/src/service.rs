use crate::network::Namespace;

pub struct Service<'a> {
    id: String,
    path: String,
    namespace: &'a Namespace
}

impl<'a> Service<'a> {
    // consider try_new with file path and runtime check
    pub fn new(path: String, namespace: &'a Namespace) -> Self {
        let addr = namespace.addr.to_string().replace(".", "-");
        let id = format!("kollaps-wasm-{}", addr);
        Self {
            id,
            path,
            namespace
        }
    }
}
