use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use clap::Parser;
use ignore::WalkBuilder;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use rayon::prelude::*;
use tempfile::NamedTempFile;
use worsier_formatter::{FormatConfig, ResolvedConfig, format_text, resolve_config};

const CONFIG_FILE: &str = "worsier.jsonc";

#[derive(Debug, Parser)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent clap flags map directly to the documented CLI surface"
)]
#[command(
    name = "worsier",
    version,
    about = "Format JavaScript and TypeScript imports and variable boundaries with Worsier"
)]
struct Args {
    /// Create an optional complete worsier.jsonc in the current directory.
    #[arg(long)]
    init: bool,

    /// Use one configuration file for the whole invocation.
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,

    /// Check files without writing them.
    #[arg(long, conflicts_with = "write")]
    check: bool,

    /// Format files in place.
    #[arg(long, conflicts_with = "check")]
    write: bool,

    /// Read stdin and parse it as this path.
    #[arg(long, value_name = "PATH")]
    stdin_filepath: Option<PathBuf>,

    /// Number of formatting worker threads.
    #[arg(long, value_name = "COUNT")]
    threads: Option<usize>,

    /// Disable output AST verification for this invocation.
    #[arg(long)]
    no_verify: bool,

    #[arg(value_name = "PATH")]
    paths: Vec<PathBuf>,
}

pub fn run<I, S>(args: I) -> i32
where
    I: IntoIterator<Item = S>,
    S: Into<OsString> + Clone,
{
    match Args::try_parse_from(args) {
        Ok(args) => match run_args(&args) {
            Ok(exit_code) => exit_code,
            Err(error) => {
                eprintln!("error: {error:#}");
                2
            }
        },
        Err(error) => {
            let exit_code = if error.use_stderr() { 2 } else { 0 };
            let _ = error.print();
            exit_code
        }
    }
}

fn run_args(args: &Args) -> Result<i32> {
    if args.init {
        if !args.paths.is_empty() || args.check || args.write || args.stdin_filepath.is_some() {
            bail!("--init cannot be combined with formatting inputs or modes");
        }
        init_config(&std::env::current_dir()?)?;
        return Ok(0);
    }

    if args.threads == Some(0) {
        bail!("--threads must be greater than zero");
    }
    if let Some(stdin_path) = &args.stdin_filepath {
        if !args.paths.is_empty() || args.write {
            bail!("--stdin-filepath cannot be combined with paths or --write");
        }
        return format_stdin(args, stdin_path);
    }
    if args.paths.is_empty() {
        bail!("provide a file, a directory, or --stdin-filepath");
    }
    if !args.check && !args.write && (args.paths.len() != 1 || args.paths[0].is_dir()) {
        bail!("directories and multiple files require --check or --write");
    }

    let explicit_single_file = args.paths.len() == 1 && args.paths[0].is_file();
    let candidates = collect_files(&args.paths)?;
    let explicit_config = args
        .config
        .as_deref()
        .map(|path| load_config(path, args.no_verify))
        .transpose()?;
    if candidates.is_empty() {
        if explicit_single_file {
            bail!(
                "{} is not a supported JavaScript or TypeScript file",
                escaped_path(&args.paths[0])
            );
        }
        return Ok(0);
    }

    let mut config_cache = ConfigCache::default();
    let mut jobs = Vec::new();
    for candidate in candidates {
        let loaded = match &explicit_config {
            Some(config) => Arc::clone(config),
            None => config_cache.discover(&candidate.path, args.no_verify)?,
        };
        if candidate.discovered && loaded.is_ignored(&candidate.path)? {
            continue;
        }
        jobs.push(Job {
            path: candidate.path,
            config: Arc::clone(&loaded.config),
        });
    }

    run_jobs(args, &jobs)
}

