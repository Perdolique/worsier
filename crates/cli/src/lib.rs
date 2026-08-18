use std::collections::HashMap;
#[cfg(unix)]
use std::ffi::CString;
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
#[cfg(unix)]
use tempfile::Builder as TempFileBuilder;
#[cfg(windows)]
use tempfile::NamedTempFile;
use worsier_formatter::{
    FormatConfig, ResolvedConfig, format_text, is_supported_path, resolve_config,
};

mod config_update;

use config_update::{complete_config, has_migratable_legacy_keys, update_config};

const CONFIG_FILE: &str = "worsier.jsonc";
const BUILT_IN_IGNORED_ENTRY_NAMES: [&str; 3] =
    [".git", "node_modules", "worker-configuration.d.ts"];

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct FileIdentity {
    volume: u64,
    index: u64,
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

fn open_read_write_no_follow(path: &Path) -> io::Result<File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
    }

    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;

        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;

        OpenOptions::new()
            .read(true)
            .write(true)
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

#[derive(Debug, Parser)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent clap flags map directly to the documented CLI surface"
)]
#[command(
    name = "worsier",
    version,
    about = "Format JavaScript, TypeScript, and inline Vue scripts with focused Worsier rules"
)]
struct Args {
    /// Create an optional complete worsier.jsonc in the current directory.
    #[arg(long)]
    init: bool,

