use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

fn command(directory: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_worsier"));
    command.current_dir(directory);
    command
}

fn write(path: &Path, source: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, source).unwrap();
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

#[test]
fn uses_defaults_without_config_and_init_refuses_to_overwrite() {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join(".git")).unwrap();
    write(
        &directory.path().join("sample.ts"),
        "import{value}from'pkg';const raw={\n  items: [\n    1,\n  ],\n};",
    );

    let output = command(directory.path()).arg("sample.ts").output().unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "import { value } from 'pkg'\n\nconst raw={\n  items: [\n    1\n  ]\n}"
    );

    let initialized = command(directory.path()).arg("--init").output().unwrap();
    assert!(initialized.status.success());
    assert!(directory.path().join("worsier.jsonc").is_file());

    let overwrite = command(directory.path()).arg("--init").output().unwrap();
    assert_eq!(overwrite.status.code(), Some(2));
    assert!(stderr(&overwrite).contains("already exists"));

    write(&directory.path().join("notes.txt"), "not source");
    let unsupported = command(directory.path()).arg("notes.txt").output().unwrap();
    assert_eq!(unsupported.status.code(), Some(2));
    assert!(stderr(&unsupported).contains("not a supported"));
}

#[test]
fn updates_default_and_explicit_configs_and_is_idempotent() {
    let directory = tempfile::tempdir().unwrap();
    let default_config = directory.path().join("worsier.jsonc");
    write(&default_config, r#"{"lineWidth":80}"#);

    let updated = command(directory.path())
        .arg("--update-config")
        .output()
        .unwrap();
    assert!(updated.status.success(), "{}", stderr(&updated));
    let updated_stdout = String::from_utf8(updated.stdout).unwrap();
    assert!(updated_stdout.contains("Added $schema"));
    assert!(updated_stdout.contains("Added rules"));
    assert!(updated_stdout.contains("Updated "));
    let first_output = fs::read_to_string(&default_config).unwrap();
    assert!(first_output.contains("\"lineWidth\":80"));
    assert!(first_output.contains("\"controlFlowStatements\": \"separate\""));
    assert!(first_output.contains("\"returnStatements\": \"separate\""));
    assert!(first_output.contains("\"typeAliases\": \"separate\""));
    let first_metadata = fs::metadata(&default_config).unwrap();

    let unchanged = command(directory.path())
        .arg("--update-config")
        .output()
        .unwrap();
    assert!(unchanged.status.success(), "{}", stderr(&unchanged));
    assert!(
        String::from_utf8(unchanged.stdout)
            .unwrap()
            .contains("Configuration is up to date:")
    );
    assert_eq!(fs::read_to_string(&default_config).unwrap(), first_output);
    let unchanged_metadata = fs::metadata(&default_config).unwrap();
    assert_eq!(
        unchanged_metadata.modified().unwrap(),
        first_metadata.modified().unwrap()
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        assert_eq!(unchanged_metadata.ino(), first_metadata.ino());
    }

    let nested_working_directory = directory.path().join("nested-work");
    fs::create_dir(&nested_working_directory).unwrap();
    let not_discovered = command(&nested_working_directory)
        .arg("--update-config")
        .output()
        .unwrap();
    assert_eq!(not_discovered.status.code(), Some(2));
    assert!(stderr(&not_discovered).contains("failed to resolve configuration"));
    assert_eq!(fs::read_to_string(&default_config).unwrap(), first_output);

    let explicit_config = directory.path().join("nested/custom.jsonc");
    write(
        &explicit_config,
        "{\n  \"rules\": {\n    // layout\n    \"imports\": false,\n    \"variables\": true\n  }\n}\n",
    );
    let explicit = command(directory.path())
        .args(["--update-config", "--config", "nested/custom.jsonc"])
        .output()
        .unwrap();
    assert!(explicit.status.success(), "{}", stderr(&explicit));
    let explicit_stdout = String::from_utf8(explicit.stdout).unwrap();
    assert!(explicit_stdout.contains("Migrated rules.imports"));
    assert!(explicit_stdout.contains("Migrated rules.variables"));
    let explicit_output = fs::read_to_string(explicit_config).unwrap();
    assert!(explicit_output.contains("// layout"));
    assert!(explicit_output.contains("\"importLayout\": false"));
    assert!(explicit_output.contains("\"imports\": \"off\""));
    assert!(explicit_output.contains("\"variableDeclarations\": \"separate\""));
    assert!(!explicit_output.contains("\"variables\""));
}

#[test]
fn update_config_rejects_conflicts_invalid_targets_and_formatting_modes() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("worsier.jsonc");

    for (conflicting, conflict_path) in [
        (
            r#"{"rules":{"imports":true,"importLayout":false}}"#,
            "rules.importLayout",
        ),
        (
            r#"{"rules":{"imports":true,"statementSpacing":{"imports":"off"}}}"#,
            "rules.statementSpacing.imports",
        ),
        (
            r#"{"rules":{"variables":true,"statementSpacing":{"variableDeclarations":"off"}}}"#,
            "rules.statementSpacing.variableDeclarations",
        ),
        (
            r#"{"rules":{"imports":true,"statementSpacing":"off"}}"#,
            "rules.statementSpacing",
        ),
    ] {
        write(&config, conflicting);
        let conflict = command(directory.path())
            .arg("--update-config")
            .output()
            .unwrap();
        assert_eq!(conflict.status.code(), Some(2));
        assert!(
            stderr(&conflict).contains(conflict_path),
            "{}",
            stderr(&conflict)
        );
        assert_eq!(fs::read(&config).unwrap(), conflicting.as_bytes());
    }

    let duplicate = r#"{"rules":{},"rules":{"imports":true}}"#;
    write(&config, duplicate);
    let duplicate_result = command(directory.path())
        .arg("--update-config")
        .output()
        .unwrap();
    assert_eq!(duplicate_result.status.code(), Some(2));
    assert!(stderr(&duplicate_result).contains("duplicate configuration property rules"));
    assert_eq!(fs::read(&config).unwrap(), duplicate.as_bytes());

    let invalid_ignore = r#"{"ignorePatterns":["[z-a]"]}"#;
    write(&config, invalid_ignore);
    let invalid_ignore_result = command(directory.path())
        .arg("--update-config")
        .output()
        .unwrap();
    assert_eq!(invalid_ignore_result.status.code(), Some(2));
    assert!(stderr(&invalid_ignore_result).contains("invalid ignore pattern"));
    assert_eq!(fs::read(&config).unwrap(), invalid_ignore.as_bytes());

    let invalid_current = r#"{"lineWidth":0}"#;
    write(&config, invalid_current);
    let invalid = command(directory.path())
        .arg("--update-config")
        .output()
        .unwrap();
    assert_eq!(invalid.status.code(), Some(2));
    assert!(stderr(&invalid).contains("lineWidth"));
    assert_eq!(fs::read_to_string(&config).unwrap(), invalid_current);

    fs::remove_file(&config).unwrap();
    let missing = command(directory.path())
        .arg("--update-config")
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(2));
    assert!(stderr(&missing).contains("failed to resolve configuration"));

    fs::create_dir(&config).unwrap();
    let directory_target = command(directory.path())
        .arg("--update-config")
        .output()
        .unwrap();
    assert_eq!(directory_target.status.code(), Some(2));
    assert!(stderr(&directory_target).contains("is not a file"));
    fs::remove_dir(&config).unwrap();
    write(&config, "{}");
    write(&directory.path().join("sample.ts"), "const value=1;");

    for arguments in [
        vec!["--update-config", "--init"],
        vec!["--update-config", "sample.ts"],
        vec!["--update-config", "--check"],
        vec!["--update-config", "--write"],
        vec!["--update-config", "--stdin-filepath", "sample.ts"],
        vec!["--update-config", "--threads", "2"],
        vec!["--update-config", "--no-verify"],
    ] {
        let output = command(directory.path()).args(arguments).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(stderr(&output).contains("cannot be used with"));
    }
}

