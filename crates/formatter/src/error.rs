use std::path::Path;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum FormatError {
    #[error("JavaScript or TypeScript parsing failed: {diagnostics}")]
    Parse { diagnostics: String },
    #[error("unsupported source: {message}")]
    UnsupportedSource { message: String },
    #[error("invalid configuration: {message}")]
    InvalidConfig { message: String },
    #[error("formatted output failed semantic verification: {message}")]
    Verification { message: String },
    #[error("internal formatter error: {message}")]
    Internal { message: String },
}

impl FormatError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Parse { .. } => "PARSE_ERROR",
            Self::UnsupportedSource { .. } => "UNSUPPORTED_SOURCE",
            Self::InvalidConfig { .. } => "CONFIG_ERROR",
            Self::Verification { .. } => "VERIFICATION_ERROR",
            Self::Internal { .. } => "INTERNAL_ERROR",
        }
    }

    #[must_use]
    pub fn unsupported_source(path: &Path) -> Self {
        Self::UnsupportedSource {
            message: path.display().to_string(),
        }
    }

    #[must_use]
    pub fn invalid_config(message: impl Into<String>) -> Self {
        Self::InvalidConfig {
            message: message.into(),
        }
    }

    #[must_use]
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal {
            message: message.into(),
        }
    }
}
