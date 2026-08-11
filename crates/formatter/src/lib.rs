mod asi;
mod comments;
mod config;
mod doc;
mod error;
mod index;
mod precedence;

pub use config::{
    ArrayConfig, ArrayObjectLayout, ArrowParentheses, CollectionItemLayout, CollectionLayout,
    FormatConfig, ImportConfig, IndentStyle, LineEnding, LineShape, ObjectArrayLayout,
    ObjectConfig, QuoteStyle, ResolvedConfig, Semicolons, StatementKind, StatementScope,
    StatementSelector, StatementSpacingRule, TrailingCommas, resolve_config,
};
pub use error::FormatError;

mod printer;

pub use printer::format_text;

#[cfg(feature = "benchmarking")]
pub use printer::{PreparedDocument, benchmark_index, benchmark_parse, prepare_document};
