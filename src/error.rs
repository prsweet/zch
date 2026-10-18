use std::{fmt, path::PathBuf};

#[derive(Debug)]
pub enum AppError {
    ParseError(String),
    FileNotFound(PathBuf),
    Io(String),
    UnsupportedPlatform(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::ParseError(msg) => write!(f, "failed to parse: {msg}"),
            AppError::FileNotFound(path) => write!(f, "file not found: {}", path.display()),
            AppError::Io(msg) => write!(f, "io error: {msg}"),
            AppError::UnsupportedPlatform(url) => write!(f, "unsupported platform: {url}"),
        }
    }
}

impl std::error::Error for AppError {}
