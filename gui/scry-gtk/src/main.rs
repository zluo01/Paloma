mod app;
mod helper;
mod runtime;
mod services;
mod style;
mod widgets;

use std::process::ExitCode;

fn main() -> ExitCode {
    app::run()
}
