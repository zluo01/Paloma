#[cfg(target_os = "macos")]
fn main() {
    uniffi::uniffi_bindgen_swift()
}

#[cfg(not(target_os = "macos"))]
fn main() -> std::process::ExitCode {
    eprintln!("uniffi-bindgen-swift is only used for the macos frontend");
    std::process::ExitCode::FAILURE
}
