use std::{env, error::Error, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    let crate_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let repository = crate_dir.join("..").canonicalize()?;
    let schema = repository.join("schema/extension/main.proto");

    for proto in [
        "schema/extension/capability.proto",
        "schema/extension/tool.proto",
        "schema/extension/main.proto",
    ] {
        println!(
            "cargo:rerun-if-changed={}",
            repository.join(proto).display()
        );
    }

    prost_build::Config::new()
        .protoc_executable(protoc_bin_vendored::protoc_bin_path()?)
        .compile_protos(&[schema], &[repository])?;

    Ok(())
}