#[test]
fn legacy_config_load_suggests_the_explicit_updater() {
    let directory = tempfile::tempdir().unwrap();
    let project = directory.path().join("project with spaces");
    fs::create_dir(&project).unwrap();
    write(
        &project.join("worsier.jsonc"),
        r#"{"rules":{"imports":true}}"#,
    );
    write(&project.join("sample.ts"), "const value=1;");

    let output = command(&project).arg("sample.ts").output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    let diagnostic = stderr(&output);
    assert!(diagnostic.contains("legacy Worsier v1 rules"));
    assert!(diagnostic.contains("worsier --update-config --config <PATH>"));
    let config = project.join("worsier.jsonc").canonicalize().unwrap();
    let config = config.to_string_lossy();
    assert!(diagnostic.contains(config.as_ref()));
    assert!(!diagnostic.contains(&format!("worsier --update-config --config {config}")));
    assert!(diagnostic.contains("rules.imports"));

    write(
        &project.join("worsier.jsonc"),
        r#"{"rules":{"statementSpacing":{"imports":"preserve"}}}"#,
    );
    let current_error = command(&project).arg("sample.ts").output().unwrap();
    assert_eq!(current_error.status.code(), Some(2));
    assert!(!stderr(&current_error).contains("--update-config"));
}

#[test]
fn supports_trailing_comma_modes_from_config() {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join(".git")).unwrap();
    let source = "const value = {\n  items: [\n    1\n  ]\n};";
    write(&directory.path().join("sample.ts"), source);

    write(
        &directory.path().join("worsier.jsonc"),
        r#"{"rules":{"importLayout":false,"objectPropertySpacing":false,"statementSpacing":{"controlFlowStatements":"off","imports":"off","multilineCallStatements":"off","returnStatements":"off","typeAliases":"off","variableDeclarations":"off"},"semicolons":{"statements":"off","classMembers":"off","typeMembers":"off"},"trailingCommas":"always"}}"#,
    );
    let always = command(directory.path()).arg("sample.ts").output().unwrap();
    assert!(always.status.success(), "{}", stderr(&always));
    assert_eq!(
        String::from_utf8(always.stdout).unwrap(),
        "const value = {\n  items: [\n    1,\n  ],\n};"
    );

    write(
        &directory.path().join("worsier.jsonc"),
        r#"{"rules":{"importLayout":false,"objectPropertySpacing":false,"statementSpacing":{"controlFlowStatements":"off","imports":"off","multilineCallStatements":"off","returnStatements":"off","typeAliases":"off","variableDeclarations":"off"},"semicolons":{"statements":"off","classMembers":"off","typeMembers":"off"},"trailingCommas":"off"}}"#,
    );
    let off = command(directory.path()).arg("sample.ts").output().unwrap();
    assert!(off.status.success(), "{}", stderr(&off));
    assert_eq!(String::from_utf8(off.stdout).unwrap(), source);
}

