use std::collections::BTreeSet;
use std::env;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::num::NonZeroUsize;
use std::path::{Component, Path, PathBuf};

use clap::{Parser, ValueHint};
use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;
use rayon::ThreadPoolBuilder;
use rayon::prelude::*;
use serde::Deserialize;
use similar::TextDiff;
use tempfile::NamedTempFile;

use crate::{FormatOptions, format_markdown};

/// Remove artificial hard wrapping from Markdown.
#[derive(Debug, Parser)]
#[command(
    name = "wide-md",
    version,
    about,
    long_about = "Remove artificial hard wrapping from Markdown. With no paths, read stdin and write stdout. With file or directory paths, format matching files in place."
)]
pub struct Cli {
    /// Reflow prose to at most this many Unicode display columns
    #[arg(long, value_name = "COLUMNS", value_parser = parse_positive)]
    width: Option<NonZeroUsize>,

    /// Do not write files; exit 1 when any file would change
    #[arg(long, conflicts_with_all = ["diff", "stdout"])]
    check: bool,

    /// Do not write files; print unified diffs
    #[arg(long, conflicts_with_all = ["check", "stdout"])]
    diff: bool,

    /// Format one file to stdout without modifying it
    #[arg(
        long,
        value_name = "FILE",
        value_hint = ValueHint::FilePath,
        conflicts_with_all = ["check", "diff"]
    )]
    stdout: Option<PathBuf>,

    /// Traverse hidden and ignored files, except .git directories
    #[arg(long)]
    no_ignore: bool,

    /// Include an additional Markdown filename glob; discovery only
    #[arg(long, value_name = "GLOB", action = clap::ArgAction::Append)]
    include: Vec<String>,

    /// Use this many worker threads for filesystem paths
    #[arg(long, value_name = "N", value_parser = parse_positive)]
    jobs: Option<NonZeroUsize>,

    /// Markdown files and directories; use - for explicit stdin; MDX is unsupported
    #[arg(value_name = "PATH", value_hint = ValueHint::AnyPath)]
    paths: Vec<PathBuf>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
struct FileConfig {
    width: Option<usize>,
    include: Vec<String>,
    jobs: Option<usize>,
    #[serde(alias = "no_ignore")]
    no_ignore: bool,
}

#[derive(Debug)]
struct Settings {
    width: Option<NonZeroUsize>,
    includes: Vec<String>,
    jobs: Option<NonZeroUsize>,
    no_ignore: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PathMode {
    Write,
    Check,
    Diff,
}

#[derive(Debug)]
enum Failure {
    BrokenPipe,
    Message(String),
}

impl Failure {
    fn message(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

#[derive(Debug)]
struct ReportedFailure {
    path: String,
    message: String,
}

#[derive(Debug)]
struct Discovery {
    files: Vec<PathBuf>,
    failures: Vec<ReportedFailure>,
}

#[derive(Debug)]
struct FileOutcome {
    display_path: String,
    state: FileState,
}

#[derive(Debug)]
enum FileState {
    Changed { diff: Option<String> },
    Unchanged,
    Failed(String),
}

/// Parses process arguments, executes the selected mode, and returns the
/// documented process exit code.
pub fn main_entry() -> i32 {
    let cli = Cli::parse();
    let current_dir = match env::current_dir() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("wide-md: cannot determine the current directory: {error}");
            return 2;
        }
    };

    match execute(cli, &current_dir) {
        Ok(code) => code,
        Err(Failure::BrokenPipe) => 0,
        Err(Failure::Message(message)) => {
            eprintln!("wide-md: {message}");
            2
        }
    }
}

fn execute(cli: Cli, current_dir: &Path) -> Result<i32, Failure> {
    let settings = load_settings(&cli, current_dir)?;

    if let Some(path) = &cli.stdout {
        if !cli.paths.is_empty() {
            return Err(Failure::message(
                "--stdout accepts its file directly and cannot be combined with PATH arguments",
            ));
        }
        if is_stdin_path(path) {
            return run_stdin(settings.width);
        }
        return run_stdout_file(path, settings.width);
    }

    if cli.paths.is_empty() {
        if cli.check || cli.diff {
            return Err(Failure::message(
                "--check and --diff require at least one filesystem path",
            ));
        }
        return run_stdin(settings.width);
    }

    let stdin_paths = cli.paths.iter().filter(|path| is_stdin_path(path)).count();
    if stdin_paths > 0 {
        if cli.paths.len() != 1 {
            return Err(Failure::message(
                "stdin path - cannot be combined with filesystem paths",
            ));
        }
        if cli.check || cli.diff {
            return Err(Failure::message(
                "--check and --diff require filesystem paths, not stdin",
            ));
        }
        return run_stdin(settings.width);
    }

    let mode = if cli.check {
        PathMode::Check
    } else if cli.diff {
        PathMode::Diff
    } else {
        PathMode::Write
    };
    run_paths(&cli.paths, current_dir, &settings, mode)
}

fn parse_positive(value: &str) -> Result<NonZeroUsize, String> {
    value
        .parse::<usize>()
        .map_err(|_| format!("{value:?} is not a positive integer"))
        .and_then(|number| {
            NonZeroUsize::new(number).ok_or_else(|| "value must be greater than zero".to_owned())
        })
}

fn load_settings(cli: &Cli, current_dir: &Path) -> Result<Settings, Failure> {
    let (config, config_path) = load_config(current_dir)?;
    let source = config_path
        .as_deref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| ".wide-md.toml".to_owned());
    let config_width = positive_config_value("width", config.width, &source)?;
    let config_jobs = positive_config_value("jobs", config.jobs, &source)?;

