use tracing_subscriber::{fmt, EnvFilter};

pub(crate) fn build_time() -> &'static str {
    option_env!("FLINGDLNA_BUILD_TIME").unwrap_or("unknown")
}

/// Initialize logging based on build configuration
///
/// Debug builds: Show debug logs from our crates, but filter noisy libraries
/// Release builds: Show only info/warn/error from our crates
pub(crate) fn init_logging() {
    #[cfg(debug_assertions)]
    {
        // Debug build: debug level for our crates, warn for noisy libraries
        let filter = EnvFilter::new("info")
            // Our crates at debug level
            .add_directive("dlna_core=debug".parse().unwrap())
            .add_directive("dlna_server=debug".parse().unwrap())
            .add_directive("dlna_combo=debug".parse().unwrap())
            .add_directive("flingdlna_ffi=debug".parse().unwrap())
            // Quiet down noisy libraries
            .add_directive("mdns_sd=warn".parse().unwrap())
            .add_directive("hyper=warn".parse().unwrap())
            .add_directive("tokio=warn".parse().unwrap())
            .add_directive("tower=warn".parse().unwrap());

        let _ = fmt()
            .with_env_filter(filter)
            .with_target(true)
            .with_thread_ids(false)
            .with_thread_names(false)
            .with_file(false)
            .with_line_number(false)
            .compact()
            .try_init();
    }

    #[cfg(not(debug_assertions))]
    {
        // Release build: only info and above for our crates
        let filter = EnvFilter::new("warn")
            .add_directive("dlna_core=info".parse().unwrap())
            .add_directive("dlna_server=info".parse().unwrap())
            .add_directive("dlna_combo=info".parse().unwrap())
            .add_directive("flingdlna_ffi=info".parse().unwrap());

        let _ = fmt()
            .with_env_filter(filter)
            .with_target(false)
            .with_thread_ids(false)
            .with_thread_names(false)
            .with_file(false)
            .with_line_number(false)
            .compact()
            .try_init();
    }
}
