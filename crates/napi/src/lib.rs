use std::path::PathBuf;

use napi::bindgen_prelude::{AsyncTask, Env, Task};
use napi::{Error, Result, Status};
use napi_derive::napi;
use worsier_formatter::{FormatConfig, format_text, resolve_config};

pub struct FormatTask {
    file_name: PathBuf,
    source_text: String,
    config_json: String,
}

impl Task for FormatTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<Self::Output> {
        let mut deserializer = serde_json::Deserializer::from_str(&self.config_json);
        let raw: FormatConfig = serde_path_to_error::deserialize(&mut deserializer)
            .map_err(|error| native_error("CONFIG_ERROR", error))?;
        let config = resolve_config(raw).map_err(format_error)?;
        let output =
            format_text(&self.file_name, &self.source_text, &config).map_err(format_error)?;
        Ok(output.unwrap_or_else(|| self.source_text.clone()))
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

#[napi]
#[must_use]
pub fn format(
    file_name: String,
    source_text: String,
    config_json: String,
) -> AsyncTask<FormatTask> {
    AsyncTask::new(FormatTask {
        file_name: PathBuf::from(file_name),
        source_text,
        config_json,
    })
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
