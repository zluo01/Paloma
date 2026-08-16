#[cfg(target_os = "macos")]
fn main() {
    uniffi::uniffi_bindgen_main()
}

#[cfg(not(target_os = "macos"))]
fn main() -> std::process::ExitCode {
    eprintln!("uniffi-bindgen is only used for the macos frontend");
    std::process::ExitCode::FAILURE
}
