use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::{collections::HashMap, mem};

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
    about = "Format JavaScript and TypeScript imports with Worsier"
)]
struct Args {
    /// Create a complete worsier.jsonc in the current directory.
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
    if candidates.is_empty() {
        if explicit_single_file {
            bail!(
                "{} is not a supported JavaScript or TypeScript file",
                args.paths[0].display()
            );
        }
        return Ok(0);
    }

    let explicit_config = args
        .config
        .as_deref()
        .map(|path| load_config(path, args.no_verify))
        .transpose()?;
    let mut config_cache = ConfigCache::default();
    let mut jobs = Vec::new();
    for candidate in candidates {
        let loaded = match &explicit_config {
            Some(config) => Arc::clone(config),
            None => config_cache.discover(&candidate.path, args.no_verify)?,
        };
        if candidate.discovered && loaded.is_ignored(&candidate.path) {
            continue;
        }
        jobs.push(Job {
            path: candidate.path,
            config: Arc::clone(&loaded.config),
        });
    }

    let run = || jobs.par_iter().map(format_job).collect::<Vec<_>>();
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
                if !args.check && !args.write {
                    print!("{source}");
                }
            }
            JobResult::Changed { path, output } => {
                changed = true;
                if args.write {
                    atomic_write(&path, output.as_bytes())?;
                    eprintln!("Formatted {}", path.display());
                } else if args.check {
                    eprintln!("Would format {}", path.display());
                } else {
                    print!("{output}");
                }
            }
            JobResult::Failed { path, error } => {
                failed = true;
                eprintln!("{}: {error:#}", path.display());
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
    if path.exists() {
        bail!("{} already exists", path.display());
    }
    let config = FormatConfig {
        schema: Some("./node_modules/worsier/configuration_schema.json".to_owned()),
        ..FormatConfig::default()
    };
    let json = serde_json::to_string_pretty(&config)?;
    fs::write(&path, format!("{json}\n"))?;
    println!("Created {}", path.display());
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
        if path.is_file() {
            if is_supported(path) {
                files.push(Candidate {
                    path: path.clone(),
                    discovered: false,
                });
            } else {
                bail!(
                    "{} is not a supported JavaScript or TypeScript file",
                    path.display()
                );
            }
            continue;
        }
        if !path.is_dir() {
            bail!("{} does not exist", path.display());
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
            let entry = entry.with_context(|| format!("failed to walk {}", path.display()))?;
            if entry.file_type().is_some_and(|kind| kind.is_file()) && is_supported(entry.path()) {
                files.push(Candidate {
                    path: entry.into_path(),
                    discovered: true,
                });
            }
        }
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    files.dedup_by(|right, left| {
        if left.path == right.path {
            left.discovered &= mem::take(&mut right.discovered);
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
            .with_context(|| format!("failed to resolve {}", start.display()))?;
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
                bail!(
                    "no {CONFIG_FILE} found for {}; run `npx worsier --init`",
                    file.display()
                );
            }
        };

        for directory in visited {
            self.by_directory.insert(directory, Arc::clone(&loaded));
        }
        Ok(loaded)
    }
}

struct LoadedConfig {
    config: Arc<ResolvedConfig>,
    ignore: Gitignore,
}

impl LoadedConfig {
    fn is_ignored(&self, path: &Path) -> bool {
        self.ignore
            .matched_path_or_any_parents(path, false)
            .is_ignore()
    }
}

fn load_config(path: &Path, no_verify: bool) -> Result<Arc<LoadedConfig>> {
    let absolute_path = path
        .canonicalize()
        .with_context(|| format!("failed to resolve configuration {}", path.display()))?;
    let source = fs::read_to_string(&absolute_path)
        .with_context(|| format!("failed to read configuration {}", absolute_path.display()))?;
    let value: serde_json::Value =
        jsonc_parser::parse_to_serde_value(&source, &jsonc_parser::ParseOptions::default())
            .with_context(|| format!("invalid JSONC configuration {}", absolute_path.display()))?;
    let mut config: FormatConfig = serde_path_to_error::deserialize(value)
        .with_context(|| format!("invalid configuration value in {}", absolute_path.display()))?;
    if no_verify {
        config.verify_ast = false;
    }
    let resolved = resolve_config(config)
        .with_context(|| format!("invalid configuration {}", absolute_path.display()))?;
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

enum JobResult {
    Clean { path: PathBuf, source: String },
    Changed { path: PathBuf, output: String },
    Failed { path: PathBuf, error: anyhow::Error },
}

fn format_job(job: &Job) -> JobResult {
    let source = match fs::read_to_string(&job.path)
        .with_context(|| format!("failed to read {}", job.path.display()))
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
        Ok(Some(output)) => JobResult::Changed {
            path: job.path.clone(),
            output,
        },
        Ok(None) => JobResult::Clean {
            path: job.path.clone(),
            source,
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
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let permissions = fs::metadata(path)?.permissions();
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.as_file_mut().set_permissions(permissions)?;
    temporary.write_all(output)?;
    temporary.as_file_mut().sync_all()?;
    temporary
        .persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("failed to replace {}", path.display()))?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{CONFIG_FILE, init_config, load_config_with_override};

    #[test]
    fn init_writes_a_loadable_complete_config_without_overwriting() {
        let directory = tempfile::tempdir().unwrap();
        init_config(directory.path()).unwrap();
        let path = directory.path().join(CONFIG_FILE);
        assert!(load_config_with_override(&path, false).is_ok());
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "{\n  \"$schema\": \"./node_modules/worsier/configuration_schema.json\",\n  \"lineWidth\": 120,\n  \"verifyAst\": true,\n  \"rules\": {\n    \"imports\": true\n  },\n  \"ignorePatterns\": []\n}\n"
        );
        assert!(init_config(directory.path()).is_err());
    }
}