#[test]
fn supports_granular_semicolon_modes_from_config() {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join(".git")).unwrap();
    write(
        &directory.path().join("worsier.jsonc"),
        r#"{"rules":{"importLayout":false,"objectPropertySpacing":false,"statementSpacing":{"controlFlowStatements":"off","imports":"off","multilineCallStatements":"off","returnStatements":"off","typeAliases":"off","variableDeclarations":"off"},"semicolons":{"statements":"off","classMembers":"asNeeded","typeMembers":"always"},"trailingCommas":"off"}}"#,
    );
    write(
        &directory.path().join("sample.ts"),
        "const runtime=1;\nclass Example {\n  field=1;\n}\ninterface Shape {\n  value: string;\n}",
    );

    let output = command(directory.path()).arg("sample.ts").output().unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "const runtime=1;\nclass Example {\n  field=1\n}\ninterface Shape {\n  value: string;\n}"
    );
}

#[test]
fn supports_type_alias_spacing_from_config() {
    let directory = tempfile::tempdir().unwrap();
    write(
        &directory.path().join("worsier.jsonc"),
        r#"{"rules":{"importLayout":false,"objectPropertySpacing":false,"interfaceLayout":"off","statementSpacing":{"controlFlowStatements":"off","imports":"off","multilineCallStatements":"off","returnStatements":"off","typeAliases":"compact","variableDeclarations":"off"},"semicolons":{"statements":"off","classMembers":"off","typeMembers":"off"},"trailingCommas":"off"}}"#,
    );
    write(
        &directory.path().join("sample.ts"),
        "type A=1;type B={\n value:string\n};\n\n\nrun();",
    );

    let output = command(directory.path()).arg("sample.ts").output().unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "type A=1;\ntype B={\n value:string\n};\nrun();"
    );
}

#[test]
fn preserves_detached_comment_gaps_with_platform_line_endings() {
    for newline in ["\n", "\r\n"] {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir(directory.path().join(".git")).unwrap();
        let source = format!(
            "import type {{{newline}  FilterDefinition,{newline}  FilterOperator,{newline}  FilterRule{newline}}} from '~/types/filter';{newline}{newline}/**{newline} * Glossary{newline} */{newline}{newline}type FilterBuilderStep = 'field' | 'operator' | 'value';"
        );
        write(&directory.path().join("sample.ts"), &source);

        let output = command(directory.path()).arg("sample.ts").output().unwrap();
        assert!(output.status.success(), "{}", stderr(&output));
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            format!(
                "import type {{ FilterDefinition, FilterOperator, FilterRule }} from '~/types/filter'{newline}{newline}/**{newline} * Glossary{newline} */{newline}{newline}type FilterBuilderStep = 'field' | 'operator' | 'value'"
            )
        );
    }
}

#[test]
fn supports_multiline_call_spacing_from_config() {
    let directory = tempfile::tempdir().unwrap();
    write(
        &directory.path().join("worsier.jsonc"),
        r#"{"rules":{"importLayout":false,"objectPropertySpacing":false,"interfaceLayout":"off","statementSpacing":{"controlFlowStatements":"off","imports":"off","multilineCallStatements":"separate","returnStatements":"off","typeAliases":"off","variableDeclarations":"off"},"semicolons":{"statements":"off","classMembers":"off","typeMembers":"off"},"trailingCommas":"off"}}"#,
    );
    write(
        &directory.path().join("sample.ts"),
        "async function f() {\n  before()\n  await call(\n    value\n  )\n  after()\n}",
    );

    let output = command(directory.path()).arg("sample.ts").output().unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "async function f() {\n  before()\n\n  await call(\n    value\n  )\n\n  after()\n}"
    );
}