fn run_jobs(args: &Args, jobs: &[Job]) -> Result<i32> {
    let mode = if args.write {
        JobMode::Write
    } else if args.check {
        JobMode::Check
    } else {
        JobMode::Stdout
    };
    let run = || {
        jobs.par_iter()
            .map(|job| format_job(job, mode))
            .collect::<Vec<_>>()
    };
    let mut results = if let Some(threads) = args.threads {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()?
            .install(run)
    } else {
        run()
    };
    results.sort_by(|left, right| result_path(left).cmp(result_path(right)));

    let mut changed = false;
    let mut failed = false;
    for result in results {
        match result {
            JobResult::Clean { source, .. } => {
                if let Some(source) = source {
                    print!("{source}");
                }
            }
            JobResult::Changed { path, output } => {
                changed = true;
                if args.write {
                    eprintln!("Formatted {}", escaped_path(&path));
                } else if args.check {
                    eprintln!("Would format {}", escaped_path(&path));
                } else if let Some(output) = output {
                    print!("{output}");
                }
            }
            JobResult::Failed { path, error } => {
                failed = true;
                eprintln!("{}: {error:#}", escaped_path(&path));
            }
        }
    }

    Ok(if failed {
        2
    } else {
        i32::from(args.check && changed)
    })
}

fn format_stdin(args: &Args, file_name: &Path) -> Result<i32> {
    let mut source = String::new();
    io::stdin().read_to_string(&mut source)?;
    let config = match args.config.as_deref() {
        Some(path) => load_config_with_override(path, args.no_verify)?,
        None => load_discovered_config(file_name, args.no_verify)?,
    };
    match format_text(file_name, &source, &config) {
        Ok(Some(output)) => {
            if args.check {
                Ok(1)
            } else {
                print!("{output}");
                Ok(0)
            }
        }
        Ok(None) => {
            if !args.check {
                print!("{source}");
            }
            Ok(0)
        }
        Err(error) => Err(anyhow!(error)),
    }
}

fn init_config(directory: &Path) -> Result<()> {
    let path = directory.join(CONFIG_FILE);
    let config = FormatConfig {
        schema: Some("./node_modules/worsier/configuration_schema.json".to_owned()),
        ..FormatConfig::default()
    };
    let json = serde_json::to_string_pretty(&config)?;
    let file = OpenOptions::new().write(true).create_new(true).open(&path);
    let mut file = match file {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            bail!("{} already exists", escaped_path(&path));
        }
        Err(error) => {
            return Err(error).with_context(|| format!("failed to create {}", escaped_path(&path)));
        }
    };
    file.write_all(format!("{json}\n").as_bytes())?;
    file.sync_all()?;
    println!("Created {}", escaped_path(&path));
    Ok(())
}

#[derive(Debug)]
struct Candidate {
    path: PathBuf,
    discovered: bool,
}

fn collect_files(paths: &[PathBuf]) -> Result<Vec<Candidate>> {
    let mut files = Vec::new();
    for path in paths {
        if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            bail!(
                "{} is a symbolic link; format its target explicitly",
                escaped_path(path)
            );
        }
        if path.is_file() {
            if is_supported(path) {
                files.push(Candidate {
                    path: path.clone(),
                    discovered: false,
                });
            } else {
                bail!(
                    "{} is not a supported JavaScript or TypeScript file",
                    escaped_path(path)
                );
            }
            continue;
        }
        if !path.is_dir() {
            bail!("{} does not exist", escaped_path(path));
        }

        let walker = WalkBuilder::new(path)
            .hidden(false)
            .git_ignore(true)
            .git_exclude(true)
            .require_git(false)
            .filter_entry(|entry| {
                !matches!(entry.file_name().to_str(), Some(".git" | "node_modules"))
            })
            .build();
        for entry in walker {
            let entry = entry.with_context(|| format!("failed to walk {}", escaped_path(path)))?;
            if entry.file_type().is_some_and(|kind| kind.is_file()) && is_supported(entry.path()) {
                files.push(Candidate {
                    path: entry.into_path(),
                    discovered: true,
                });
            }
        }
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    files.dedup_by(|duplicate, retained| {
        if retained.path == duplicate.path {
            // Vec::dedup_by passes the later duplicate first. Any explicit occurrence wins.
            retained.discovered = retained.discovered && duplicate.discovered;
            true
        } else {
            false
        }
    });
    Ok(files)
}

fn is_supported(path: &Path) -> bool {
    matches!(
        path.extension().and_then(std::ffi::OsStr::to_str),
        Some("js" | "mjs" | "cjs" | "jsx" | "ts" | "mts" | "cts" | "tsx")
    )
}

fn load_discovered_config(file: &Path, no_verify: bool) -> Result<Arc<ResolvedConfig>> {
    Ok(ConfigCache::default()
        .discover(file, no_verify)?
        .config
        .clone())
}

