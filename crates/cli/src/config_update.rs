use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use jsonc_parser::cst::{
    CstInputValue, CstNewlineKind, CstNode, CstObject, CstObjectProp, CstRootNode, ObjectPropName,
};
use jsonc_parser::{ParseOptions, parse_to_serde_value};
use serde_json::Value;
use worsier_formatter::{FormatConfig, resolve_config};

use super::{atomic_write_from_source, build_config_ignore, escaped_path};

const DEFAULT_SCHEMA: &str = "./node_modules/worsier/configuration_schema.json";
const LEGACY_V0_KEYS: [&str; 12] = [
    "indentStyle",
    "indentWidth",
    "lineEnding",
    "quoteStyle",
    "semicolons",
    "trailingCommas",
    "bracketSpacing",
    "arrowParentheses",
    "finalNewline",
    "objects",
    "arrays",
    "imports",
];

#[derive(Debug, Eq, PartialEq)]
enum ConfigChange {
    Added(String),
    Migrated(&'static str),
}

impl std::fmt::Display for ConfigChange {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Added(path) => write!(formatter, "Added {path}"),
            Self::Migrated(path) => write!(formatter, "Migrated {path}"),
        }
    }
}

#[derive(Debug)]
struct UpdateResult {
    output: String,
    changes: Vec<ConfigChange>,
}

struct UpdateTarget {
    file: File,
    identity: FileIdentity,
    path: PathBuf,
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct FileIdentity {
    volume: u64,
    index: u64,
}

struct DeferredComments {
    target: CstObjectProp,
    comments: Vec<String>,
}

#[derive(Clone, Copy)]
struct LegacyRules {
    imports: Option<bool>,
    variables: Option<bool>,
}

pub(super) fn complete_config() -> FormatConfig {
    FormatConfig {
        schema: Some(DEFAULT_SCHEMA.to_owned()),
        ..FormatConfig::default()
    }
}

pub(super) fn has_migratable_legacy_keys(value: &Value) -> bool {
    value
        .get("rules")
        .and_then(Value::as_object)
        .is_some_and(|rules| rules.contains_key("imports") || rules.contains_key("variables"))
}

pub(super) fn update_config(path: &Path) -> Result<()> {
    let mut target = resolve_update_target(path)?;
    let mut source = String::new();
    target.file.read_to_string(&mut source).with_context(|| {
        format!(
            "failed to read configuration {}",
            escaped_path(&target.path)
        )
    })?;
    let result = update_config_source(&source, &target.path)?;

    if result.output == source {
        println!(
            "Configuration is up to date: {}",
            escaped_path(&target.path)
        );
        return Ok(());
    }

    write_updated_config(&target, &source, &result.output)?;
    for change in result.changes {
        println!("{change}");
    }
    println!("Updated {}", escaped_path(&target.path));
    Ok(())
}

fn write_updated_config(target: &UpdateTarget, source: &str, output: &str) -> Result<()> {
    atomic_write_from_source(&target.path, output.as_bytes(), &target.file, || {
        verify_update_target_unchanged(target, source)
    })
}

fn resolve_update_target(path: &Path) -> Result<UpdateTarget> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("failed to resolve configuration {}", escaped_path(path)))?;
    if metadata.file_type().is_symlink() {
        bail!(
            "configuration {} is a symbolic link; update its target explicitly",
            escaped_path(path)
        );
    }
    if !metadata.is_file() {
        bail!("configuration {} is not a file", escaped_path(path));
    }
    let file = open_read_only_no_follow(path)
        .with_context(|| format!("failed to open configuration {}", escaped_path(path)))?;
    let opened_metadata = file.metadata()?;
    if !opened_metadata.is_file() {
        bail!(
            "configuration {} changed while it was being opened",
            escaped_path(path)
        );
    }
    #[cfg(unix)]
    if file_identity_from_metadata(&metadata) != file_identity_from_metadata(&opened_metadata) {
        bail!(
            "configuration {} changed while it was being opened",
            escaped_path(path)
        );
    }
    let identity = file_identity(&file)?;
    let absolute_path = path
        .canonicalize()
        .with_context(|| format!("failed to resolve configuration {}", escaped_path(path)))?;
    Ok(UpdateTarget {
        file,
        identity,
        path: absolute_path,
    })
}

