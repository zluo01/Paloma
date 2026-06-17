//! GTK4/Linux frontend binary.

mod app;
mod runtime;
mod services;
mod style;
mod widgets;

use std::process::ExitCode;

fn main() -> ExitCode {
    app::run()
}