fn load_config_with_override(path: &Path, no_verify: bool) -> Result<Arc<ResolvedConfig>> {
    Ok(load_config(path, no_verify)?.config.clone())
}

#[derive(Default)]
struct ConfigCache {
    by_directory: HashMap<PathBuf, Arc<LoadedConfig>>,
}

impl ConfigCache {
    fn discover(&mut self, file: &Path, no_verify: bool) -> Result<Arc<LoadedConfig>> {
        let start = if file.is_dir() {
            file
        } else {
            file.parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."))
        };
        let mut current = start
            .canonicalize()
            .with_context(|| format!("failed to resolve {}", escaped_path(start)))?;
        let mut visited = Vec::new();

        let loaded = loop {
            if let Some(config) = self.by_directory.get(&current) {
                break Arc::clone(config);
            }
            visited.push(current.clone());
            let candidate = current.join(CONFIG_FILE);
            if candidate.is_file() {
                break load_config(&candidate, no_verify)?;
            }
            let at_vcs_root = current.join(".git").exists();
            if at_vcs_root || !current.pop() {
                break load_default_config(&current, no_verify)?;
            }
        };

        for directory in visited {
            self.by_directory.insert(directory, Arc::clone(&loaded));
        }
        Ok(loaded)
    }
}

fn load_default_config(directory: &Path, no_verify: bool) -> Result<Arc<LoadedConfig>> {
    let mut config = FormatConfig::default();
    if no_verify {
        config.verify_ast = false;
    }
    let resolved = resolve_config(config).context("invalid default configuration")?;
    let ignore = GitignoreBuilder::new(directory).build()?;
    Ok(Arc::new(LoadedConfig {
        config: Arc::new(resolved),
        ignore,
    }))
}

struct LoadedConfig {
    config: Arc<ResolvedConfig>,
    ignore: Gitignore,
}

impl LoadedConfig {
    fn is_ignored(&self, path: &Path) -> Result<bool> {
        let absolute_path = path
            .canonicalize()
            .with_context(|| format!("failed to resolve {}", escaped_path(path)))?;
        Ok(self
            .ignore
            .matched_path_or_any_parents(absolute_path, false)
            .is_ignore())
    }
}

fn load_config(path: &Path, no_verify: bool) -> Result<Arc<LoadedConfig>> {
    let absolute_path = path
        .canonicalize()
        .with_context(|| format!("failed to resolve configuration {}", escaped_path(path)))?;
    let source = fs::read_to_string(&absolute_path).with_context(|| {
        format!(
            "failed to read configuration {}",
            escaped_path(&absolute_path)
        )
    })?;
    let value: serde_json::Value =
        jsonc_parser::parse_to_serde_value(&source, &jsonc_parser::ParseOptions::default())
            .with_context(|| {
                format!(
                    "invalid JSONC configuration {}",
                    escaped_path(&absolute_path)
                )
            })?;
    let mut config: FormatConfig = serde_path_to_error::deserialize(value).with_context(|| {
        format!(
            "invalid configuration value in {}",
            escaped_path(&absolute_path)
        )
    })?;
    if no_verify {
        config.verify_ast = false;
    }
    let resolved = resolve_config(config)
        .with_context(|| format!("invalid configuration {}", escaped_path(&absolute_path)))?;
    let directory = absolute_path.parent().unwrap_or_else(|| Path::new("."));
    let mut ignore = GitignoreBuilder::new(directory);
    for pattern in resolved.ignore_patterns() {
        ignore
            .add_line(Some(absolute_path.clone()), pattern)
            .with_context(|| format!("invalid ignore pattern {pattern:?}"))?;
    }
    Ok(Arc::new(LoadedConfig {
        config: Arc::new(resolved),
        ignore: ignore.build()?,
    }))
}

struct Job {
    path: PathBuf,
    config: Arc<ResolvedConfig>,
}

#[derive(Clone, Copy)]
enum JobMode {
    Check,
    Write,
    Stdout,
}

enum JobResult {
    Clean {
        path: PathBuf,
        source: Option<String>,
    },
    Changed {
        path: PathBuf,
        output: Option<String>,
    },
    Failed {
        path: PathBuf,
        error: anyhow::Error,
    },
}