    let mut includes = config.include;
    includes.extend(cli.include.iter().cloned());
    includes.sort();
    includes.dedup();

    Ok(Settings {
        width: cli.width.or(config_width),
        includes,
        jobs: cli.jobs.or(config_jobs),
        no_ignore: cli.no_ignore || config.no_ignore,
    })
}

fn load_config(current_dir: &Path) -> Result<(FileConfig, Option<PathBuf>), Failure> {
    let mut directory = Some(current_dir);
    while let Some(candidate_dir) = directory {
        let candidate = candidate_dir.join(".wide-md.toml");
        match fs::read_to_string(&candidate) {
            Ok(contents) => {
                let config = toml::from_str(&contents).map_err(|error| {
                    Failure::message(format!("cannot parse {}: {error}", candidate.display()))
                })?;
                return Ok((config, Some(candidate)));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(Failure::message(format!(
                    "cannot read {}: {error}",
                    candidate.display()
                )));
            }
        }
        directory = candidate_dir.parent();
    }
    Ok((FileConfig::default(), None))
}

fn positive_config_value(
    name: &str,
    value: Option<usize>,
    source: &str,
) -> Result<Option<NonZeroUsize>, Failure> {
    value
        .map(|number| {
            NonZeroUsize::new(number).ok_or_else(|| {
                Failure::message(format!("{name} in {source} must be greater than zero"))
            })
        })
        .transpose()
}

fn is_stdin_path(path: &Path) -> bool {
    path.as_os_str() == OsStr::new("-")
}

fn run_stdin(width: Option<NonZeroUsize>) -> Result<i32, Failure> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input).map_err(|error| {
        Failure::message(format!("cannot read stdin as UTF-8 Markdown: {error}"))
    })?;
    let formatted = format_markdown(&input, FormatOptions { width })
        .map_err(|error| Failure::message(error.to_string()))?;
    write_stdout(formatted.as_bytes())?;
    Ok(0)
}

fn run_stdout_file(path: &Path, width: Option<NonZeroUsize>) -> Result<i32, Failure> {
    ensure_regular_file(path)?;
    if is_mdx(path) {
        return Err(Failure::message(format!(
            "{}: {UNSUPPORTED_MDX}",
            path.display()
        )));
    }
    let input = fs::read_to_string(path).map_err(|error| {
        Failure::message(format!(
            "cannot read {} as UTF-8 Markdown: {error}",
            path.display()
        ))
    })?;
    let formatted = format_markdown(&input, FormatOptions { width })
        .map_err(|error| Failure::message(format!("{}: {error}", path.display())))?;
    write_stdout(formatted.as_bytes())?;
    Ok(0)
}

fn ensure_regular_file(path: &Path) -> Result<(), Failure> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| Failure::message(format!("cannot inspect {}: {error}", path.display())))?;
    if metadata.file_type().is_symlink() {
        return Err(Failure::message(format!(
            "refusing to follow symlink {}",
            path.display()
        )));
    }
    if !metadata.is_file() {
        return Err(Failure::message(format!(
            "{} is not a regular file",
            path.display()
        )));
    }
    Ok(())
}

