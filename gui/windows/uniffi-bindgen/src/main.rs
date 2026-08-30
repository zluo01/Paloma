#[cfg(windows)]
fn main() -> std::process::ExitCode {
    match uniffi_bindgen_cs::main() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e:#}");
            std::process::ExitCode::FAILURE
        },
    }
}

#[cfg(not(windows))]
fn main() -> std::process::ExitCode {
    eprintln!("uniffi-bindgen-cs is only used for the windows frontend");
    std::process::ExitCode::FAILURE
}
