use std::fs;
use std::path::PathBuf;

fn main() {
    let proto_dir = PathBuf::from("../../proto");

    // Read all .proto files in the directory
    let mut proto_files = Vec::new();
    if let Ok(entries) = fs::read_dir(&proto_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("proto") {
                proto_files.push(path);
            }
        }
    }

    if proto_files.is_empty() {
        println!("cargo:warning=No .proto files found in ../../proto");
        return;
    }

    // Tell cargo to recompile if the .proto files change
    for file in &proto_files {
        println!("cargo:rerun-if-changed={}", file.display());
    }

    let mut config = prost_build::Config::new();
    // Use bytes::Bytes for string fields which handles zero-copy decoding
    config.bytes(["."]);

    config
        .compile_protos(&proto_files, &[proto_dir])
        .unwrap_or_else(|e| panic!("Failed to compile protos: {}", e));
}
