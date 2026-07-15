use std::{env, error::Error, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    let crate_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let repository = crate_dir.join("..").canonicalize()?;
    let schema = repository.join("schema/provider/main.proto");

    for proto in [
        "schema/provider/codec.proto",
        "schema/provider/common.proto",
        "schema/provider/connection.proto",
        "schema/provider/main.proto",
        "schema/provider/runtime.proto",
    ] {
        println!(
            "cargo:rerun-if-changed={}",
            repository.join(proto).display()
        );
    }

    // SAFETY: build scripts run as isolated processes, before any threads are started.
    unsafe { env::set_var("PROTOC", protoc_bin_vendored::protoc_bin_path()?) };

    // Manually inject serde to protobuf generated struct as currently there is no built-in way
    // need to manually update whenever we update the protobuf schema
    let mut config = prost_build::Config::new();
    const SERDE_DERIVE: &str = "#[derive(serde::Serialize, serde::Deserialize)]";
    for message in [
        "UserPrompt",
        "ConversationMessage",
        "Reasoning",
        "ToolCall",
        "ToolResult",
        "HostedTool",
        "Unknown",
        "MessageContentItem",
        "SummaryItem",
    ] {
        config.type_attribute(format!(".scry.provider.runtime.v1.{message}"), SERDE_DERIVE);
    }
    config.type_attribute(
        ".scry.provider.runtime.v1.ConversationItem.item",
        SERDE_DERIVE,
    );
    config.type_attribute(
        ".scry.provider.runtime.v1.ConversationItem.item",
        "#[serde(tag = \"kind\", rename_all = \"snake_case\")]",
    );

    for field in [
        "ConversationMessage.provider_meta",
        "Reasoning.provider_meta",
        "ToolCall.provider_meta",
        "HostedTool.provider_meta",
        "MessageContentItem.provider_meta",
    ] {
        config.field_attribute(
            format!(".scry.provider.runtime.v1.{field}"),
            "#[serde(default, skip_serializing_if = \"::std::collections::HashMap::is_empty\")]",
        );
    }
    config.field_attribute(
        ".scry.provider.runtime.v1.HostedTool.content",
        "#[serde(default, skip_serializing_if = \"::core::option::Option::is_none\")]",
    );

    config.compile_protos(&[schema], &[repository])?;

    Ok(())
}