#[test]
fn supports_control_flow_spacing_from_config() {
    let directory = tempfile::tempdir().unwrap();
    write(
        &directory.path().join("worsier.jsonc"),
        r#"{"rules":{"importLayout":false,"objectPropertySpacing":false,"interfaceLayout":"off","statementSpacing":{"controlFlowStatements":"separate","imports":"off","multilineCallStatements":"off","returnStatements":"off","typeAliases":"off","variableDeclarations":"off"},"semicolons":{"statements":"off","classMembers":"off","typeMembers":"off"},"trailingCommas":"off"}}"#,
    );
    write(
        &directory.path().join("sample.ts"),
        "function f(){before();if(ok)work();after();}",
    );

    let output = command(directory.path()).arg("sample.ts").output().unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "function f(){\n  before();\n\n  if(ok)work();\n\n  after();\n}"
    );
}

#[test]
fn supports_interface_layout_thresholds_and_off_from_config() {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join(".git")).unwrap();
    let source = "interface Shape { value: string; }";
    write(&directory.path().join("sample.ts"), source);

    let default = command(directory.path()).arg("sample.ts").output().unwrap();
    assert!(default.status.success(), "{}", stderr(&default));
    assert_eq!(
        String::from_utf8(default.stdout).unwrap(),
        "interface Shape {\n  value: string;\n}"
    );

    write(
        &directory.path().join("worsier.jsonc"),
        r#"{"rules":{"interfaceLayout":1.0,"semicolons":{"typeMembers":"off"}}}"#,
    );
    let threshold = command(directory.path()).arg("sample.ts").output().unwrap();
    assert!(threshold.status.success(), "{}", stderr(&threshold));
    assert_eq!(String::from_utf8(threshold.stdout).unwrap(), source);

    write(
        &directory.path().join("worsier.jsonc"),
        r#"{"rules":{"interfaceLayout":"off","semicolons":{"typeMembers":"off"}}}"#,
    );
    let off = command(directory.path()).arg("sample.ts").output().unwrap();
    assert!(off.status.success(), "{}", stderr(&off));
    assert_eq!(String::from_utf8(off.stdout).unwrap(), source);
}

#[test]
fn supports_object_property_spacing_from_config() {
    let directory = tempfile::tempdir().unwrap();
    let source = "const value={first:1,second:2};";
    write(&directory.path().join("sample.ts"), source);

    let default = command(directory.path()).arg("sample.ts").output().unwrap();
    assert!(default.status.success(), "{}", stderr(&default));
    assert_eq!(
        String::from_utf8(default.stdout).unwrap(),
        "const value={\n  first:1,\n  second:2\n}"
    );

    write(
        &directory.path().join("worsier.jsonc"),
        r#"{"rules":{"importLayout":false}}"#,
    );
    let partial = command(directory.path()).arg("sample.ts").output().unwrap();
    assert!(partial.status.success(), "{}", stderr(&partial));
    assert_eq!(
        String::from_utf8(partial.stdout).unwrap(),
        "const value={\n  first:1,\n  second:2\n}"
    );

    write(
        &directory.path().join("worsier.jsonc"),
        r#"{"rules":{"importLayout":false,"interfaceLayout":"off","objectPropertySpacing":false,"statementSpacing":{"controlFlowStatements":"off","imports":"off","multilineCallStatements":"off","returnStatements":"off","typeAliases":"off","variableDeclarations":"off"},"semicolons":{"statements":"off","classMembers":"off","typeMembers":"off"},"trailingCommas":"off"}}"#,
    );
    let disabled = command(directory.path()).arg("sample.ts").output().unwrap();
    assert!(disabled.status.success(), "{}", stderr(&disabled));
    assert_eq!(String::from_utf8(disabled.stdout).unwrap(), source);
}

#[test]
fn supports_stdout_stdin_check_and_direct_write() {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join(".git")).unwrap();
    write(
        &directory.path().join("sample.ts"),
        "import{answer,type Value}from'pkg';const value={answer:42};",
    );

    let stdout = command(directory.path()).arg("sample.ts").output().unwrap();
    assert!(stdout.status.success());
    assert_eq!(
        String::from_utf8(stdout.stdout).unwrap(),
        "import { answer, type Value } from 'pkg'\n\nconst value={answer:42}"
    );

    let check = command(directory.path())
        .args(["--check", "."])
        .output()
        .unwrap();
    assert_eq!(check.status.code(), Some(1));
    assert_eq!(
        fs::read_to_string(directory.path().join("sample.ts")).unwrap(),
        "import{answer,type Value}from'pkg';const value={answer:42};"
    );

    let write_result = command(directory.path())
        .args(["--write", "sample.ts"])
        .output()
        .unwrap();
    assert!(write_result.status.success());
    assert_eq!(
        fs::read_to_string(directory.path().join("sample.ts")).unwrap(),
        "import { answer, type Value } from 'pkg'\n\nconst value={answer:42}"
    );
    assert!(
        command(directory.path())
            .args(["--check", "."])
            .status()
            .unwrap()
            .success()
    );

    let mut child = command(directory.path())
        .args(["--stdin-filepath", "virtual.ts"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"import{stdinValue}from'pkg';const raw=[1,2];")
        .unwrap();
    let stdin = child.wait_with_output().unwrap();
    assert!(stdin.status.success());
    assert_eq!(
        String::from_utf8(stdin.stdout).unwrap(),
        "import { stdinValue } from 'pkg'\n\nconst raw=[1,2]"
    );
}

