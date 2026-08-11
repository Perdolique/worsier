#![no_main]

use std::path::Path;

use libfuzzer_sys::fuzz_target;
use worsier_formatter::{FormatConfig, format_text, resolve_config};

fuzz_target!(|data: &[u8]| {
    let Ok(source) = std::str::from_utf8(data) else {
        return;
    };
    let config = resolve_config(FormatConfig::default()).unwrap();
    let Ok(formatted) = format_text(Path::new("fuzz.tsx"), source, &config) else {
        return;
    };
    if let Some(output) = formatted {
        assert!(
            format_text(Path::new("fuzz.tsx"), &output, &config)
                .unwrap()
                .is_none()
        );
    }
});
