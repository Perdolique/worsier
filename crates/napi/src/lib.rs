use std::path::PathBuf;

use napi::bindgen_prelude::spawn_blocking;
use napi::{Error, Result, Status};
use napi_derive::napi;
use worsier_formatter::{FormatConfig, format_text, resolve_config};

#[napi]
/// Formats source text on Tokio's blocking pool rather than Node's shared libuv pool.
///
/// # Errors
///
/// Rejects with a stable Worsier error code when configuration, parsing, verification, or the
/// background worker fails.
pub async fn format(file_name: String, source_text: String, config_json: String) -> Result<String> {
    spawn_blocking(move || format_in_worker(file_name, source_text, &config_json))
        .await
        .map_err(|error| native_error("INTERNAL_ERROR", error))?
}

fn format_in_worker(file_name: String, source_text: String, config_json: &str) -> Result<String> {
    let mut deserializer = serde_json::Deserializer::from_str(config_json);
    let raw: FormatConfig = serde_path_to_error::deserialize(&mut deserializer)
        .map_err(|error| native_error("CONFIG_ERROR", error))?;
    let config = resolve_config(raw).map_err(format_error)?;
    let file_name = PathBuf::from(file_name);
    let output = format_text(&file_name, &source_text, &config).map_err(format_error)?;
    Ok(output.unwrap_or(source_text))
}

#[napi]
#[allow(
    clippy::unused_async,
    reason = "the NAPI launcher intentionally exposes a Promise-returning CLI entrypoint"
)]
pub async fn run_cli(args: Vec<String>) -> i32 {
    worsier_cli::run(std::iter::once("worsier".to_owned()).chain(args))
}

fn format_error(error: worsier_formatter::FormatError) -> Error {
    native_error(error.code(), error)
}

fn native_error(code: &str, error: impl std::fmt::Display) -> Error {
    Error::new(Status::GenericFailure, format!("[{code}] {error}"))
}