    /// Migrate and complete an existing worsier.jsonc.
    #[arg(
        long,
        conflicts_with_all = [
            "init",
            "check",
            "write",
            "stdin_filepath",
            "threads",
            "no_verify",
            "paths"
        ]
    )]
    update_config: bool,

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

    if args.update_config {
        let path = match &args.config {
            Some(path) => path.clone(),
            None => std::env::current_dir()?.join(CONFIG_FILE),
        };
        update_config(&path)?;
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
                "{} is not a supported JavaScript, TypeScript, or Vue file",
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
            identity: identify_source_path(&candidate.path)?,
            path: candidate.path,
            config: Arc::clone(&loaded.config),
        });
    }

    reject_duplicate_job_targets(&jobs)?;
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
    let config = complete_config();
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
            if is_supported_path(path) {
                files.push(Candidate {
                    path: path.clone(),
                    discovered: false,
                });
            } else {
                bail!(
                    "{} is not a supported JavaScript, TypeScript, or Vue file",
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
                !entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| BUILT_IN_IGNORED_ENTRY_NAMES.contains(&name))
            })
            .build();
        for entry in walker {
            let entry = entry.with_context(|| format!("failed to walk {}", escaped_path(path)))?;
            if entry.file_type().is_some_and(|kind| kind.is_file())
                && is_supported_path(entry.path())
            {
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

fn identify_source_path(path: &Path) -> Result<FileIdentity> {
    let file = open_read_only_no_follow(path)
        .with_context(|| format!("failed to open {}", escaped_path(path)))?;
    let metadata = file.metadata()?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!("{} changed while it was being opened", escaped_path(path));
    }
    Ok(file_identity(&file)?)
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
    let contains_migratable_legacy_rules = has_migratable_legacy_keys(&value);
    let mut config: FormatConfig = match serde_path_to_error::deserialize(value) {
        Ok(config) => config,
        Err(error) => {
            let error = anyhow!(error).context(format!(
                "invalid configuration value in {}",
                escaped_path(&absolute_path)
            ));
            if contains_migratable_legacy_rules {
                return Err(error.context(format!(
                    "legacy Worsier v1 rules can be migrated with `worsier --update-config --config <PATH>`; configuration path: {}",
                    escaped_path(&absolute_path)
                )));
            }
            return Err(error);
        }
    };
    if no_verify {
        config.verify_ast = false;
    }
    let resolved = resolve_config(config)
        .with_context(|| format!("invalid configuration {}", escaped_path(&absolute_path)))?;
    let ignore = build_config_ignore(&absolute_path, &resolved)?;
    Ok(Arc::new(LoadedConfig {
        config: Arc::new(resolved),
        ignore,
    }))
}

fn build_config_ignore(path: &Path, config: &ResolvedConfig) -> Result<Gitignore> {
    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    let mut ignore = GitignoreBuilder::new(directory);
    for pattern in config.ignore_patterns() {
        ignore
            .add_line(Some(path.to_owned()), pattern)
            .with_context(|| format!("invalid ignore pattern {pattern:?}"))?;
    }
    Ok(ignore.build()?)
}

struct Job {
    path: PathBuf,
    config: Arc<ResolvedConfig>,
    identity: FileIdentity,
}

fn reject_duplicate_job_targets(jobs: &[Job]) -> Result<()> {
    let mut paths_by_identity = HashMap::new();
    for job in jobs {
        if let Some(existing) = paths_by_identity.insert(job.identity, &job.path) {
            bail!(
                "{} and {} identify the same file; pass each source file once",
                escaped_path(existing),
                escaped_path(&job.path)
            );
        }
    }
    Ok(())
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
    let source = match read_source(job) {
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
                && let Err(error) = write_source_direct(&job.path, job.identity, output.as_bytes())
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

fn read_source(job: &Job) -> Result<String> {
    let mut file = open_validated_source(&job.path, job.identity, false)?;
    let mut source = String::new();
    file.read_to_string(&mut source)
        .with_context(|| format!("failed to read {}", escaped_path(&job.path)))?;
    Ok(source)
}

fn write_source_direct(path: &Path, identity: FileIdentity, output: &[u8]) -> Result<()> {
    let mut file = open_validated_source(path, identity, true)?;
    file.set_len(0)
        .with_context(|| format!("failed to truncate {}", escaped_path(path)))?;
    file.write_all(output)
        .with_context(|| format!("failed to write {}", escaped_path(path)))?;
    Ok(())
}

fn open_validated_source(path: &Path, identity: FileIdentity, writable: bool) -> Result<File> {
    let file = if writable {
        open_read_write_no_follow(path)
    } else {
        open_read_only_no_follow(path)
    }
    .with_context(|| format!("failed to open {}", escaped_path(path)))?;
    let metadata = file.metadata()?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || file_identity(&file)? != identity
    {
        bail!(
            "{} changed while it was being formatted; no changes were written",
            escaped_path(path)
        );
    }
    Ok(file)
}

fn result_path(result: &JobResult) -> &Path {
    match result {
        JobResult::Clean { path, .. }
        | JobResult::Changed { path, .. }
        | JobResult::Failed { path, .. } => path,
    }
}

#[cfg(test)]
fn atomic_write_with_before_replace(
    path: &Path,
    output: &[u8],
    before_replace: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let source = File::open(path)?;
    atomic_write_from_source(path, output, &source, before_replace)
}

fn atomic_write_from_source(
    path: &Path,
    output: &[u8],
    source: &File,
    before_replace: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));

    #[cfg(windows)]
    {
        let mut temporary = NamedTempFile::new_in(parent)?;
        temporary.write_all(output)?;
        copy_file_metadata(source, temporary.as_file())?;
        temporary.as_file_mut().sync_all()?;
        replace_file_windows(path, temporary, before_replace)
    }

    #[cfg(unix)]
    {
        let directory = File::open(parent)?;
        let mut temporary = AnchoredTempFile::new(parent, &directory)?;
        temporary.file.write_all(output)?;
        copy_file_metadata(source, &temporary.file)?;
        temporary.file.sync_all()?;
        temporary.persist(path, before_replace)
    }
}

#[cfg(unix)]
struct AnchoredTempFile {
    directory: File,
    file: File,
    name: CString,
    remove_on_drop: bool,
}

#[cfg(unix)]
impl AnchoredTempFile {
    #[allow(
        unsafe_code,
        reason = "openat binds temporary-file creation to the already opened target directory"
    )]
    fn new(parent: &Path, directory: &File) -> Result<Self> {
        use std::os::fd::{AsRawFd, FromRawFd};
        use std::os::unix::ffi::OsStrExt;

        let directory_fd = directory.as_raw_fd();
        let temporary =
            TempFileBuilder::new()
                .prefix(".worsier-")
                .make_in(parent, |candidate| {
                    let name = candidate.file_name().ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidInput, "temporary file has no name")
                    })?;
                    let name = CString::new(name.as_bytes()).map_err(|_| {
                        io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "temporary file name contains a null byte",
                        )
                    })?;
                    // SAFETY: directory_fd remains open for the closure, name is nul-terminated, and
                    // O_EXCL prevents an existing entry from being opened or replaced.
                    let descriptor = unsafe {
                        libc::openat(
                            directory_fd,
                            name.as_ptr(),
                            libc::O_CLOEXEC | libc::O_CREAT | libc::O_EXCL | libc::O_RDWR,
                            0o600,
                        )
                    };
                    if descriptor == -1 {
                        return Err(io::Error::last_os_error());
                    }
                    // SAFETY: openat returned a new owned descriptor.
                    Ok(unsafe { File::from_raw_fd(descriptor) })
                })?;
        let (file, path) = temporary.keep().map_err(|error| error.error)?;
        let name = path
            .file_name()
            .ok_or_else(|| anyhow!("temporary file has no name"))?;
        let name = CString::new(name.as_bytes())
            .map_err(|_| anyhow!("temporary file name contains a null byte"))?;
        Ok(Self {
            directory: directory.try_clone()?,
            file,
            name,
            remove_on_drop: true,
        })
    }

    #[allow(
        unsafe_code,
        reason = "renameat atomically replaces the target inside the already opened directory"
    )]
    fn persist(mut self, path: &Path, before_replace: impl FnOnce() -> Result<()>) -> Result<()> {
        use std::os::fd::AsRawFd;
        use std::os::unix::ffi::OsStrExt;

        let target_name = path
            .file_name()
            .ok_or_else(|| anyhow!("configuration path has no file name"))?;
        let target_name = CString::new(target_name.as_bytes())
            .map_err(|_| anyhow!("configuration file name contains a null byte"))?;
        before_replace()?;
        // SAFETY: both names are nul-terminated and both directory descriptors remain open.
        let result = unsafe {
            libc::renameat(
                self.directory.as_raw_fd(),
                self.name.as_ptr(),
                self.directory.as_raw_fd(),
                target_name.as_ptr(),
            )
        };
        if result != 0 {
            return Err(io::Error::last_os_error())
                .with_context(|| format!("failed to replace {}", escaped_path(path)));
        }
        self.remove_on_drop = false;
        self.directory.sync_all()?;
        Ok(())
    }
}

