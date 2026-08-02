#[cfg(windows)]
mod app;
#[cfg(windows)]
mod convert;
#[cfg(windows)]
mod service;
#[cfg(windows)]
mod transport;

use std::process::ExitCode;

#[cfg(windows)]
fn main() -> ExitCode {
    app::run()
}

#[cfg(not(windows))]
fn main() -> ExitCode {
    eprintln!("paloma-core (gRPC service) only supports Windows");
    ExitCode::FAILURE
}
