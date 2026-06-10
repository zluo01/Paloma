use std::{collections::HashSet, sync::LazyLock};

pub(crate) static SHELLS: LazyLock<HashSet<&'static str>> =
    LazyLock::new(|| HashSet::from(["bash", "sh", "zsh", "fish", "dash", "ksh"]));
