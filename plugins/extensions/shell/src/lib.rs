mod exec;

use std::io;

use paloma_extension_base::{Capability, ExtensionService};
use paloma_utils::init_logging;

use crate::exec::Shell;
pub use crate::exec::{CAPABILITY_ID, ShellArgs};

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
    let capabilities: Vec<Box<dyn Capability>> = vec![Box::new(Shell::new())];

    Ok(ExtensionService::new(
        EXTENSION_ID,
        "Execute shell commands.",
        None,
        None,
        capabilities,
    ))
}
