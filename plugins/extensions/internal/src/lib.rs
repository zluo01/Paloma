mod app_search;
mod calculator;
mod clipboard;
mod file_search;

use std::io;

use paloma_extension_base::{Capability, ExtensionService};
use paloma_utils::init_logging;

use crate::{
    app_search::AppSearch, calculator::Calculator, clipboard::Clipboard, file_search::FileSearch,
};

pub fn run() -> io::Result<()> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?
        .block_on(async {
            init_logging("info".into());
            service()?.serve().await
        })
}

fn service() -> io::Result<ExtensionService> {
    let capabilities: Vec<Box<dyn Capability>> = vec![
        Box::new(Calculator),
        Box::new(Clipboard::new()),
        Box::new(FileSearch::new().map_err(io::Error::other)?),
        Box::new(AppSearch::new().map_err(io::Error::other)?),
    ];

    Ok(ExtensionService::new(
        "Internal",
        "Built-in launcher capabilities.",
        None,
        None,
        capabilities,
    ))
}