fn format_job(job: &Job, mode: JobMode) -> JobResult {
    let source = match fs::read_to_string(&job.path)
        .with_context(|| format!("failed to read {}", escaped_path(&job.path)))
    {
        Ok(source) => source,
        Err(error) => {
            return JobResult::Failed {
                path: job.path.clone(),
                error,
            };
        }
    };
    let result = format_text(&job.path, &source, &job.config).map_err(anyhow::Error::from);
    match result {
        Ok(Some(output)) => {
            if matches!(mode, JobMode::Write)
                && let Err(error) = atomic_write(&job.path, output.as_bytes())
            {
                return JobResult::Failed {
                    path: job.path.clone(),
                    error,
                };
            }
            JobResult::Changed {
                path: job.path.clone(),
                output: matches!(mode, JobMode::Stdout).then_some(output),
            }
        }
        Ok(None) => JobResult::Clean {
            path: job.path.clone(),
            source: matches!(mode, JobMode::Stdout).then_some(source),
        },
        Err(error) => JobResult::Failed {
            path: job.path.clone(),
            error,
        },
    }
}

fn result_path(result: &JobResult) -> &Path {
    match result {
        JobResult::Clean { path, .. }
        | JobResult::Changed { path, .. }
        | JobResult::Failed { path, .. } => path,
    }
}

fn atomic_write(path: &Path, output: &[u8]) -> Result<()> {
    atomic_write_with_before_replace(path, output, || Ok(()))
}

fn atomic_write_with_before_replace(
    path: &Path,
    output: &[u8],
    before_replace: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(output)?;
    copy_file_metadata(path, temporary.path(), temporary.as_file())?;
    temporary.as_file_mut().sync_all()?;
    before_replace()?;

    #[cfg(windows)]
    {
        replace_file_windows(path, temporary)
    }

    #[cfg(unix)]
    {
        temporary
            .persist(path)
            .map_err(|error| error.error)
            .with_context(|| format!("failed to replace {}", escaped_path(path)))?;
        File::open(parent)?.sync_all()?;
        Ok(())
    }
}

#[cfg(target_os = "macos")]
#[allow(
    unsafe_code,
    reason = "copyfile(3) is the platform API that preserves ACLs and extended metadata"
)]
fn copy_file_metadata(path: &Path, _temporary_path: &Path, temporary: &File) -> Result<()> {
    use std::os::fd::AsRawFd;

    let source = File::open(path)?;
    // SAFETY: both descriptors remain open for the duration of fcopyfile, the state pointer is
    // null as permitted by copyfile(3), and COPYFILE_METADATA copies no file contents.
    let result = unsafe {
        libc::fcopyfile(
            source.as_raw_fd(),
            temporary.as_raw_fd(),
            std::ptr::null_mut(),
            libc::COPYFILE_METADATA,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error()).context("failed to preserve file metadata")
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
#[allow(
    unsafe_code,
    reason = "fchown is required to preserve the original file owner before atomic replacement"
)]
fn copy_file_metadata(path: &Path, temporary_path: &Path, temporary: &File) -> Result<()> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::MetadataExt;

    let metadata = fs::metadata(path)?;
    // SAFETY: the descriptor is owned by `temporary` and uid/gid come directly from metadata.
    let result = unsafe { libc::fchown(temporary.as_raw_fd(), metadata.uid(), metadata.gid()) };
    if result != 0 {
        return Err(io::Error::last_os_error()).context("failed to preserve file ownership");
    }
    temporary.set_permissions(metadata.permissions())?;
    for name in xattr::list(path)? {
        if let Some(value) = xattr::get(path, &name)? {
            xattr::set(temporary_path, &name, &value)?;
        }
    }
    Ok(())
}

#[cfg(windows)]
fn copy_file_metadata(_path: &Path, _temporary_path: &Path, _temporary: &File) -> Result<()> {
    // ReplaceFileW merges the original file's attributes, ACLs, encryption, and named streams.
    Ok(())
}

