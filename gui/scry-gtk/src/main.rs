#[cfg(target_os = "linux")]
mod app;
#[cfg(target_os = "linux")]
mod helper;
#[cfg(target_os = "linux")]
mod runtime;
#[cfg(target_os = "linux")]
mod services;
#[cfg(target_os = "linux")]
mod style;
#[cfg(target_os = "linux")]
mod widgets;

use std::process::ExitCode;

#[cfg(target_os = "linux")]
fn main() -> ExitCode {
    app::run()
}

#[cfg(not(target_os = "linux"))]
fn main() -> ExitCode {
    eprintln!("scry (GTK frontend) only supports Linux");
    ExitCode::FAILURE
}
