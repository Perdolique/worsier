mod config;
mod document;
mod embedded;
mod error;
mod rewriter;
mod vue;

pub use config::{
    FormatConfig, InterfaceLayoutMode, InterfaceLayoutRule, QuoteStyle, ResolvedConfig,
    RulesConfig, SemicolonConfig, SemicolonMode, SingleLineCallStatementSpacingConfig,
    SingleLineCallStatementSpacingRule, StatementSpacingConfig, StatementSpacingMode,
    TrailingCommaMode, TypeMemberSemicolonConfig, TypeMemberSemicolonRule, resolve_config,
};
pub use document::{format_text, is_supported_path};
pub use error::FormatError;

#[cfg(feature = "benchmarking")]
pub use rewriter::{benchmark_parse, benchmark_rewrite, benchmark_verify};
