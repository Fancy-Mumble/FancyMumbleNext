//! Build script for the `mumble-protocol` crate.
//!
//! Invokes `prost-build` to compile the Mumble protobuf definitions into Rust
//! source files that are written to `src/proto/`.
use std::io::Result;

fn main() -> Result<()> {
    println!("cargo:rerun-if-changed=proto/Mumble.proto");
    println!("cargo:rerun-if-changed=proto/MumbleUDP.proto");

    prost_build::Config::new()
        .out_dir("src/proto")
        .compile_protos(&["proto/Mumble.proto", "proto/MumbleUDP.proto"], &["proto/"])?;
    Ok(())
}
