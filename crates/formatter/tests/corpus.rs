use std::fs;
use std::path::PathBuf;

use worsier_formatter::{FormatConfig, format_text, resolve_config};

#[test]
fn committed_fuzz_corpus_formats_successfully_and_idempotently() {
    let corpus = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/formatter");
    let config = resolve_config(FormatConfig::default()).unwrap();
    let mut entries = fs::read_dir(corpus)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    entries.sort();
    assert!(!entries.is_empty());

    for path in entries {
        let source = fs::read_to_string(&path).unwrap();
        let output = format_text(&path, &source, &config).unwrap();
        let formatted = output.as_deref().unwrap_or(&source);
        assert!(
            format_text(&path, formatted, &config).unwrap().is_none(),
            "{} was not idempotent",
            path.display()
        );
    }
}
