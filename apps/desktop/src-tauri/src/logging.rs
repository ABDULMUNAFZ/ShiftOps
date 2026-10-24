//! Redacted application log at `<log dir>/shiftops.log`, rotated at 5 MiB
//! to `shiftops.log.1`. Every line passes through `redact` first. Before
//! `init` is called (and in unit tests) logging is a no-op.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::redact::redact;
use crate::time_util;

pub const LOG_FILE_NAME: &str = "shiftops.log";
const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;

struct Logger {
    path: PathBuf,
    file: Mutex<Option<File>>,
}

static LOGGER: OnceLock<Logger> = OnceLock::new();

pub fn init(log_dir: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(log_dir)?;
    let path = log_dir.join(LOG_FILE_NAME);
    let file = open(&path)?;
    let _ = LOGGER.set(Logger {
        path: path.clone(),
        file: Mutex::new(Some(file)),
    });
    Ok(path)
}

fn open(path: &Path) -> std::io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

fn write(level: &str, target: &str, message: &str) {
    let Some(logger) = LOGGER.get() else { return };
    let line = format!(
        "{} {level:<5} {target} {}\n",
        time_util::now(),
        redact(message).replace('\n', "\\n")
    );
    let Ok(mut guard) = logger.file.lock() else {
        return;
    };
    let too_big = std::fs::metadata(&logger.path)
        .map(|m| m.len() > MAX_LOG_BYTES)
        .unwrap_or(false);
    if too_big {
        *guard = None;
        let _ = std::fs::rename(&logger.path, logger.path.with_extension("log.1"));
        *guard = open(&logger.path).ok();
    }
    if let Some(file) = guard.as_mut() {
        let _ = file.write_all(line.as_bytes());
    }
}

pub fn info(target: &str, message: &str) {
    write("INFO", target, message);
}

pub fn warn(target: &str, message: &str) {
    write("WARN", target, message);
}

pub fn error(target: &str, message: &str) {
    write("ERROR", target, message);
}
