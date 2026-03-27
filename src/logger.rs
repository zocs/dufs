use anyhow::{Context, Result};
use log::{Level, LevelFilter, Metadata, Record};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// Global file handle — can be updated between start/stop cycles.
static LOG_FILE: OnceLock<Mutex<Option<std::fs::File>>> = OnceLock::new();

struct SimpleLogger;

impl log::Log for SimpleLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        // Since v0.46.0 the http logger composes the full line (timestamp,
        // level, remote addr, request, status — per --log-format), so write
        // record.args() verbatim instead of prepending another prefix here.
        let text = record.args().to_string();

        if let Some(guard) = LOG_FILE.get() {
            if let Ok(mut file_opt) = guard.lock() {
                if let Some(ref mut file) = *file_opt {
                    let _ = writeln!(file, "{text}");
                    return;
                }
            }
        }
        // Fallback: no log file configured — print to stdout/stderr by level.
        if record.level() < Level::Info {
            eprintln!("{text}");
        } else {
            println!("{text}");
        }
    }

    fn flush(&self) {}
}

pub fn init(log_file: Option<PathBuf>) -> Result<()> {
    // Open/replace the file handle (can be called multiple times)
    let file_handle = match log_file {
        None => None,
        Some(ref path) => {
            let f = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .with_context(|| format!("Failed to open the log file at '{}'", path.display()))?;
            Some(f)
        }
    };

    // Update or initialize the global file slot
    let slot = LOG_FILE.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = slot.lock() {
        *guard = file_handle; // drops old file handle
    }

    // Set the global logger (only succeeds once per process)
    let _ = log::set_boxed_logger(Box::new(SimpleLogger))
        .map(|_| log::set_max_level(LevelFilter::Info));

    Ok(())
}
