use std::fs;
use std::path::PathBuf;

use schemars::schema_for;
use worsier_formatter::FormatConfig;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let schema = schema_for!(FormatConfig);
    let json = serde_json::to_string_pretty(&schema)?;
    let package_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packages/npm");
    fs::create_dir_all(&package_dir)?;
    fs::write(
        package_dir.join("configuration_schema.json"),
        format!("{json}\n"),
    )?;
    Ok(())
}
