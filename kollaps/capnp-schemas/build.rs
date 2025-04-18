fn main() {
    capnpc::CompilerCommand::new()
        .src_prefix("schema")
        .file("schema/message.capnp")
        .run().expect("compiling schema");
}