fn run_paths(
    roots: &[PathBuf],
    current_dir: &Path,
    settings: &Settings,
    mode: PathMode,
) -> Result<i32, Failure> {
    let matcher = IncludeMatcher::new(&settings.includes)?;
    let discovery = discover_files(roots, current_dir, settings.no_ignore, &matcher);
    let outcomes = process_files(
        &discovery.files,
        current_dir,
        settings.width,
        settings.jobs,
        mode,
    )?;

    let mut changed = 0;
    let mut unchanged = 0;
    let mut failed = discovery.failures.len();
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();

    for failure in discovery.failures {
        write_stderr(
            &mut stderr,
            format!("wide-md: {}: {}\n", failure.path, failure.message).as_bytes(),
        )?;
    }

    for outcome in outcomes {
        match outcome.state {
            FileState::Changed { diff } => {
                changed += 1;
                if let Some(diff) = diff {
                    write_output(&mut stdout, diff.as_bytes(), "stdout")?;
                    if !diff.ends_with('\n') {
                        write_output(&mut stdout, b"\n", "stdout")?;
                    }
                }
            }
            FileState::Unchanged => unchanged += 1,
            FileState::Failed(message) => {
                failed += 1;
                write_stderr(
                    &mut stderr,
                    format!("wide-md: {}: {message}\n", outcome.display_path).as_bytes(),
                )?;
            }
        }
    }

    let changed_label = if mode == PathMode::Write {
        "changed"
    } else {
        "would change"
    };
    write_stderr(
        &mut stderr,
        format!("wide-md: {changed} {changed_label}, {unchanged} unchanged, {failed} failed\n")
            .as_bytes(),
    )?;

    if failed > 0 {
        Ok(2)
    } else if mode == PathMode::Check && changed > 0 {
        Ok(1)
    } else {
        Ok(0)
    }
}

struct IncludeMatcher {
    set: GlobSet,
}

impl IncludeMatcher {
    fn new(patterns: &[String]) -> Result<Self, Failure> {
        let mut builder = GlobSetBuilder::new();
        for pattern in patterns {
            builder.add(Glob::new(pattern).map_err(|error| {
                Failure::message(format!("invalid --include glob {pattern:?}: {error}"))
            })?);
        }
        let set = builder
            .build()
            .map_err(|error| Failure::message(format!("cannot build include globs: {error}")))?;
        Ok(Self { set })
    }

    fn matches(&self, path: &Path, root: &Path) -> bool {
        let relative = path.strip_prefix(root).unwrap_or(path);
        self.set.is_match(relative)
            || path
                .file_name()
                .is_some_and(|file_name| self.set.is_match(Path::new(file_name)))
    }
}

fn discover_files(
    roots: &[PathBuf],
    current_dir: &Path,
    no_ignore: bool,
    matcher: &IncludeMatcher,
) -> Discovery {
    let mut files = BTreeSet::new();
    let mut failures = Vec::new();

    for root in roots {
        let root = normalize_absolute(root, current_dir);
        let display_root = display_path(&root, current_dir);
        let metadata = match fs::symlink_metadata(&root) {
            Ok(metadata) => metadata,
            Err(error) => {
                failures.push(ReportedFailure {
                    path: display_root,
                    message: format!("cannot inspect path: {error}"),
                });
                continue;
            }
        };

        if metadata.file_type().is_symlink() {
            failures.push(ReportedFailure {
                path: display_root,
                message: "refusing to follow symlink".to_owned(),
            });
        } else if metadata.is_file() {
            files.insert(root);
        } else if metadata.is_dir() {
            discover_directory(
                &root,
                current_dir,
                no_ignore,
                matcher,
                &mut files,
                &mut failures,
            );
        } else {
            failures.push(ReportedFailure {
                path: display_root,
                message: "not a regular file or directory".to_owned(),
            });
        }
    }

    Discovery {
        files: files.into_iter().collect(),
        failures,
    }
}

fn discover_directory(
    root: &Path,
    current_dir: &Path,
    no_ignore: bool,
    matcher: &IncludeMatcher,
    files: &mut BTreeSet<PathBuf>,
    failures: &mut Vec<ReportedFailure>,
) {
    let mut builder = WalkBuilder::new(root);
    builder.follow_links(false);
    builder.hidden(!no_ignore);
    builder.parents(!no_ignore);
    builder.ignore(!no_ignore);
    builder.git_global(!no_ignore);
    builder.git_ignore(!no_ignore);
    builder.git_exclude(!no_ignore);
    builder.filter_entry(|entry| entry.file_name() != OsStr::new(".git"));

    for entry in builder.build() {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                failures.push(ReportedFailure {
                    path: display_path(root, current_dir),
                    message: format!("directory traversal failed: {error}"),
                });
                continue;
            }
        };
        if !entry
            .file_type()
            .is_some_and(|file_type| file_type.is_file())
        {
            continue;
        }
        let path = entry.into_path();
        if is_default_markdown(&path) || matcher.matches(&path, root) {
            files.insert(normalize_absolute(&path, current_dir));
        }
    }
}

fn is_default_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("markdown")
        })
}

const UNSUPPORTED_MDX: &str = "MDX is not supported safely; use a dialect-aware formatter";

fn is_mdx(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| extension.eq_ignore_ascii_case("mdx"))
}

