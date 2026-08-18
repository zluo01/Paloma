mod delete;
mod exec;

use std::io;

use paloma_extension_base::{Capability, ExtensionService};
use paloma_utils::init_logging;

pub use crate::exec::{CAPABILITY_ID, ExecArgs};
use crate::{delete::DeleteFiles, exec::Exec};

pub const EXTENSION_ID: &str = "Shell";

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
    let capabilities: Vec<Box<dyn Capability>> =
        vec![Box::new(Exec::new()), Box::new(DeleteFiles::new())];

    Ok(ExtensionService::new(
        EXTENSION_ID,
        "Execute shell commands and move files to the trash.",
        None,
        None,
        capabilities,
    ))
}
