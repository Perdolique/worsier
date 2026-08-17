use std::path::Path;

use oxc_span::SourceType;

use crate::embedded::{EmbeddedRegion, validate_regions};
use crate::{FormatError, ResolvedConfig, rewriter, vue};

#[derive(Clone, Copy, Debug)]
enum DocumentKind {
    Script(SourceType),
    Embedded(EmbeddedKind),
}

#[derive(Clone, Copy, Debug)]
enum EmbeddedKind {
    Vue,
}

impl DocumentKind {
    fn from_path(path: &Path) -> Option<Self> {
        if path.extension().and_then(std::ffi::OsStr::to_str) == Some("vue") {
            return Some(Self::Embedded(EmbeddedKind::Vue));
        }
        script_source_type(path).map(Self::Script)
    }
}

/// Returns whether a path has a source type supported by Worsier.
#[must_use]
pub fn is_supported_path(path: &Path) -> bool {
    DocumentKind::from_path(path).is_some()
}

/// Formats a supported script or embedded document.
///
/// # Errors
///
/// Returns a [`FormatError`] when the source type is unsupported, parsing fails, an embedded
/// adapter violates its range contract, or semantic verification detects a changed AST.
pub fn format_text(
    file_name: &Path,
    source_text: &str,
    config: &ResolvedConfig,
) -> Result<Option<String>, FormatError> {
    match DocumentKind::from_path(file_name) {
        Some(DocumentKind::Script(source_type)) => {
            rewriter::format_script(file_name, source_text, source_type, None, config)
        }
        Some(DocumentKind::Embedded(EmbeddedKind::Vue)) => {
            let regions = vue::embedded_regions(source_text)?;
            format_embedded(file_name, source_text, config, &regions)
        }
        None => Err(FormatError::unsupported_source(file_name)),
    }
}

fn format_embedded(
    file_name: &Path,
    source: &str,
    config: &ResolvedConfig,
    regions: &[EmbeddedRegion],
) -> Result<Option<String>, FormatError> {
    validate_regions(source, regions)?;
    let document_newline = rewriter::detect_newline(source, None);
    let mut formatted = Vec::with_capacity(regions.len());
    let mut changed = false;

    for region in regions {
        let script = &source[region.range.clone()];
        let output = rewriter::format_script(
            Path::new(&region.label),
            script,
            region.source_type,
            Some(document_newline),
            config,
        )
        .map_err(|error| embedded_error(file_name, &region.label, error))?;
        changed |= output.is_some();
        formatted.push(output);
    }

    if !changed {
        return Ok(None);
    }

    let output_len =
        regions
            .iter()
            .zip(&formatted)
            .fold(source.len(), |length, (region, output)| {
                output
                    .as_ref()
                    .map_or(length, |output| length - region.range.len() + output.len())
            });
    let mut output = String::with_capacity(output_len);
    let mut cursor = 0;
    for (region, formatted) in regions.iter().zip(formatted) {
        output.push_str(&source[cursor..region.range.start]);
        output.push_str(
            formatted
                .as_deref()
                .unwrap_or(&source[region.range.clone()]),
        );
        cursor = region.range.end;
    }
    output.push_str(&source[cursor..]);
    Ok(Some(output))
}

fn embedded_error(file_name: &Path, label: &str, error: FormatError) -> FormatError {
    let context = format!("{} {label}", diagnostic_path(file_name));
    match error {
        FormatError::Parse { diagnostics } => FormatError::Parse {
            diagnostics: format!("{context}: {diagnostics}"),
        },
        FormatError::Verification { message } => FormatError::Verification {
            message: format!("{context}: {message}"),
        },
        error => error,
    }
}

fn diagnostic_path(path: &Path) -> String {
    let mut escaped = String::new();
    for character in path.to_string_lossy().chars() {
        if character.is_control() {
            escaped.extend(character.escape_default());
        } else {
            escaped.push(character);
        }
    }
    escaped
}