#[test]
fn config_discovery_stops_at_a_repository_without_config() {
    let directory = tempfile::tempdir().unwrap();
    write(
        &directory.path().join("worsier.jsonc"),
        r#"{"lineWidth":20}"#,
    );
    fs::create_dir_all(directory.path().join("repository/.git")).unwrap();
    write(
        &directory.path().join("repository/sample.ts"),
        "import{one,two}from'package';",
    );

    let output = command(directory.path())
        .arg("repository/sample.ts")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "import { one, two } from 'package'"
    );
}

#[test]
fn nearest_config_wins_and_explicit_config_disables_discovery() {
    let directory = tempfile::tempdir().unwrap();
    write(
        &directory.path().join("worsier.jsonc"),
        r#"{"lineWidth":120}"#,
    );
    write(
        &directory.path().join("nested/worsier.jsonc"),
        r#"{"lineWidth":20}"#,
    );
    write(
        &directory.path().join("nested/sample.ts"),
        "import{one,two}from'package';",
    );

    let nearest = command(directory.path())
        .arg("nested/sample.ts")
        .output()
        .unwrap();
    assert!(nearest.status.success());
    assert_eq!(
        String::from_utf8(nearest.stdout).unwrap(),
        "import {\n  one,\n  two\n} from 'package'"
    );

    let explicit = command(directory.path())
        .args(["--config", "worsier.jsonc", "nested/sample.ts"])
        .output()
        .unwrap();
    assert!(explicit.status.success());
    assert_eq!(
        String::from_utf8(explicit.stdout).unwrap(),
        "import { one, two } from 'package'"
    );
}

#[test]
fn partial_rule_configs_keep_sibling_defaults() {
    let directory = tempfile::tempdir().unwrap();
    write(
        &directory.path().join("worsier.jsonc"),
        r#"{"rules":{"statementSpacing":{"variableDeclarations":"off"}}}"#,
    );
    let source = "import{a}from'x';const first=1;let second=2;run();";
    write(&directory.path().join("sample.ts"), source);

    let imports_only = command(directory.path()).arg("sample.ts").output().unwrap();
    assert!(imports_only.status.success(), "{}", stderr(&imports_only));
    assert_eq!(
        String::from_utf8(imports_only.stdout).unwrap(),
        "import { a } from 'x'\n\nconst first=1;let second=2;run()"
    );

    write(
        &directory.path().join("worsier.jsonc"),
        r#"{"rules":{"importLayout":false,"objectPropertySpacing":false,"statementSpacing":{"imports":"off"}}}"#,
    );
    let variables_only = command(directory.path()).arg("sample.ts").output().unwrap();
    assert!(
        variables_only.status.success(),
        "{}",
        stderr(&variables_only)
    );
    assert_eq!(
        String::from_utf8(variables_only.stdout).unwrap(),
        "import{a}from'x'\n\nconst first=1\nlet second=2\n\nrun()"
    );
}

