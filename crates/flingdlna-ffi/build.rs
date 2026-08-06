use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    println!("cargo:rerun-if-env-changed=FLINGDLNA_BUILD_TIME");
    let build_time = std::env::var("FLINGDLNA_BUILD_TIME").unwrap_or_else(|_| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs().to_string())
            .unwrap_or_else(|_| "unknown".to_string())
    });

    println!("cargo:rustc-env=FLINGDLNA_BUILD_TIME={build_time}");
}