#[cfg(unix)]
#[allow(
    unsafe_code,
    reason = "unlinkat removes an abandoned temporary file from the anchored directory"
)]
impl Drop for AnchoredTempFile {
    fn drop(&mut self) {
        if self.remove_on_drop {
            use std::os::fd::AsRawFd;

            // SAFETY: the directory descriptor remains open and name is nul-terminated.
            let _ = unsafe { libc::unlinkat(self.directory.as_raw_fd(), self.name.as_ptr(), 0) };
        }
    }
}

#[cfg(target_os = "macos")]
#[allow(
    unsafe_code,
    reason = "copyfile(3) is the platform API that preserves ACLs and extended metadata"
)]
fn copy_file_metadata(source: &File, temporary: &File) -> Result<()> {
    use std::os::fd::AsRawFd;

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
fn copy_file_metadata(source: &File, temporary: &File) -> Result<()> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::MetadataExt;
    use xattr::FileExt;

    let metadata = source.metadata()?;
    // SAFETY: the descriptor is owned by `temporary` and uid/gid come directly from metadata.
    let result = unsafe { libc::fchown(temporary.as_raw_fd(), metadata.uid(), metadata.gid()) };
    if result != 0 {
        return Err(io::Error::last_os_error()).context("failed to preserve file ownership");
    }
    temporary.set_permissions(metadata.permissions())?;
    for name in source.list_xattr()? {
        if let Some(value) = source.get_xattr(&name)? {
            temporary.set_xattr(&name, &value)?;
        }
    }
    Ok(())
}

