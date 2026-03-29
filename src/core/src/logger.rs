use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use chrono::Utc;

const MAX_LOG_SIZE: u64 = 5 * 1024 * 1024; // 5 MB

pub trait LogEmitter: Send + Sync {
    fn emit(&self, entry: &LogEntry);
}

pub struct NoopEmitter;
impl LogEmitter for NoopEmitter {
    fn emit(&self, _entry: &LogEntry) {}
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub level: String,
    pub source: String,
    pub message: String,
    pub timestamp: String,
}

struct Inner {
    log_file_path: Option<PathBuf>,
    emitter: Arc<dyn LogEmitter>,
}

#[derive(Clone)]
pub struct Logger {
    inner: Arc<Mutex<Inner>>,
}

impl Logger {
    pub fn new(user_data_path: impl Into<PathBuf>, emitter: Arc<dyn LogEmitter>) -> Self {
        let path = user_data_path.into().join("mediaharbor.log");
        let logger = Self {
            inner: Arc::new(Mutex::new(Inner {
                log_file_path: Some(path),
                emitter,
            })),
        };
        logger.rotate_if_needed();
        logger.info("system", "Logger initialized");
        logger
    }

    pub fn headless(emitter: Arc<dyn LogEmitter>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                log_file_path: None,
                emitter,
            })),
        }
    }

    pub fn info(&self, source: &str, message: &str) {
        self.write("info", source, message);
    }

    pub fn warn(&self, source: &str, message: &str) {
        self.write("warn", source, message);
    }

    pub fn error(&self, source: &str, message: &str) {
        self.write("error", source, message);
    }

    fn write(&self, level: &str, source: &str, message: &str) {
        let timestamp = Utc::now().to_rfc3339();
        let line = format!(
            "[{}] [{}] [{}] {}\n",
            timestamp,
            level.to_uppercase(),
            source,
            message
        );

        let guard = self.inner.lock().unwrap();

        if let Some(ref path) = guard.log_file_path {
            let _ = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .and_then(|mut f| f.write_all(line.as_bytes()));
        }

        let entry = LogEntry {
            level: level.to_string(),
            source: source.to_string(),
            message: message.to_string(),
            timestamp: timestamp.clone(),
        };
        guard.emitter.emit(&entry);
    }

    fn rotate_if_needed(&self) {
        let guard = self.inner.lock().unwrap();
        if let Some(ref path) = guard.log_file_path {
            if let Ok(meta) = fs::metadata(path) {
                if meta.len() > MAX_LOG_SIZE {
                    if let Ok(content) = fs::read_to_string(path) {
                        let lines: Vec<&str> = content.lines().collect();
                        let half = lines[lines.len() / 2..].join("\n");
                        let _ = fs::write(path, half);
                    }
                }
            }
        }
    }
}

impl Default for Logger {
    fn default() -> Self {
        Self::headless(Arc::new(NoopEmitter))
    }
}
