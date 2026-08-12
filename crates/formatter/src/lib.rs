mod config;
mod error;
mod rewriter;

pub use config::{
    FormatConfig, ResolvedConfig, RulesConfig, StatementSpacingConfig, StatementSpacingMode,
    resolve_config,
};
pub use error::FormatError;
pub use rewriter::format_text;

#[cfg(feature = "benchmarking")]
pub use rewriter::{benchmark_parse, benchmark_rewrite, benchmark_verify};