#[test]
fn configuration_errors_include_the_nested_json_path() {
    let directory = tempfile::tempdir().unwrap();
    write(&directory.path().join("sample.ts"), "const value=1;");

    for (config, path) in [
        (r#"{"rules":{"imports":true}}"#, "rules.imports"),
        (r#"{"rules":{"variables":true}}"#, "rules.variables"),
        (
            r#"{"rules":{"interfaceLayout":-1}}"#,
            "rules.interfaceLayout",
        ),
        (
            r#"{"rules":{"interfaceLayout":4294967296}}"#,
            "rules.interfaceLayout",
        ),
        (
            r#"{"rules":{"objectPropertySpacing":"always"}}"#,
            "rules.objectPropertySpacing",
        ),
        (r#"{"rules":{"semicolons":"always"}}"#, "rules.semicolons"),
        (
            r#"{"rules":{"semicolons":{"statements":"never"}}}"#,
            "rules.semicolons.statements",
        ),
        (
            r#"{"rules":{"semicolons":{"typeMembers":{"singleLine":"never"}}}}"#,
            "rules.semicolons.typeMembers.singleLine",
        ),
        (
            r#"{"rules":{"semicolons":{"typeMembers":{"unknown":"off"}}}}"#,
            "rules.semicolons.typeMembers.unknown",
        ),
        (
            r#"{"rules":{"semicolons":{"unknown":"off"}}}"#,
            "rules.semicolons.unknown",
        ),
        (
            r#"{"rules":{"statementSpacing":{"controlFlowStatements":"preserve"}}}"#,
            "rules.statementSpacing.controlFlowStatements",
        ),
        (
            r#"{"rules":{"statementSpacing":{"imports":"preserve"}}}"#,
            "rules.statementSpacing.imports",
        ),
        (
            r#"{"rules":{"statementSpacing":{"multilineCallStatements":"preserve"}}}"#,
            "rules.statementSpacing.multilineCallStatements",
        ),
        (
            r#"{"rules":{"statementSpacing":{"typeAliases":"preserve"}}}"#,
            "rules.statementSpacing.typeAliases",
        ),
        (
            r#"{"rules":{"trailingCommas":"multiline"}}"#,
            "rules.trailingCommas",
        ),
        (
            r#"{"rules":{"statementSpacing":{"unknown":"off"}}}"#,
            "rules.statementSpacing.unknown",
        ),
        (r#"{"quoteStyle":"single"}"#, "quoteStyle"),
    ] {
        write(&directory.path().join("worsier.jsonc"), config);
        let output = command(directory.path()).arg("sample.ts").output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(stderr(&output).contains(path), "{}", stderr(&output));
    }
}

#[test]
fn explicit_config_errors_when_inputs_are_empty() {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join("empty")).unwrap();

    let output = command(directory.path())
        .args(["--config", "missing.jsonc", "--check", "empty"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("failed to resolve configuration"));
}

#[test]
fn directory_walk_respects_builtin_gitignore_config_ignores_and_sorts_diagnostics() {
    let directory = tempfile::tempdir().unwrap();
    write(
        &directory.path().join("worsier.jsonc"),
        r#"{"ignorePatterns":["ignored/**"]}"#,
    );
    write(&directory.path().join(".gitignore"), "gitignored.js\n");
    write(&directory.path().join("ignored/skip.ts"), "const skip=1;");
    write(&directory.path().join("gitignored.js"), "const ignored=1;");
    write(
        &directory.path().join("worker-configuration.d.ts"),
        "declare const generated = @;",
    );
    write(&directory.path().join("z-invalid.ts"), "const z = @;");
    write(&directory.path().join("a-invalid.ts"), "const a = @;");

    let output = command(directory.path())
        .args(["--check", ".", "--threads", "2"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let diagnostics = stderr(&output);
    assert!(!diagnostics.contains("skip.ts"));
    assert!(!diagnostics.contains("gitignored.js"));
    assert!(!diagnostics.contains("worker-configuration.d.ts"));
    assert!(diagnostics.find("a-invalid.ts").unwrap() < diagnostics.find("z-invalid.ts").unwrap());

    write(
        &directory.path().join("worker-configuration.d.ts"),
        "import{Generated}from'pkg';",
    );
    let explicit = command(directory.path())
        .args(["--check", "worker-configuration.d.ts"])
        .output()
        .unwrap();
    assert_eq!(explicit.status.code(), Some(1));
    assert!(stderr(&explicit).contains("Would format worker-configuration.d.ts"));
}

#[test]
fn nested_config_ignores_are_relative_to_that_config() {
    let directory = tempfile::tempdir().unwrap();
    write(&directory.path().join("worsier.jsonc"), "{}");
    write(
        &directory.path().join("nested/worsier.jsonc"),
        r#"{"ignorePatterns":["ignored/**"]}"#,
    );
    let ignored = directory.path().join("nested/ignored/sample.ts");
    write(&ignored, "import{ignored}from'pkg';");
    let included = directory.path().join("nested/included.ts");
    write(&included, "import{included}from'pkg';");

    let output = command(directory.path())
        .args(["--write", "."])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        fs::read_to_string(ignored).unwrap(),
        "import{ignored}from'pkg';"
    );
    assert_eq!(
        fs::read_to_string(included).unwrap(),
        "import { included } from 'pkg'"
    );
}

#[cfg(unix)]
#[test]
fn explicit_symbolic_links_are_rejected_without_modifying_the_target() {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir().unwrap();
    write(&directory.path().join("worsier.jsonc"), "{}");
    let target = directory.path().join("target.ts");
    let link = directory.path().join("link.ts");
    write(&target, "import{value}from'pkg';");
    symlink("target.ts", &link).unwrap();

    let output = command(directory.path())
        .args(["--write", "link.ts"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("symbolic link"));
    assert!(fs::symlink_metadata(link).unwrap().file_type().is_symlink());
    assert_eq!(
        fs::read_to_string(target).unwrap(),
        "import{value}from'pkg';"
    );
}

#[test]
fn hard_link_aliases_are_rejected_before_parallel_writes() {
    let directory = tempfile::tempdir().unwrap();
    let first = directory.path().join("a/shared.ts");
    let second = directory.path().join("b/shared.ts");
    write(
        &directory.path().join("a/worsier.jsonc"),
        r#"{"rules":{"statementSpacing":{"variableDeclarations":"compact"},"semicolons":{"statements":"always"}}}"#,
    );
    write(
        &directory.path().join("b/worsier.jsonc"),
        r#"{"rules":{"statementSpacing":{"variableDeclarations":"compact"},"semicolons":{"statements":"asNeeded"}}}"#,
    );
    write(&first, "const first=1;const second=2;");
    fs::hard_link(&first, &second).unwrap();

    let output = command(directory.path())
        .args(["--write", "--threads", "2", "a/shared.ts", "b/shared.ts"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("identify the same file"));
    assert_eq!(
        fs::read_to_string(first).unwrap(),
        "const first=1;const second=2;"
    );
}

#[cfg(unix)]
#[test]
fn config_update_rejects_symbolic_links_without_modifying_the_target() {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("target.jsonc");
    let link = directory.path().join("worsier.jsonc");
    write(&target, "{}");
    symlink("target.jsonc", &link).unwrap();

    let output = command(directory.path())
        .arg("--update-config")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("symbolic link"));
    assert!(fs::symlink_metadata(link).unwrap().file_type().is_symlink());
    assert_eq!(fs::read_to_string(target).unwrap(), "{}");
}

#[cfg(unix)]
#[test]
fn direct_write_preserves_inode_permissions_and_extended_attributes() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let directory = tempfile::tempdir().unwrap();
    write(&directory.path().join("worsier.jsonc"), "{}");
    let source = directory.path().join("sample.ts");
    write(&source, "import{value}from'pkg';");
    fs::set_permissions(&source, fs::Permissions::from_mode(0o640)).unwrap();
    let attribute = if cfg!(target_os = "macos") {
        "com.perdolique.worsier-test"
    } else {
        "user.worsier-test"
    };
    xattr::set(&source, attribute, b"preserved").unwrap();
    let before = fs::metadata(&source).unwrap();

    let output = command(directory.path())
        .args(["--write", "sample.ts"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));

    let after = fs::metadata(&source).unwrap();
    assert_eq!((after.dev(), after.ino()), (before.dev(), before.ino()));
    assert_eq!(after.permissions().mode() & 0o777, 0o640);
    assert_eq!((after.uid(), after.gid()), (before.uid(), before.gid()));
    assert_eq!(
        xattr::get(source, attribute).unwrap().unwrap(),
        b"preserved"
    );
}

#[cfg(unix)]
#[test]
fn config_update_preserves_permissions_and_extended_attributes() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("worsier.jsonc");
    write(&config, "{}");
    fs::set_permissions(&config, fs::Permissions::from_mode(0o640)).unwrap();
    let attribute = if cfg!(target_os = "macos") {
        "com.perdolique.worsier-config-test"
    } else {
        "user.worsier-config-test"
    };
    xattr::set(&config, attribute, b"preserved").unwrap();
    let before = fs::metadata(&config).unwrap();

    let output = command(directory.path())
        .arg("--update-config")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));

    let after = fs::metadata(&config).unwrap();
    assert_eq!(after.permissions().mode() & 0o777, 0o640);
    assert_eq!((after.uid(), after.gid()), (before.uid(), before.gid()));
    assert_eq!(
        xattr::get(config, attribute).unwrap().unwrap(),
        b"preserved"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn direct_write_preserves_access_control_lists() {
    let directory = tempfile::tempdir().unwrap();
    write(&directory.path().join("worsier.jsonc"), "{}");
    let source = directory.path().join("protected.ts");
    write(&source, "import{value}from'pkg';");

    let chmod = Command::new("chmod")
        .args(["+a", "nobody deny read"])
        .arg(&source)
        .output()
        .unwrap();
    assert!(chmod.status.success(), "{}", stderr(&chmod));

    let output = command(directory.path())
        .args(["--write", "protected.ts"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));

    let listing = Command::new("ls").arg("-le").arg(&source).output().unwrap();
    assert!(listing.status.success(), "{}", stderr(&listing));
    assert!(
        String::from_utf8(listing.stdout)
            .unwrap()
            .contains("user:nobody deny read")
    );
}

#[cfg(target_os = "macos")]
#[test]
fn config_update_preserves_access_control_lists() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("worsier.jsonc");
    write(&config, "{}");

    let chmod = Command::new("chmod")
        .args(["+a", "nobody deny read"])
        .arg(&config)
        .output()
        .unwrap();
    assert!(chmod.status.success(), "{}", stderr(&chmod));

    let output = command(directory.path())
        .arg("--update-config")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));

    let listing = Command::new("ls").arg("-le").arg(&config).output().unwrap();
    assert!(listing.status.success(), "{}", stderr(&listing));
    assert!(
        String::from_utf8(listing.stdout)
            .unwrap()
            .contains("user:nobody deny read")
    );
}

#[test]
fn parse_errors_do_not_modify_files_and_unicode_paths_work() {
    let directory = tempfile::tempdir().unwrap();
    write(&directory.path().join("worsier.jsonc"), "{}");
    let invalid = directory.path().join("invalid.ts");
    write(&invalid, "const value = @;");

    let failed = command(directory.path())
        .args(["--write", "invalid.ts"])
        .output()
        .unwrap();
    assert_eq!(failed.status.code(), Some(2));
    assert_eq!(fs::read_to_string(&invalid).unwrap(), "const value = @;");

    let unicode = directory.path().join("папка с пробелом/файл.ts");
    write(&unicode, "import{value}from'pkg';const raw=[1,2];");
    let written = command(directory.path())
        .arg("--write")
        .arg("папка с пробелом/файл.ts")
        .output()
        .unwrap();
    assert!(written.status.success(), "{}", stderr(&written));
    assert_eq!(
        fs::read_to_string(unicode).unwrap(),
        "import { value } from 'pkg'\n\nconst raw=[1,2]"
    );
}

#[test]
fn accepts_every_documented_source_extension() {
    let directory = tempfile::tempdir().unwrap();
    write(&directory.path().join("worsier.jsonc"), "{}");
    let cases = [
        ("sample.js", "const element=<div/>;"),
        ("sample.mjs", "const value=1;"),
        ("sample.cjs", "const value=1;"),
        ("sample.jsx", "const element=<div/>;"),
        ("sample.ts", "const value:number=1;"),
        ("sample.mts", "const value:number=1;"),
        ("sample.cts", "const value:number=1;"),
        ("sample.tsx", "const element: JSX.Element=<div/>;"),
        (
            "sample.vue",
            "<template><div>untouched</div></template><script setup lang=\"ts\">const value:number=1;</script>",
        ),
    ];

    for (file_name, source) in cases {
        write(&directory.path().join(file_name), source);
        let output = command(directory.path()).arg(file_name).output().unwrap();
        assert!(output.status.success(), "{file_name}: {}", stderr(&output));
        let expected = if file_name == "sample.vue" {
            source.replace("value:number=1;</script>", "value:number=1</script>")
        } else {
            source.strip_suffix(';').unwrap().to_owned()
        };
        assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
    }
}

#[test]
fn supports_vue_in_directory_stdin_check_and_write_flows() {
    let directory = tempfile::tempdir().unwrap();
    write(&directory.path().join("worsier.jsonc"), "{}");
    let source = "<template><div data-label=\">\">{{ \"<template>\" }}</div></template>\n<i18n>{\"message\":\"<!--\"}</i18n>\n<script setup lang=\"ts\">import{value}from'pkg';const count:number=1;</script>\n<style>.x{color:red}</style>";
    let expected = "<template><div data-label=\">\">{{ \"<template>\" }}</div></template>\n<i18n>{\"message\":\"<!--\"}</i18n>\n<script setup lang=\"ts\">import { value } from 'pkg'\n\nconst count:number=1</script>\n<style>.x{color:red}</style>";
    write(&directory.path().join("nested/component.vue"), source);

    let explicit = command(directory.path())
        .arg("nested/component.vue")
        .output()
        .unwrap();
    assert!(explicit.status.success(), "{}", stderr(&explicit));
    assert_eq!(String::from_utf8(explicit.stdout).unwrap(), expected);

    let mut stdin = command(directory.path());
    stdin
        .args(["--stdin-filepath", "nested/component.vue"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    let mut child = stdin.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(source.as_bytes())
        .unwrap();
    let stdin_output = child.wait_with_output().unwrap();
    assert!(stdin_output.status.success(), "{}", stderr(&stdin_output));
    assert_eq!(String::from_utf8(stdin_output.stdout).unwrap(), expected);

    let checked = command(directory.path())
        .args(["--check", "."])
        .output()
        .unwrap();
    assert_eq!(checked.status.code(), Some(1));
    let checked_stderr = stderr(&checked);
    let expected_path = Path::new("nested")
        .join("component.vue")
        .to_string_lossy()
        .into_owned();
    assert!(checked_stderr.contains(&expected_path), "{checked_stderr}");

    let written = command(directory.path())
        .args(["--write", "."])
        .output()
        .unwrap();
    assert!(written.status.success(), "{}", stderr(&written));
    assert_eq!(
        fs::read_to_string(directory.path().join("nested/component.vue")).unwrap(),
        expected
    );

    let clean = command(directory.path())
        .args(["--check", "."])
        .output()
        .unwrap();
    assert!(clean.status.success(), "{}", stderr(&clean));
}
