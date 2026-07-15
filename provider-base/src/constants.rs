use std::{collections::BTreeMap, sync::LazyLock};

pub static ENVIRONMENT_CONTEXT: LazyLock<BTreeMap<&'static str, String>> =
    LazyLock::new(build_environment_context);

fn build_environment_context() -> BTreeMap<&'static str, String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "unknown".into());
    let home = std::env::var("HOME").unwrap_or_else(|_| "unknown".into());

    BTreeMap::from([
        ("os", std::env::consts::OS.to_string()),
        ("os_family", std::env::consts::FAMILY.to_string()),
        ("arch", std::env::consts::ARCH.to_string()),
        ("home", home),
        ("shell", shell),
    ])
}
