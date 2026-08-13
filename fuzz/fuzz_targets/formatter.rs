#![no_main]

use std::path::Path;

use libfuzzer_sys::fuzz_target;
use worsier_formatter::{FormatConfig, format_text, resolve_config};

fuzz_target!(|data: &[u8]| {
    let Ok(source) = std::str::from_utf8(data) else {
        return;
    };
    let config = resolve_config(FormatConfig::default()).unwrap();
    for file_name in ["fuzz.js", "fuzz.ts", "fuzz.tsx"] {
        let path = Path::new(file_name);
        let Ok(formatted) = format_text(path, source, &config) else {
            continue;
        };
        if let Some(output) = formatted {
            assert!(format_text(path, &output, &config).unwrap().is_none());
        }
    }
});
