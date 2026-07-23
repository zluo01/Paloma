pub fn init_logging(filter: String) {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(filter))
        .target(env_logger::Target::Stderr)
        .try_init()
        .ok();
}
