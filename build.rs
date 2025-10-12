fn main() {
    // Get the run number from environment variable, default to "dev" if not set
    let run_number = std::env::var("GITHUB_RUN_NUMBER").unwrap_or_else(|_| "dev".to_string());

    // Get the commit hash from environment variable, default to "unknown" if not set
    let commit_hash = std::env::var("GITHUB_SHA").unwrap_or_else(|_| "unknown".to_string());

    // Embed these values into the binary as compile-time constants
    println!("cargo:rustc-env=BUILD_RUN_NUMBER={}", run_number);
    println!("cargo:rustc-env=BUILD_COMMIT_HASH={}", commit_hash);

    // Re-run the build script if these environment variables change
    println!("cargo:rerun-if-env-changed=GITHUB_RUN_NUMBER");
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
}
