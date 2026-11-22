// protoc comes from protoc-bin-vendored, so no system protoc is needed (CI, Docker, laptops).
// An explicit PROTOC env var still wins.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("PROTOC").is_none() {
        // SAFETY: build scripts are single-threaded at this point.
        unsafe { std::env::set_var("PROTOC", protoc_bin_vendored::protoc_bin_path()?) };
    }
    let proto_root = "../proto";
    let protos = ["ts2/v1/chunk_store.proto", "ts2/v1/object_index.proto"];
    for p in protos {
        println!("cargo:rerun-if-changed={proto_root}/{p}");
    }
    tonic_prost_build::configure().compile_protos(&protos, &[proto_root])?;
    Ok(())
}