pub(crate) fn script_source_type(path: &Path) -> Option<SourceType> {
    let source_type = SourceType::from_path(path).ok()?;
    Some(if source_type.is_javascript() {
        source_type.with_jsx(true)
    } else {
        source_type
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::{FormatConfig, FormatError, format_text, resolve_config};

    fn config() -> crate::ResolvedConfig {
        resolve_config(FormatConfig::default()).unwrap()
    }

    #[test]
    fn formats_all_supported_vue_scripts_and_is_idempotent() {
        let source = "﻿<!-- top -->\r\n<template><script>not code</script><div>é</div></template>\r\n<script>import{a}from'a';const one={x:1,};</script>\r\n<style>.x{}</style>\r\n<script setup lang=\"ts\">import{b}from'b';const two:number=2;</script>\r\n<docs><script>also not code</script></docs>\r\n";
        let output = format_text(Path::new("тест.vue"), source, &config())
            .unwrap()
            .unwrap();

        assert_eq!(
            output,
            "﻿<!-- top -->\r\n<template><script>not code</script><div>é</div></template>\r\n<script>import { a } from 'a'\r\n\r\nconst one={x:1}</script>\r\n<style>.x{}</style>\r\n<script setup lang=\"ts\">import { b } from 'b'\r\n\r\nconst two:number=2</script>\r\n<docs><script>also not code</script></docs>\r\n"
        );
        assert!(
            format_text(Path::new("тест.vue"), &output, &config())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn preserves_vue_script_indent_across_layout_rules() {
        let source = "<script setup lang=\"ts\">\n  import{first,second}from'pkg';type Alias=string;interface Shape { first: string; second: number; }class Store { value=1; }const value={items:[1,2,],};run();\n</script>";
        let config = resolve_config(FormatConfig {
            line_width: 36,
            ..FormatConfig::default()
        })
        .unwrap();
        let output = format_text(Path::new("component.vue"), source, &config)
            .unwrap()
            .unwrap();

        assert_eq!(
            output,
            "<script setup lang=\"ts\">\n  import {\n    first,\n    second\n  } from 'pkg'\n\n  type Alias=string\n\n  interface Shape {\n    first: string\n    second: number\n  }\n  class Store { value=1 }\n\n  const value={items:[1,2]}\n\n  run()\n</script>"
        );
        assert!(
            format_text(Path::new("component.vue"), &output, &config)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn formats_inline_vue_import_with_final_indent_idempotently() {
        let config = resolve_config(FormatConfig {
            line_width: 24,
            ..FormatConfig::default()
        })
        .unwrap();

        for newline in ["\n", "\r\n"] {
            let source = format!(
                "<script setup lang=\"ts\">{newline}  run();import{{x}}from'pkg';{newline}</script>"
            );
            let expected = format!(
                "<script setup lang=\"ts\">{newline}  run(){newline}{newline}  import {{{newline}    x{newline}  }} from 'pkg'{newline}</script>"
            );
            let output = format_text(Path::new("component.vue"), &source, &config)
                .unwrap()
                .unwrap();

            assert_eq!(output, expected);
            assert!(
                format_text(Path::new("component.vue"), &output, &config)
                    .unwrap()
                    .is_none()
            );
        }
    }

    #[test]
    fn preserves_the_complete_vue_document_when_rules_are_off() {
        let source = "<template> untouched </template>\n<script>const value={a:1,};function f(){work();return value;}</script>";
        let raw = r#"{
            "rules": {
                "importLayout": false,
                "interfaceLayout": "off",
                "statementSpacing": {
                    "controlFlowStatements": "off",
                    "imports": "off",
                    "returnStatements": "off",
                    "typeAliases": "off",
                    "variableDeclarations": "off"
                },
                "semicolons": {
                    "statements": "off",
                    "classMembers": "off",
                    "typeMembers": "off"
                },
                "trailingCommas": "off"
            }
        }"#;
        let config = resolve_config(serde_json::from_str(raw).unwrap()).unwrap();
        assert!(
            format_text(Path::new("component.vue"), source, &config)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn reports_vue_and_script_parse_errors_with_stable_codes() {
        let script_error = format_text(
            Path::new("component.vue"),
            "<script setup lang=\"ts\">const value = @;</script>",
            &config(),
        )
        .unwrap_err();
        assert_eq!(script_error.code(), "PARSE_ERROR");
        assert!(
            script_error
                .to_string()
                .contains("component.vue <script setup lang=\"ts\">")
        );

        let vue_error = format_text(
            Path::new("component.vue"),
            "<script>const value = 1;",
            &config(),
        )
        .unwrap_err();
        assert!(matches!(vue_error, FormatError::EmbeddedParse { .. }));
        assert_eq!(vue_error.code(), "PARSE_ERROR");
    }

    #[test]
    fn formats_jsx_and_tsx_vue_scripts_with_their_declared_source_types() {
        for (source, expected) in [
            (
                r#"<script lang="jsx">const element=<div/>;</script>"#,
                r#"<script lang="jsx">const element=<div/></script>"#,
            ),
            (
                r#"<script setup lang="tsx">const element: JSX.Element=<div/>;</script>"#,
                r#"<script setup lang="tsx">const element: JSX.Element=<div/></script>"#,
            ),
        ] {
            assert_eq!(
                format_text(Path::new("component.vue"), source, &config())
                    .unwrap()
                    .unwrap(),
                expected
            );
        }
    }

    #[test]
    fn escapes_control_characters_in_embedded_script_error_paths() {
        let error = format_text(
            Path::new("component\u{1b}[31m.vue"),
            "<script>const value = @;</script>",
            &config(),
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("component\\u{1b}[31m.vue <script>"));
        assert!(!error.contains('\u{1b}'));
    }

    #[test]
    fn returns_no_partial_document_when_a_later_script_fails() {
        let source = concat!(
            "<script>import{value}from'pkg';</script>",
            "<template>untouched</template>",
            "<script setup lang=\"ts\">const broken = @;</script>",
        );
        let error = format_text(Path::new("component.vue"), source, &config()).unwrap_err();
        assert_eq!(error.code(), "PARSE_ERROR");
        assert!(error.to_string().contains("<script setup lang=\"ts\">"));
    }

    #[test]
    fn direct_scripts_do_not_invoke_the_vue_scanner() {
        crate::vue::reset_scanner_steps();
        format_text(Path::new("direct.ts"), "const value=1;", &config()).unwrap();
        assert_eq!(crate::vue::scanner_steps(), 0);
    }

    #[test]
    fn verifies_each_embedded_script_before_assembling_output() {
        crate::rewriter::corrupt_next_rewrite_for_test();
        let source = "<template>untouched</template><script>const value=1;</script>";
        let error = format_text(Path::new("component.vue"), source, &config()).unwrap_err();
        assert_eq!(error.code(), "VERIFICATION_ERROR");
        assert!(error.to_string().contains("component.vue <script>"));
    }
}