#[cfg(windows)]
#[allow(
    clippy::unnecessary_wraps,
    reason = "all platform implementations share a fallible metadata-copy call site"
)]
fn copy_file_metadata(_source: &File, _temporary: &File) -> Result<()> {
    // ReplaceFileW merges the original file's attributes, ACLs, encryption, and named streams.
    Ok(())
}

#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "ReplaceFileW is the platform API that atomically preserves Windows file metadata"
)]
fn replace_file_windows(
    path: &Path,
    temporary: NamedTempFile,
    before_replace: impl FnOnce() -> Result<()>,
) -> Result<()> {
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
    before_replace()?;
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
        format_job, identify_source_path, init_config, load_config_with_override,
        load_discovered_config, write_source_direct,
    };
    use worsier_formatter::{FormatConfig, resolve_config};

    fn job(path: std::path::PathBuf, config: worsier_formatter::ResolvedConfig) -> Job {
        Job {
            identity: identify_source_path(&path).unwrap(),
            path,
            config: Arc::new(config),
        }
    }

    #[test]
    fn init_writes_a_loadable_complete_config_without_overwriting() {
        let directory = tempfile::tempdir().unwrap();
        init_config(directory.path()).unwrap();
        let path = directory.path().join(CONFIG_FILE);
        assert!(load_config_with_override(&path, false).is_ok());
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "{\n  \"$schema\": \"./node_modules/worsier/configuration_schema.json\",\n  \"lineWidth\": 120,\n  \"verifyAst\": true,\n  \"rules\": {\n    \"importLayout\": true,\n    \"interfaceLayout\": 0,\n    \"objectPropertySpacing\": true,\n    \"statementSpacing\": {\n      \"controlFlowStatements\": \"separate\",\n      \"imports\": \"separate\",\n      \"returnStatements\": \"separate\",\n      \"typeAliases\": \"separate\",\n      \"variableDeclarations\": \"separate\"\n    },\n    \"semicolons\": {\n      \"statements\": \"asNeeded\",\n      \"classMembers\": \"asNeeded\",\n      \"typeMembers\": \"always\"\n    },\n    \"trailingCommas\": \"never\"\n  },\n  \"ignorePatterns\": []\n}\n"
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
        fs::write(&clean, "import { value } from 'pkg'").unwrap();
        let config = resolve_config(FormatConfig::default()).unwrap();
        let clean_job = job(clean, config.clone());
        assert!(matches!(
            format_job(&clean_job, JobMode::Check),
            JobResult::Clean { source: None, .. }
        ));

        let changed = directory.path().join("changed.ts");
        fs::write(&changed, "import{value}from'pkg';").unwrap();
        let changed_job = job(changed, config);
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
    fn direct_write_rejects_a_path_replaced_with_another_file() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.ts");
        let original = directory.path().join("original.ts");
        fs::write(&source, "import{value}from'pkg';").unwrap();
        let identity = identify_source_path(&source).unwrap();
        fs::rename(&source, &original).unwrap();
        fs::write(&source, "doNotChange()").unwrap();

        assert!(write_source_direct(&source, identity, b"changed").is_err());
        assert_eq!(fs::read_to_string(&source).unwrap(), "doNotChange()");
        assert_eq!(
            fs::read_to_string(&original).unwrap(),
            "import{value}from'pkg';"
        );
    }

    #[cfg(unix)]
    #[test]
    fn direct_write_rejects_a_path_replaced_with_a_symbolic_link() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.ts");
        let original = directory.path().join("original.ts");
        let victim = directory.path().join("victim.ts");
        fs::write(&source, "import{value}from'pkg';").unwrap();
        fs::write(&victim, "doNotChange()").unwrap();
        let identity = identify_source_path(&source).unwrap();
        fs::rename(&source, &original).unwrap();
        symlink(&victim, &source).unwrap();

        assert!(write_source_direct(&source, identity, b"changed").is_err());
        assert_eq!(
            fs::read_to_string(&original).unwrap(),
            "import{value}from'pkg';"
        );
        assert_eq!(fs::read_to_string(&victim).unwrap(), "doNotChange()");
    }

    #[test]
    fn config_atomic_write_failure_before_replace_leaves_original_unchanged() {
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