fn normalize_absolute(path: &Path, current_dir: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        current_dir.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn display_path(path: &Path, current_dir: &Path) -> String {
    path.strip_prefix(current_dir)
        .ok()
        .filter(|relative| !relative.as_os_str().is_empty())
        .unwrap_or(path)
        .display()
        .to_string()
}

fn process_files(
    files: &[PathBuf],
    current_dir: &Path,
    width: Option<NonZeroUsize>,
    jobs: Option<NonZeroUsize>,
    mode: PathMode,
) -> Result<Vec<FileOutcome>, Failure> {
    let process = || {
        files
            .par_iter()
            .map(|path| process_file(path, current_dir, width, mode))
            .collect()
    };

    if let Some(jobs) = jobs {
        ThreadPoolBuilder::new()
            .num_threads(jobs.get())
            .build()
            .map_err(|error| Failure::message(format!("cannot build worker pool: {error}")))
            .map(|pool| pool.install(process))
    } else {
        Ok(process())
    }
}

fn process_file(
    path: &Path,
    current_dir: &Path,
    width: Option<NonZeroUsize>,
    mode: PathMode,
) -> FileOutcome {
    let display_path = display_path(path, current_dir);
    let state = (|| {
        let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() {
            return Err("refusing to follow symlink".to_owned());
        }
        if !metadata.is_file() {
            return Err("not a regular file".to_owned());
        }
        if is_mdx(path) {
            return Err(UNSUPPORTED_MDX.to_owned());
        }

        let input = fs::read_to_string(path)
            .map_err(|error| format!("cannot read as UTF-8 Markdown: {error}"))?;
        let formatted =
            format_markdown(&input, FormatOptions { width }).map_err(|error| error.to_string())?;
        if formatted == input {
            return Ok(FileState::Unchanged);
        }

        match mode {
            PathMode::Write => {
                write_atomically(path, formatted.as_bytes(), metadata.permissions())?;
                Ok(FileState::Changed { diff: None })
            }
            PathMode::Check => Ok(FileState::Changed { diff: None }),
            PathMode::Diff => Ok(FileState::Changed {
                diff: Some(unified_diff(&display_path, &input, &formatted)),
            }),
        }
    })()
    .unwrap_or_else(FileState::Failed);

    FileOutcome {
        display_path,
        state,
    }
}

fn write_atomically(
    path: &Path,
    contents: &[u8],
    permissions: fs::Permissions,
) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "file has no parent directory".to_owned())?;
    let mut temporary = NamedTempFile::new_in(parent)
        .map_err(|error| format!("cannot create temporary file: {error}"))?;
    temporary
        .write_all(contents)
        .map_err(|error| format!("cannot write temporary file: {error}"))?;
    temporary
        .flush()
        .map_err(|error| format!("cannot flush temporary file: {error}"))?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| format!("cannot sync temporary file: {error}"))?;
    temporary
        .as_file()
        .set_permissions(permissions)
        .map_err(|error| format!("cannot preserve permissions: {error}"))?;
    temporary
        .persist(path)
        .map_err(|error| format!("cannot replace file atomically: {}", error.error))?;
    sync_parent(parent)?;
    Ok(())
}

fn sync_parent(parent: &Path) -> Result<(), String> {
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("cannot sync parent directory: {error}"))
}

fn unified_diff(path: &str, before: &str, after: &str) -> String {
    let before_label = format!("a/{path}");
    let after_label = format!("b/{path}");
    TextDiff::from_lines(before, after)
        .unified_diff()
        .header(&before_label, &after_label)
        .to_string()
}

fn write_stdout(contents: &[u8]) -> Result<(), Failure> {
    write_output(&mut io::stdout().lock(), contents, "stdout")
}

fn write_output(
    writer: &mut impl Write,
    contents: &[u8],
    destination: &str,
) -> Result<(), Failure> {
    writer.write_all(contents).map_err(|error| {
        if error.kind() == io::ErrorKind::BrokenPipe {
            Failure::BrokenPipe
        } else {
            Failure::message(format!("cannot write {destination}: {error}"))
        }
    })
}

fn write_stderr(writer: &mut impl Write, contents: &[u8]) -> Result<(), Failure> {
    writer
        .write_all(contents)
        .map_err(|error| Failure::message(format!("cannot write stderr: {error}")))
}

#[cfg(test)]
mod tests {
    use super::{display_path, normalize_absolute, parse_positive};
    use std::path::Path;

    #[test]
    fn positive_values_reject_zero_and_non_numbers() {
        assert_eq!(parse_positive("3").expect("three is positive").get(), 3);
        assert!(parse_positive("0").is_err());
        assert!(parse_positive("nope").is_err());
    }

    #[test]
    fn paths_are_normalized_and_displayed_relative_to_the_working_directory() {
        let current = Path::new("/tmp/project");
        let path = normalize_absolute(Path::new("docs/../README.md"), current);

        assert_eq!(path, Path::new("/tmp/project/README.md"));
        assert_eq!(display_path(&path, current), "README.md");
    }
}
