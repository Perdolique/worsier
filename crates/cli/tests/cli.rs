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
        "import{value}from'pkg';const raw=[1,2];",
    );

    let output = command(directory.path()).arg("sample.ts").output().unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "import { value } from 'pkg';\n\nconst raw=[1,2];"
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
fn supports_stdout_stdin_check_and_atomic_write() {
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
        "import { answer, type Value } from 'pkg';\n\nconst value={answer:42};"
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
        "import { answer, type Value } from 'pkg';\n\nconst value={answer:42};"
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
        "import { stdinValue } from 'pkg';\n\nconst raw=[1,2];"
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
        "import { one, two } from 'package';"
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
        "import {\n  one,\n  two\n} from 'package';"
    );

    let explicit = command(directory.path())
        .args(["--config", "worsier.jsonc", "nested/sample.ts"])
        .output()
        .unwrap();
    assert!(explicit.status.success());
    assert_eq!(
        String::from_utf8(explicit.stdout).unwrap(),
        "import { one, two } from 'package';"
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
        "import { a } from 'x';\n\nconst first=1;let second=2;run();"
    );

    write(
        &directory.path().join("worsier.jsonc"),
        r#"{"rules":{"importLayout":false,"statementSpacing":{"imports":"off"}}}"#,
    );
    let variables_only = command(directory.path()).arg("sample.ts").output().unwrap();
    assert!(
        variables_only.status.success(),
        "{}",
        stderr(&variables_only)
    );
    assert_eq!(
        String::from_utf8(variables_only.stdout).unwrap(),
        "import{a}from'x';\n\nconst first=1;\nlet second=2;\n\nrun();"
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
            r#"{"rules":{"statementSpacing":{"imports":"preserve"}}}"#,
            "rules.statementSpacing.imports",
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
fn directory_walk_respects_gitignore_config_ignores_and_sorts_diagnostics() {
    let directory = tempfile::tempdir().unwrap();
    write(
        &directory.path().join("worsier.jsonc"),
        r#"{"ignorePatterns":["ignored/**"]}"#,
    );
    write(&directory.path().join(".gitignore"), "gitignored.js\n");
    write(&directory.path().join("ignored/skip.ts"), "const skip=1;");
    write(&directory.path().join("gitignored.js"), "const ignored=1;");
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
    assert!(diagnostics.find("a-invalid.ts").unwrap() < diagnostics.find("z-invalid.ts").unwrap());
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
        "import { included } from 'pkg';"
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

#[cfg(unix)]
#[test]
fn atomic_write_preserves_permissions_and_extended_attributes() {
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
    assert_eq!(after.permissions().mode() & 0o777, 0o640);
    assert_eq!((after.uid(), after.gid()), (before.uid(), before.gid()));
    assert_eq!(
        xattr::get(source, attribute).unwrap().unwrap(),
        b"preserved"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn atomic_write_preserves_access_control_lists() {
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
        "import { value } from 'pkg';\n\nconst raw=[1,2];"
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
    ];

    for (file_name, source) in cases {
        write(&directory.path().join(file_name), source);
        let output = command(directory.path()).arg(file_name).output().unwrap();
        assert!(output.status.success(), "{file_name}: {}", stderr(&output));
        assert_eq!(String::from_utf8(output.stdout).unwrap(), source);
    }
}
