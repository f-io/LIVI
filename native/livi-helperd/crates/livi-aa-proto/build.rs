use std::path::{Path, PathBuf};

fn main() {
    let root = Path::new("proto");
    println!("cargo:rerun-if-changed=proto");
    let mut files: Vec<PathBuf> = std::fs::read_dir(root)
        .expect("proto dir")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "proto"))
        .collect();
    files.sort();
    let fds = protox::compile(&files, [root]).expect("the proto tree compiles");
    prost_build::Config::new().compile_fds(fds).expect("prost generates the messages");
}
