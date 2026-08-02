use std::{env, error::Error, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    let crate_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let repository = crate_dir.parent().ok_or("crate dir has no parent")?;
    let schema = repository.join("schema/binding/main.proto");

    for proto in [
        "schema/binding/common.proto",
        "schema/binding/connector.proto",
        "schema/binding/main.proto",
        "schema/binding/plugin.proto",
        "schema/binding/render.proto",
        "schema/binding/session.proto",
    ] {
        println!(
            "cargo:rerun-if-changed={}",
            repository.join(proto).display()
        );
    }

    let mut config = prost_build::Config::new();
    config.protoc_executable(protoc_bin_vendored::protoc_bin_path()?);
    config.extern_path(".paloma.extension.v1", "::paloma_extension_protocol::v1");
    config.extern_path(
        ".paloma.provider.runtime.v1",
        "::paloma_provider_protocol::v1",
    );

    tonic_prost_build::configure().compile_with_config(
        config,
        &[schema],
        &[repository.to_path_buf()],
    )?;

    Ok(())
}
