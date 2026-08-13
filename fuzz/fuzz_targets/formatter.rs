#![no_main]

use std::path::Path;

use libfuzzer_sys::fuzz_target;
use worsier_formatter::{FormatConfig, FormatError, format_text, resolve_config};

fuzz_target!(|data: &[u8]| {
    let Ok(source) = std::str::from_utf8(data) else {
        return;
    };
    let config = resolve_config(FormatConfig::default()).unwrap();
    for file_name in ["fuzz.js", "fuzz.ts", "fuzz.tsx"] {
        let path = Path::new(file_name);
        let formatted = match format_text(path, source, &config) {
            Ok(formatted) => formatted,
            Err(FormatError::Parse { .. } | FormatError::UnsupportedSource { .. }) => continue,
            Err(error) => panic!("{file_name}: formatting failed: {error}\nsource:\n{source}"),
        };
        if let Some(output) = formatted {
            match format_text(path, &output, &config) {
                Ok(None) => {}
                Ok(Some(second_output)) => panic!(
                    "{file_name}: formatting was not idempotent\nsource:\n{source}\nfirst output:\n{output}\nsecond output:\n{second_output}"
                ),
                Err(error) => panic!(
                    "{file_name}: formatting the first output failed: {error}\nsource:\n{source}\nfirst output:\n{output}"
                ),
            }
        }
    }
});