#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "ReplaceFileW is the platform API that atomically preserves Windows file metadata"
)]
fn replace_file_windows(path: &Path, temporary: NamedTempFile) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Storage::FileSystem::ReplaceFileW;

    let (replacement, replacement_path) = temporary.keep()?;
    replacement.sync_all()?;
    drop(replacement);
    let replaced_wide = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let replacement_wide = replacement_path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    // SAFETY: both paths are valid, nul-terminated UTF-16 buffers; optional pointers are null.
    let result = unsafe {
        ReplaceFileW(
            replaced_wide.as_ptr(),
            replacement_wide.as_ptr(),
            std::ptr::null(),
            0,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if result == 0 {
        let error = io::Error::last_os_error();
        let _ = fs::remove_file(replacement_path);
        return Err(error).with_context(|| format!("failed to replace {}", escaped_path(path)));
    }

    Ok(())
}

fn escaped_path(path: &Path) -> String {
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::{Arc, Barrier};
    use std::thread;

    use super::{
        CONFIG_FILE, Job, JobMode, JobResult, atomic_write_with_before_replace, escaped_path,
        format_job, init_config, load_config_with_override, load_discovered_config,
    };
    use worsier_formatter::{FormatConfig, resolve_config};

    #[test]
    fn init_writes_a_loadable_complete_config_without_overwriting() {
        let directory = tempfile::tempdir().unwrap();
        init_config(directory.path()).unwrap();
        let path = directory.path().join(CONFIG_FILE);
        assert!(load_config_with_override(&path, false).is_ok());
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "{\n  \"$schema\": \"./node_modules/worsier/configuration_schema.json\",\n  \"lineWidth\": 120,\n  \"verifyAst\": true,\n  \"rules\": {\n    \"importLayout\": true,\n    \"statementSpacing\": {\n      \"imports\": \"separate\",\n      \"variableDeclarations\": \"separate\"\n    }\n  },\n  \"ignorePatterns\": []\n}\n"
        );
        assert!(init_config(directory.path()).is_err());
    }

    #[test]
    fn concurrent_init_has_exactly_one_winner() {
        let directory = tempfile::tempdir().unwrap();
        let path = Arc::new(directory.path().to_owned());
        let barrier = Arc::new(Barrier::new(16));
        let handles = (0..16)
            .map(|_| {
                let path = Arc::clone(&path);
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    barrier.wait();
                    init_config(&path).is_ok()
                })
            })
            .collect::<Vec<_>>();
        let successes = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .filter(|success| *success)
            .count();

        assert_eq!(successes, 1);
        assert!(load_config_with_override(&path.join(CONFIG_FILE), false).is_ok());
    }

    #[test]
    fn default_config_honors_no_verify_override() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir(directory.path().join(".git")).unwrap();
        let path = directory.path().join("sample.ts");
        fs::write(&path, "const value=1;").unwrap();

        assert!(load_discovered_config(&path, false).unwrap().verify_ast());
        assert!(!load_discovered_config(&path, true).unwrap().verify_ast());
    }

    #[test]
    fn check_jobs_do_not_retain_source_sized_payloads() {
        let directory = tempfile::tempdir().unwrap();
        let clean = directory.path().join("clean.ts");
        fs::write(&clean, "import { value } from 'pkg';").unwrap();
        let config = resolve_config(FormatConfig::default()).unwrap();
        let clean_job = Job {
            path: clean,
            config: Arc::new(config.clone()),
        };
        assert!(matches!(
            format_job(&clean_job, JobMode::Check),
            JobResult::Clean { source: None, .. }
        ));

        let changed = directory.path().join("changed.ts");
        fs::write(&changed, "import{value}from'pkg';").unwrap();
        let changed_job = Job {
            path: changed,
            config: Arc::new(config),
        };
        assert!(matches!(
            format_job(&changed_job, JobMode::Check),
            JobResult::Changed { output: None, .. }
        ));
    }

    #[test]
    fn diagnostic_paths_escape_terminal_control_characters() {
        assert_eq!(
            escaped_path(std::path::Path::new("source\u{1b}[31m.ts")),
            "source\\u{1b}[31m.ts"
        );
    }

    #[test]
    fn atomic_write_failure_before_replace_leaves_original_unchanged() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("sample.ts");
        fs::write(&path, "original").unwrap();

        let error = atomic_write_with_before_replace(&path, b"replacement", || {
            Err(anyhow::anyhow!("injected failure"))
        })
        .unwrap_err();

        assert!(error.to_string().contains("injected failure"));
        assert_eq!(fs::read_to_string(path).unwrap(), "original");
    }
}
