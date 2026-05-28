use std::collections::HashSet;
use std::path::Path;
use std::sync::LazyLock;

pub(crate) static SHELLS: LazyLock<HashSet<&'static str>> =
    LazyLock::new(|| HashSet::from(["bash", "sh", "zsh", "fish", "dash", "ksh"]));

pub(crate) fn is_supported_shell(shell: &str) -> bool {
    let name = Path::new(shell)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(shell);
    SHELLS.contains(name)
}
