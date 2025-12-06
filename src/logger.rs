use chrono::Local;
use std::fs;
use std::sync::Arc;
use std::path::Path;

pub struct Logger {}

pub enum LoggerError {
    CannotCreateFile(String)
}

impl Logger {
    pub fn init() -> Result<(), LoggerError> {
        let log_file_path = Self::log_file();

        if let Some(parent) = Path::new(&log_file_path).parent() {
            fs::create_dir_all(parent)
                .map_err(|_| LoggerError::CannotCreateFile(log_file_path.clone()))?;
        }

        let log_file = fs::File::create(&log_file_path)
            .map_err(|_| LoggerError::CannotCreateFile(log_file_path))?;

        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
            )
            .with_writer(Arc::new(log_file))
            .init();

        Ok(())
    }

    pub fn log_file() -> String {
        let timestamp = Local::now().format("%Y-%m-%d");
        format!("logs/{}.log", timestamp)
    }
}

