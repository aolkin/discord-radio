mod json_logger;
mod log_reader;
mod span;
mod throttle;

pub use json_logger::JsonLogger;
pub use log_reader::{LogReadResult, LogReader};
pub(crate) use span::guild_span;
pub use throttle::LogThrottle;

use std::path::{Path, PathBuf};

/// Get the logs directory path for a given base path
pub fn logs_dir(base_path: &Path) -> PathBuf {
    base_path.join("logs")
}

/// Get the logs directory path for a specific guild
pub fn guild_logs_dir(base_path: &Path, guild_id: u64) -> PathBuf {
    logs_dir(base_path).join(guild_id.to_string())
}
