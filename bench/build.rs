use std::path::PathBuf;
use std::process::Command;

fn main() {
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    println!("cargo:rerun-if-changed=schema");
    capnpc::CompilerCommand::new().src_prefix("schema").file("schema/rows.capnp").run().expect("capnp compile");
    let status = Command::new("flatc").arg("--rust").arg("-o").arg(&out).arg("schema/rows.fbs").status().expect("flatc not found");
    assert!(status.success(), "flatc failed");
}
