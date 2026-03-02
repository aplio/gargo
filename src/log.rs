use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

macro_rules! debug_log {
    ($config:expr, $($arg:tt)*) => {
        if $config.debug {
            crate::log::write_log(&$config.debug_log_path, &format!($($arg)*));
        }
    };
}
pub(crate) use debug_log;

pub fn write_log(path: &Path, msg: &str) {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let _ = writeln!(file, "[{}] {}", timestamp, msg);
}
