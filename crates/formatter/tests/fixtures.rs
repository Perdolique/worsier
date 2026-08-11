use std::fs;
use std::path::{Path, PathBuf};

use worsier_formatter::{FormatConfig, format_text, resolve_config};

#[test]
fn fixtures_match_expected_output_and_are_idempotent() {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut inputs = fs::read_dir(&directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| !path.to_string_lossy().contains(".expected."))
        .collect::<Vec<_>>();
    inputs.sort();
    assert!(!inputs.is_empty());

    let config = resolve_config(FormatConfig::default()).unwrap();
    for input_path in inputs {
        assert_fixture(&input_path, &config);
    }
}

fn assert_fixture(input_path: &Path, config: &worsier_formatter::ResolvedConfig) {
    let source = fs::read_to_string(input_path).unwrap();
    let expected_path = expected_path(input_path);
    let expected = fs::read_to_string(&expected_path).unwrap();
    let actual = format_text(input_path, &source, config)
        .unwrap()
        .unwrap_or(source);

    assert_eq!(actual, expected, "fixture {}", input_path.display());
    assert!(
        format_text(input_path, &actual, config).unwrap().is_none(),
        "fixture {} is not idempotent",
        input_path.display()
    );
}

fn expected_path(input: &Path) -> PathBuf {
    let extension = input.extension().unwrap().to_string_lossy();
    input.with_extension(format!("expected.{extension}"))
}