fn open_read_only_no_follow(path: &Path) -> io::Result<File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
    }

    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;

        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;

        OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
    }
}

#[cfg(unix)]
fn file_identity(file: &File) -> io::Result<FileIdentity> {
    Ok(file_identity_from_metadata(&file.metadata()?))
}

#[cfg(unix)]
fn file_identity_from_metadata(metadata: &fs::Metadata) -> FileIdentity {
    use std::os::unix::fs::MetadataExt;

    FileIdentity {
        volume: metadata.dev(),
        index: metadata.ino(),
    }
}

#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "GetFileInformationByHandle provides stable Windows file identity for an open handle"
)]
fn file_identity(file: &File) -> io::Result<FileIdentity> {
    use std::os::windows::io::AsRawHandle;

    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };

    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the file owns a valid handle and information points to writable initialized storage.
    let result =
        unsafe { GetFileInformationByHandle(file.as_raw_handle() as HANDLE, &raw mut information) };
    if result == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(FileIdentity {
        volume: u64::from(information.dwVolumeSerialNumber),
        index: u64::from(information.nFileIndexHigh) << 32 | u64::from(information.nFileIndexLow),
    })
}

fn verify_update_target_unchanged(target: &UpdateTarget, source: &str) -> Result<()> {
    let metadata = fs::symlink_metadata(&target.path).with_context(|| {
        format!(
            "configuration {} changed while it was being updated",
            escaped_path(&target.path)
        )
    })?;
    let canonical_path = target.path.canonicalize().with_context(|| {
        format!(
            "configuration {} changed while it was being updated",
            escaped_path(&target.path)
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || canonical_path != target.path {
        bail!(
            "configuration {} changed while it was being updated; no changes were written",
            escaped_path(&target.path)
        );
    }

    let mut current_file = open_read_only_no_follow(&target.path)?;
    let current_metadata = current_file.metadata()?;
    let mut current_source = String::new();
    current_file.read_to_string(&mut current_source)?;
    if !current_metadata.is_file()
        || file_identity(&current_file)? != target.identity
        || current_source != source
    {
        bail!(
            "configuration {} changed while it was being updated; no changes were written",
            escaped_path(&target.path)
        );
    }
    Ok(())
}

fn update_config_source(source: &str, path: &Path) -> Result<UpdateResult> {
    let root = CstRootNode::parse(source, &ParseOptions::default())
        .with_context(|| format!("invalid JSONC configuration {}", escaped_path(path)))?;
    let root_object = root
        .value()
        .and_then(|value| value.as_object())
        .ok_or_else(|| {
            anyhow!(
                "configuration {} must contain an object",
                escaped_path(path)
            )
        })?;
    reject_duplicate_properties(&root_object, "")?;
    let source_value: Value = parse_to_serde_value(source, &ParseOptions::default())
        .with_context(|| format!("invalid JSONC configuration {}", escaped_path(path)))?;

    reject_v0_config(&source_value, path)?;
    let legacy = read_legacy_rules(&source_value)?;
    validate_legacy_conflicts(&source_value, legacy)?;

    let mut changes = Vec::new();
    let mut deferred_comments = Vec::new();
    migrate_v1_rules(&root_object, legacy, &mut changes, &mut deferred_comments)?;

    let template_source = serde_json::to_string_pretty(&complete_config())?;
    let template_root = CstRootNode::parse(&template_source, &ParseOptions::default())?;
    let template_object = template_root
        .value()
        .and_then(|value| value.as_object())
        .expect("serialized FormatConfig must be an object");
    merge_missing_properties(&root_object, &template_object, "", &mut changes)?;
    apply_deferred_comments(deferred_comments, root.newline_kind())?;

    let output = root.to_string();
    validate_updated_config(&output, path)?;
    Ok(UpdateResult { output, changes })
}

fn reject_duplicate_properties(object: &CstObject, parent_path: &str) -> Result<()> {
    let mut names = HashSet::new();
    for property in object.properties() {
        let name = property_name(&property)?;
        let path = if parent_path.is_empty() {
            name.clone()
        } else {
            format!("{parent_path}.{name}")
        };
        if !names.insert(name) {
            bail!("duplicate configuration property {path}");
        }
        if let Some(value) = property.value() {
            reject_duplicate_properties_in_node(&value, &path)?;
        }
    }
    Ok(())
}

fn reject_duplicate_properties_in_node(node: &CstNode, path: &str) -> Result<()> {
    if let Some(object) = node.as_object() {
        reject_duplicate_properties(&object, path)?;
    } else if let Some(array) = node.as_array() {
        for element in array.elements() {
            reject_duplicate_properties_in_node(&element, path)?;
        }
    }
    Ok(())
}

fn reject_v0_config(value: &Value, path: &Path) -> Result<()> {
    let Some(object) = value.as_object() else {
        return Ok(());
    };
    if LEGACY_V0_KEYS.iter().any(|key| object.contains_key(*key))
        || object.contains_key("statementSpacing")
    {
        bail!(
            "configuration {} predates Worsier v1.0 and cannot be updated automatically; create a new configuration with --init and copy supported settings",
            escaped_path(path)
        );
    }
    Ok(())
}

fn read_legacy_rules(value: &Value) -> Result<LegacyRules> {
    let Some(rules) = value.get("rules").and_then(Value::as_object) else {
        return Ok(LegacyRules {
            imports: None,
            variables: None,
        });
    };
    Ok(LegacyRules {
        imports: read_optional_bool(rules.get("imports"), "rules.imports")?,
        variables: read_optional_bool(rules.get("variables"), "rules.variables")?,
    })
}

fn read_optional_bool(value: Option<&Value>, path: &str) -> Result<Option<bool>> {
    match value {
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => bail!("{path} must be a boolean to migrate"),
        None => Ok(None),
    }
}

fn validate_legacy_conflicts(value: &Value, legacy: LegacyRules) -> Result<()> {
    if legacy.imports.is_none() && legacy.variables.is_none() {
        return Ok(());
    }
    let rules = value
        .get("rules")
        .and_then(Value::as_object)
        .expect("legacy rule lookup already established an object");

    if let Some(imports) = legacy.imports {
        ensure_compatible(
            rules.get("importLayout"),
            &Value::Bool(imports),
            "rules.importLayout",
            "rules.imports",
        )?;
    }

    let spacing = match rules.get("statementSpacing") {
        Some(Value::Object(spacing)) => Some(spacing),
        Some(_) => {
            bail!("cannot migrate legacy rules because rules.statementSpacing is not an object")
        }
        None => None,
    };
    if let Some(imports) = legacy.imports {
        ensure_compatible(
            spacing.and_then(|spacing| spacing.get("imports")),
            &Value::String(spacing_mode(imports).to_owned()),
            "rules.statementSpacing.imports",
            "rules.imports",
        )?;
    }
    if let Some(variables) = legacy.variables {
        ensure_compatible(
            spacing.and_then(|spacing| spacing.get("variableDeclarations")),
            &Value::String(spacing_mode(variables).to_owned()),
            "rules.statementSpacing.variableDeclarations",
            "rules.variables",
        )?;
    }
    Ok(())
}

fn ensure_compatible(
    actual: Option<&Value>,
    expected: &Value,
    current_path: &str,
    legacy_path: &str,
) -> Result<()> {
    if let Some(actual) = actual
        && actual != expected
    {
        bail!("cannot migrate {legacy_path} because {current_path} has conflicting value {actual}");
    }
    Ok(())
}

fn spacing_mode(enabled: bool) -> &'static str {
    if enabled { "separate" } else { "off" }
}

fn migrate_v1_rules(
    root: &CstObject,
    legacy: LegacyRules,
    changes: &mut Vec<ConfigChange>,
    deferred_comments: &mut Vec<DeferredComments>,
) -> Result<()> {
    if legacy.imports.is_none() && legacy.variables.is_none() {
        return Ok(());
    }
    let rules = root
        .object_value("rules")
        .ok_or_else(|| anyhow!("rules must be an object to migrate"))?;

    if legacy.imports.is_some() {
        let legacy_property = rules
            .get("imports")
            .expect("legacy property must remain connected before migration");
        if let Some(import_layout) = rules.get("importLayout") {
            defer_removed_comments(&legacy_property, import_layout, deferred_comments);
            legacy_property.remove();
        } else {
            rename_property(&legacy_property, "importLayout")?;
        }
        changes.push(ConfigChange::Migrated("rules.imports"));
    }

    let variables_property = legacy.variables.and_then(|_| rules.get("variables"));
    let mut variables_property_reused = false;
    let spacing = if let Some(spacing) = rules.object_value("statementSpacing") {
        spacing
    } else if let Some(property) = variables_property.clone() {
        variables_property_reused = true;
        rename_property(&property, "statementSpacing")?;
        property
            .value()
            .ok_or_else(|| anyhow!("rules.variables is missing its value"))?
            .as_boolean_lit()
            .ok_or_else(|| anyhow!("rules.variables is not a boolean"))?
            .replace_with(CstInputValue::Object(Vec::new()));
        property
            .object_value()
            .ok_or_else(|| anyhow!("failed to migrate rules.variables"))?
    } else {
        let mut values = Vec::new();
        if let Some(imports) = legacy.imports {
            values.push((
                "imports".to_owned(),
                CstInputValue::String(spacing_mode(imports).to_owned()),
            ));
        }
        if let Some(variables) = legacy.variables {
            values.push((
                "variableDeclarations".to_owned(),
                CstInputValue::String(spacing_mode(variables).to_owned()),
            ));
        }
        let property = rules.append("statementSpacing", CstInputValue::Object(values));
        property
            .object_value()
            .expect("inserted statementSpacing must be an object")
    };

    if let Some(imports) = legacy.imports
        && spacing.get("imports").is_none()
    {
        spacing.insert(
            0,
            "imports",
            CstInputValue::String(spacing_mode(imports).to_owned()),
        );
    }
    if let Some(variables) = legacy.variables
        && spacing.get("variableDeclarations").is_none()
    {
        spacing.append(
            "variableDeclarations",
            CstInputValue::String(spacing_mode(variables).to_owned()),
        );
    }
    if let Some(property) = variables_property
        && !variables_property_reused
        && property.root_node().is_some()
    {
        let variable_declarations = spacing
            .get("variableDeclarations")
            .expect("migration must create rules.statementSpacing.variableDeclarations");
        defer_removed_comments(&property, variable_declarations, deferred_comments);
        property.remove();
    }
    if legacy.variables.is_some() {
        changes.push(ConfigChange::Migrated("rules.variables"));
    }
    Ok(())
}

fn rename_property(property: &CstObjectProp, new_name: &str) -> Result<()> {
    let name = property
        .name()
        .ok_or_else(|| anyhow!("configuration property is missing a name"))?;
    match name {
        ObjectPropName::String(name) => name.set_raw_value(serde_json::to_string(new_name)?),
        ObjectPropName::Word(name) => name.set_raw_value(new_name.to_owned()),
    }
    Ok(())
}

fn defer_removed_comments(
    source: &CstObjectProp,
    target: CstObjectProp,
    deferred_comments: &mut Vec<DeferredComments>,
) {
    let source_node: CstNode = source.clone().into();
    let mut comments = Vec::new();

    let mut leading_comments = source_node
        .leading_comments_same_line()
        .map(|comment| comment.raw_value())
        .collect::<Vec<_>>();
    leading_comments.reverse();
    comments.extend(leading_comments);
    collect_nested_comments(&source_node, &mut comments);

    if let Some(comma) = source.trailing_comma() {
        for sibling in source_node.next_siblings() {
            if let Some(comment) = sibling.as_comment() {
                comments.push(comment.raw_value());
            }
            if sibling.is_comma() {
                break;
            }
        }
        comments.extend(
            CstNode::from(comma)
                .trailing_comments_same_line()
                .map(|comment| comment.raw_value()),
        );
    } else {
        comments.extend(
            source_node
                .trailing_comments_same_line()
                .map(|comment| comment.raw_value()),
        );
    }

    if !comments.is_empty() {
        deferred_comments.push(DeferredComments { target, comments });
    }
}

fn collect_nested_comments(node: &CstNode, comments: &mut Vec<String>) {
    if let Some(comment) = node.as_comment() {
        comments.push(comment.raw_value());
    } else if let CstNode::Container(container) = node {
        for child in container.children() {
            collect_nested_comments(&child, comments);
        }
    }
}

fn apply_deferred_comments(
    deferred_comments: Vec<DeferredComments>,
    newline_kind: CstNewlineKind,
) -> Result<()> {
    let newline = match newline_kind {
        CstNewlineKind::LineFeed => "\n",
        CstNewlineKind::CarriageReturnLineFeed => "\r\n",
    };

    for deferred in deferred_comments {
        let target_node: CstNode = deferred.target.clone().into();
        let indent = target_node
            .previous_siblings()
            .next()
            .and_then(|node| node.as_whitespace())
            .map(|whitespace| whitespace.value())
            .unwrap_or_default();
        let prefix = deferred.comments.join(&format!("{newline}{indent}"));
        let name = deferred
            .target
            .name()
            .ok_or_else(|| anyhow!("migrated configuration property is missing a name"))?;
        let decoded_name = name.decoded_value()?;

        match name {
            ObjectPropName::String(name) => {
                name.set_raw_value(format!("{prefix}{newline}{indent}{}", name.raw_value()));
            }
            ObjectPropName::Word(name) => {
                name.set_raw_value(format!("{prefix}{newline}{indent}{decoded_name}"));
            }
        }
    }

    Ok(())
}

fn merge_missing_properties(
    target: &CstObject,
    template: &CstObject,
    parent_path: &str,
    changes: &mut Vec<ConfigChange>,
) -> Result<()> {
    let template_properties = template.properties();
    let template_names = template_properties
        .iter()
        .map(property_name)
        .collect::<Result<Vec<_>>>()?;

    for (template_index, template_property) in template_properties.into_iter().enumerate() {
        let name = &template_names[template_index];
        let path = if parent_path.is_empty() {
            name.clone()
        } else {
            format!("{parent_path}.{name}")
        };
        if let Some(target_property) = target.get(name) {
            if let (Some(target_object), Some(template_object)) = (
                target_property.object_value(),
                template_property.object_value(),
            ) {
                merge_missing_properties(&target_object, &template_object, &path, changes)?;
            }
            continue;
        }

        let value = template_property
            .value()
            .ok_or_else(|| anyhow!("current configuration template is missing {path}"))?;
        let insert_index = canonical_insert_index(target, &template_names, template_index)?;
        target.insert(insert_index, name, cst_node_to_input(&value)?);
        changes.push(ConfigChange::Added(path));
    }
    Ok(())
}

fn canonical_insert_index(
    target: &CstObject,
    template_names: &[String],
    missing_index: usize,
) -> Result<usize> {
    let properties = target.properties();
    for (target_index, property) in properties.iter().enumerate() {
        let name = property_name(property)?;
        if template_names
            .iter()
            .position(|template_name| template_name == &name)
            .is_some_and(|index| index > missing_index)
        {
            return Ok(target_index);
        }
    }
    Ok(properties.len())
}

fn property_name(property: &jsonc_parser::cst::CstObjectProp) -> Result<String> {
    property
        .name()
        .ok_or_else(|| anyhow!("configuration property is missing a name"))?
        .decoded_value()
        .map_err(anyhow::Error::from)
}

fn cst_node_to_input(node: &CstNode) -> Result<CstInputValue> {
    if let Some(object) = node.as_object() {
        let mut properties = Vec::new();
        for property in object.properties() {
            let name = property_name(&property)?;
            let value = property
                .value()
                .ok_or_else(|| anyhow!("configuration template property {name} has no value"))?;
            properties.push((name, cst_node_to_input(&value)?));
        }
        return Ok(CstInputValue::Object(properties));
    }
    if let Some(array) = node.as_array() {
        return array
            .elements()
            .iter()
            .map(cst_node_to_input)
            .collect::<Result<Vec<_>>>()
            .map(CstInputValue::Array);
    }

    let value: Value = parse_to_serde_value(&node.to_string(), &ParseOptions::default())?;
    serde_value_to_input(&value)
}

fn serde_value_to_input(value: &Value) -> Result<CstInputValue> {
    match value {
        Value::Null => Ok(CstInputValue::Null),
        Value::Bool(value) => Ok(CstInputValue::Bool(*value)),
        Value::Number(value) => Ok(CstInputValue::Number(value.to_string())),
        Value::String(value) => Ok(CstInputValue::String(value.clone())),
        Value::Array(_) | Value::Object(_) => {
            bail!("nested configuration template values must be converted from the CST")
        }
    }
}

fn validate_updated_config(source: &str, path: &Path) -> Result<()> {
    let value: Value = parse_to_serde_value(source, &ParseOptions::default())
        .with_context(|| format!("invalid updated JSONC configuration {}", escaped_path(path)))?;
    let config: FormatConfig = serde_path_to_error::deserialize(value).with_context(|| {
        format!(
            "invalid updated configuration value in {}",
            escaped_path(path)
        )
    })?;
    let resolved = resolve_config(config)
        .with_context(|| format!("invalid updated configuration {}", escaped_path(path)))?;
    build_config_ignore(path, &resolved)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::fs;
    use std::path::Path;

    use jsonc_parser::{ParseOptions, parse_to_serde_value};
    use serde_json::Value;

    use super::{ConfigChange, update_config_source};
    #[cfg(unix)]
    use super::{resolve_update_target, write_updated_config};

    fn parsed_output(source: &str) -> Value {
        let result = update_config_source(source, Path::new("worsier.jsonc")).unwrap();
        parse_to_serde_value(&result.output, &ParseOptions::default()).unwrap()
    }

    #[test]
    fn empty_object_becomes_the_complete_current_config() {
        let result = update_config_source("{}", Path::new("worsier.jsonc")).unwrap();
        assert_eq!(
            result.output,
            "{\n  \"$schema\": \"./node_modules/worsier/configuration_schema.json\",\n  \"lineWidth\": 120,\n  \"verifyAst\": true,\n  \"rules\": {\n    \"importLayout\": true,\n    \"interfaceLayout\": 0,\n    \"statementSpacing\": {\n      \"imports\": \"separate\",\n      \"typeAliases\": \"separate\",\n      \"variableDeclarations\": \"separate\"\n    },\n    \"semicolons\": {\n      \"statements\": \"asNeeded\",\n      \"classMembers\": \"asNeeded\",\n      \"typeMembers\": \"asNeeded\"\n    },\n    \"trailingCommas\": \"never\"\n  },\n  \"ignorePatterns\": []\n}"
        );
        assert_eq!(result.changes.len(), 5);
    }

    #[test]
    fn migrates_v1_rules_and_preserves_comments_and_crlf() {
        let source = "{\r\n  // schema comment\r\n  \"$schema\": \"custom.json\",\r\n  \"lineWidth\": 80,\r\n  \"verifyAst\": false,\r\n  \"rules\": {\r\n    // imports comment\r\n    \"imports\" /* imports key */ : /* imports value */ false, // imports trailing comment\r\n    // variables comment\r\n    \"variables\"/* variables key */:/* variables value */true, // variables trailing comment\r\n  },\r\n  \"ignorePatterns\": [\"generated/**\"],\r\n}";
        let result = update_config_source(source, Path::new("worsier.jsonc")).unwrap();

        assert!(result.output.contains("// schema comment\r\n"));
        assert!(result.output.contains("// imports comment\r\n"));
        assert!(result.output.contains("// imports trailing comment\r\n"));
        assert!(result.output.contains("// variables comment\r\n"));
        assert!(result.output.contains("// variables trailing comment\r\n"));
        assert!(result.output.contains("/* imports key */"));
        assert!(result.output.contains("/* imports value */"));
        assert!(result.output.contains("/* variables key */"));
        assert!(result.output.contains("/* variables value */"));
        assert!(result.output.contains("\"$schema\": \"custom.json\""));
        assert!(result.output.contains("\"lineWidth\": 80"));
        assert!(
            result
                .output
                .contains("\"importLayout\" /* imports key */ : /* imports value */ false")
        );
        assert!(
            result
                .output
                .contains("\"statementSpacing\"/* variables key */:/* variables value */{")
        );
        assert!(result.output.contains("\"imports\": \"off\""));
        assert!(
            result
                .output
                .contains("\"variableDeclarations\": \"separate\"")
        );
        assert!(!result.output.contains("\"variables\""));
        assert!(!result.output.ends_with('\n'));
        assert!(!result.output.replace("\r\n", "").contains('\n'));
        assert!(
            result
                .changes
                .contains(&ConfigChange::Migrated("rules.imports"))
        );
        assert!(
            result
                .changes
                .contains(&ConfigChange::Migrated("rules.variables"))
        );
    }

    #[test]
    fn migrates_each_v1_boolean_mapping() {
        for (source, import_layout, import_spacing, variable_spacing) in [
            (
                r#"{"rules":{"imports":true}}"#,
                true,
                "separate",
                "separate",
            ),
            (r#"{"rules":{"imports":false}}"#, false, "off", "separate"),
            (
                r#"{"rules":{"variables":true}}"#,
                true,
                "separate",
                "separate",
            ),
            (r#"{"rules":{"variables":false}}"#, true, "separate", "off"),
            (
                r#"{"rules":{"imports":false,"variables":false}}"#,
                false,
                "off",
                "off",
            ),
        ] {
            let value = parsed_output(source);
            let rules = value["rules"].as_object().unwrap();
            assert_eq!(rules["importLayout"], import_layout, "{source}");
            assert_eq!(
                rules["statementSpacing"]["imports"], import_spacing,
                "{source}"
            );
            assert_eq!(
                rules["statementSpacing"]["variableDeclarations"], variable_spacing,
                "{source}"
            );
            assert!(!rules.contains_key("imports"), "{source}");
            assert!(!rules.contains_key("variables"), "{source}");
        }
    }

    #[test]
    fn compatible_current_values_remove_legacy_keys_and_conflicts_fail() {
        let compatible = r#"{
  "rules": {
    /* legacy imports leading */ "imports": /* legacy imports inner */ true, // legacy imports comment
    "importLayout": true, // current imports comment
    "statementSpacing": {
      "imports": "separate",
      "variableDeclarations": "off"
    }, // current spacing comment
    "variables": /* legacy variables inner */ false // legacy variables comment
  }
}"#;
        let result = update_config_source(compatible, Path::new("worsier.jsonc")).unwrap();
        assert!(!result.output.contains("\"variables\""));
        assert!(!result.output.contains("\"imports\": true"));
        assert!(result.output.contains("\"variableDeclarations\": \"off\""));
        assert!(result.output.contains("// legacy imports comment"));
        assert!(result.output.contains("/* legacy imports leading */"));
        assert!(result.output.contains("/* legacy imports inner */"));
        assert!(result.output.contains("// current imports comment"));
        assert!(result.output.contains("// current spacing comment"));
        assert!(result.output.contains("/* legacy variables inner */"));
        assert!(result.output.contains("// legacy variables comment"));

        let conflicting = r#"{"rules":{"imports":true,"importLayout":false}}"#;
        let error = update_config_source(conflicting, Path::new("worsier.jsonc")).unwrap_err();
        assert!(error.to_string().contains("rules.importLayout"));
    }

    #[test]
    fn rejects_v0_unknown_invalid_and_non_object_configs() {
        let v0 = r#"{"quoteStyle":"single"}"#;
        assert!(
            update_config_source(v0, Path::new("worsier.jsonc"))
                .unwrap_err()
                .to_string()
                .contains("predates Worsier v1.0")
        );

        let unknown = r#"{"rules":{"unknown":true}}"#;
        assert!(
            format!(
                "{:#}",
                update_config_source(unknown, Path::new("worsier.jsonc")).unwrap_err()
            )
            .contains("rules.unknown")
        );

        let invalid_legacy = r#"{"rules":{"imports":"yes"}}"#;
        assert!(
            update_config_source(invalid_legacy, Path::new("worsier.jsonc"))
                .unwrap_err()
                .to_string()
                .contains("must be a boolean")
        );

        let duplicate_rules = r#"{"rules":{},"rules":{"imports":true}}"#;
        assert!(
            update_config_source(duplicate_rules, Path::new("worsier.jsonc"))
                .unwrap_err()
                .to_string()
                .contains("duplicate configuration property rules")
        );

        let invalid_ignore = r#"{"ignorePatterns":["[z-a]"]}"#;
        assert!(
            format!(
                "{:#}",
                update_config_source(invalid_ignore, Path::new("worsier.jsonc")).unwrap_err()
            )
            .contains("invalid ignore pattern")
        );

        assert!(update_config_source("[]", Path::new("worsier.jsonc")).is_err());
        assert!(update_config_source("{", Path::new("worsier.jsonc")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn ancestor_redirect_after_opening_target_does_not_replace_the_victim() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().unwrap();
        let selected_directory = directory.path().join("selected");
        let moved_directory = directory.path().join("selected-moved");
        let victim_directory = directory.path().join("victim");
        fs::create_dir(&selected_directory).unwrap();
        fs::create_dir(&victim_directory).unwrap();
        let selected_path = selected_directory.join("worsier.jsonc");
        let victim_path = victim_directory.join("worsier.jsonc");
        let source = r#"{"lineWidth":80}"#;
        let victim_source = r#"{"lineWidth":90}"#;
        fs::write(&selected_path, source).unwrap();
        fs::write(&victim_path, victim_source).unwrap();
        let target = resolve_update_target(&selected_path).unwrap();

        fs::rename(&selected_directory, &moved_directory).unwrap();
        symlink(&victim_directory, &selected_directory).unwrap();

        let error = write_updated_config(&target, source, r#"{"lineWidth":100}"#).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("changed while it was being updated")
        );
        assert_eq!(fs::read_to_string(victim_path).unwrap(), victim_source);
        assert_eq!(
            fs::read_to_string(moved_directory.join("worsier.jsonc")).unwrap(),
            source
        );
    }
}
