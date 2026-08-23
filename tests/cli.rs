use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use tempfile::TempDir;

fn run(cwd: &Path, arguments: &[&str], stdin: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_wide-md"));
    command
        .current_dir(cwd)
        .args(arguments)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("wide-md should start");
    if let Some(input) = stdin {
        child
            .stdin
            .take()
            .expect("stdin should be piped")
            .write_all(input.as_bytes())
            .expect("stdin should be writable");
    }
    child.wait_with_output().expect("wide-md should finish")
}

fn write(path: impl AsRef<Path>, contents: &str) {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("parent directory should be created");
    }
    fs::write(path, contents).expect("fixture should be written");
}

fn text(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).expect("process output should be UTF-8")
}

#[test]
fn reads_stdin_by_default_and_with_an_explicit_dash() {
    let directory = TempDir::new().expect("temporary directory should be created");
    let input = "A narrow Markdown\nparagraph.\n\n```text\nkeep\nnewlines\n```\n";
    let expected = "A narrow Markdown paragraph.\n\n```text\nkeep\nnewlines\n```\n";

    let implicit = run(directory.path(), &[], Some(input));
    let explicit = run(directory.path(), &["-"], Some(input));

    assert!(implicit.status.success());
    assert_eq!(text(&implicit.stdout), expected);
    assert!(implicit.stderr.is_empty());
    assert!(explicit.status.success());
    assert_eq!(text(&explicit.stdout), expected);
    assert!(explicit.stderr.is_empty());
}

#[test]
fn preserves_utf8_bom_and_front_matter_while_formatting_prose() {
    let directory = TempDir::new().expect("temporary directory should be created");
    let path = directory.path().join("bom.md");
    let input = "\u{feff}---\ntitle: Test\ndescription: two\n lines\n---\n\nWrapped\nprose.\n";
    let expected = "\u{feff}---\ntitle: Test\ndescription: two\n lines\n---\n\nWrapped prose.\n";
    write(&path, input);

    let output = run(directory.path(), &["bom.md"], None);

    assert!(output.status.success());
    assert_eq!(
        text(&output.stderr),
        "wide-md: 1 changed, 0 unchanged, 0 failed\n"
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), expected);

    let check = run(directory.path(), &["--check", "bom.md"], None);
    assert!(check.status.success());
    assert_eq!(
        text(&check.stderr),
        "wide-md: 0 would change, 1 unchanged, 0 failed\n"
    );
}

#[test]
fn refuses_mdx_files_without_changing_them() {
    let directory = TempDir::new().expect("temporary directory should be created");
    let path = directory.path().join("component.mdx");
    let input = "import Alpha from './alpha'\nexport const value = 1\n";
    write(&path, input);
    let dotfile = directory.path().join(".mdx");
    write(&dotfile, input);

    let output = run(directory.path(), &["component.mdx"], None);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(text(&output.stderr).contains("component.mdx: MDX is not supported safely"));
    assert!(text(&output.stderr).ends_with("wide-md: 0 changed, 0 unchanged, 1 failed\n"));
    assert_eq!(fs::read_to_string(&path).unwrap(), input);

    let stdout = run(
        directory.path(),
        &["--stdout", path.to_str().unwrap()],
        None,
    );
    assert_eq!(stdout.status.code(), Some(2));
    assert!(stdout.stdout.is_empty());
    assert!(text(&stdout.stderr).contains("MDX is not supported safely"));
    assert_eq!(fs::read_to_string(&path).unwrap(), input);

    let discovered = run(directory.path(), &["--include=*.mdx", "."], None);
    assert_eq!(discovered.status.code(), Some(2));
    assert!(discovered.stdout.is_empty());
    assert!(text(&discovered.stderr).contains("component.mdx: MDX is not supported safely"));
    assert!(text(&discovered.stderr).ends_with("wide-md: 0 changed, 0 unchanged, 1 failed\n"));
    assert_eq!(fs::read_to_string(path).unwrap(), input);

    let dotfile_output = run(directory.path(), &[".mdx"], None);
    assert_eq!(dotfile_output.status.code(), Some(2));
    assert!(dotfile_output.stdout.is_empty());
    assert!(text(&dotfile_output.stderr).contains(".mdx: MDX is not supported safely"));
    assert_eq!(fs::read_to_string(dotfile).unwrap(), input);
}

#[test]
fn refuses_custom_containers_without_changing_the_file() {
    let directory = TempDir::new().expect("temporary directory should be created");
    let path = directory.path().join("container.md");
    let input = ":::note\nA wrapped\ncontainer body.\n:::\n";
    write(&path, input);

    let output = run(directory.path(), &["container.md"], None);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(
        text(&output.stderr)
            .contains("container.md: unsupported custom container syntax at line 1")
    );
    assert!(text(&output.stderr).ends_with("wide-md: 0 changed, 0 unchanged, 1 failed\n"));
    assert_eq!(fs::read_to_string(&path).unwrap(), input);

    let stdin = run(directory.path(), &[], Some(input));
    assert_eq!(stdin.status.code(), Some(2));
    assert!(stdin.stdout.is_empty());
    assert_eq!(
        text(&stdin.stderr),
        "wide-md: unsupported custom container syntax at line 1; `:::` containers require a dialect-aware formatter\n"
    );
    assert_eq!(fs::read_to_string(path).unwrap(), input);
}

#[test]
fn formats_mixed_files_and_directories_in_place_with_stable_summaries() {
    let directory = TempDir::new().expect("temporary directory should be created");
    write(directory.path().join("one.md"), "One narrow\nparagraph.\n");
    write(
        directory.path().join("docs/two.markdown"),
        "Two narrow\nparagraphs.\n",
    );
    write(directory.path().join("docs/skip.txt"), "Do not\ntouch.\n");

    let first = run(directory.path(), &["--jobs=2", "one.md", "docs"], None);
    let second = run(directory.path(), &["--jobs=2", "one.md", "docs"], None);

    assert!(first.status.success());
    assert!(first.stdout.is_empty());
    assert_eq!(
        text(&first.stderr),
        "wide-md: 2 changed, 0 unchanged, 0 failed\n"
    );
    assert_eq!(
        fs::read_to_string(directory.path().join("one.md")).unwrap(),
        "One narrow paragraph.\n"
    );
    assert_eq!(
        fs::read_to_string(directory.path().join("docs/two.markdown")).unwrap(),
        "Two narrow paragraphs.\n"
    );
    assert_eq!(
        fs::read_to_string(directory.path().join("docs/skip.txt")).unwrap(),
        "Do not\ntouch.\n"
    );
    assert!(second.status.success());
    assert_eq!(
        text(&second.stderr),
        "wide-md: 0 changed, 2 unchanged, 0 failed\n"
    );
}

#[test]
fn check_uses_exit_one_without_writing() {
    let directory = TempDir::new().expect("temporary directory should be created");
    let path = directory.path().join("check.md");
    write(&path, "Needs to be\nunwrapped.\n");

    let pending = run(directory.path(), &["--check", "check.md"], None);

    assert_eq!(pending.status.code(), Some(1));
    assert!(pending.stdout.is_empty());
    assert_eq!(
        text(&pending.stderr),
        "wide-md: 1 would change, 0 unchanged, 0 failed\n"
    );
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "Needs to be\nunwrapped.\n"
    );

    assert!(run(directory.path(), &["check.md"], None).status.success());
    let clean = run(directory.path(), &["--check", "check.md"], None);
    assert!(clean.status.success());
    assert_eq!(
        text(&clean.stderr),
        "wide-md: 0 would change, 1 unchanged, 0 failed\n"
    );
}

#[test]
fn diff_previews_a_unified_diff_without_writing() {
    let directory = TempDir::new().expect("temporary directory should be created");
    let path = directory.path().join("preview.md");
    write(&path, "Preview this\nchange.\n");

    let output = run(directory.path(), &["--diff", "preview.md"], None);

    assert!(output.status.success());
    let stdout = text(&output.stdout);
    assert!(stdout.contains("--- a/preview.md\n+++ b/preview.md\n"));
    assert!(stdout.contains("-Preview this\n-change.\n+Preview this change.\n"));
    assert_eq!(
        text(&output.stderr),
        "wide-md: 1 would change, 0 unchanged, 0 failed\n"
    );
    assert_eq!(fs::read_to_string(path).unwrap(), "Preview this\nchange.\n");
}

#[test]
fn stdout_previews_one_file_and_width_reflows_it() {
    let directory = TempDir::new().expect("temporary directory should be created");
    let path = directory.path().join("stdout.md");
    let input = "One two three four five six seven eight.\n";
    write(&path, input);

    let output = run(
        directory.path(),
        &["--width=20", "--stdout", "stdout.md"],
        None,
    );

    assert!(output.status.success());
    assert_eq!(
        text(&output.stdout),
        "One two three four\nfive six seven\neight.\n"
    );
    assert!(output.stderr.is_empty());
    assert_eq!(fs::read_to_string(path).unwrap(), input);
}

#[test]
fn include_adds_extensions_and_no_ignore_overrides_repository_ignores() {
    let directory = TempDir::new().expect("temporary directory should be created");
    fs::create_dir(directory.path().join(".git")).unwrap();
    write(directory.path().join(".gitignore"), "ignored.md\n");
    write(
        directory.path().join(".git/internal.md"),
        "Repository\nmetadata.\n",
    );
    write(directory.path().join("visible.md"), "Visible\ntext.\n");
    write(directory.path().join("ignored.md"), "Ignored\ntext.\n");
    write(directory.path().join("note.mkd"), "Included\ntext.\n");
    write(directory.path().join(".hidden.md"), "Hidden\ntext.\n");

    let normal = run(directory.path(), &["--include=*.mkd", "."], None);

    assert!(normal.status.success());
    assert_eq!(
        text(&normal.stderr),
        "wide-md: 2 changed, 0 unchanged, 0 failed\n"
    );
    assert_eq!(
        fs::read_to_string(directory.path().join("visible.md")).unwrap(),
        "Visible text.\n"
    );
    assert_eq!(
        fs::read_to_string(directory.path().join("note.mkd")).unwrap(),
        "Included text.\n"
    );
    assert_eq!(
        fs::read_to_string(directory.path().join("ignored.md")).unwrap(),
        "Ignored\ntext.\n"
    );
    assert_eq!(
        fs::read_to_string(directory.path().join(".hidden.md")).unwrap(),
        "Hidden\ntext.\n"
    );

    let unrestricted = run(directory.path(), &["--no-ignore", "."], None);
    assert!(unrestricted.status.success());
    assert_eq!(
        text(&unrestricted.stderr),
        "wide-md: 2 changed, 1 unchanged, 0 failed\n"
    );
    assert_eq!(
        fs::read_to_string(directory.path().join("ignored.md")).unwrap(),
        "Ignored text.\n"
    );
    assert_eq!(
        fs::read_to_string(directory.path().join(".hidden.md")).unwrap(),
        "Hidden text.\n"
    );
    assert_eq!(
        fs::read_to_string(directory.path().join(".git/internal.md")).unwrap(),
        "Repository\nmetadata.\n"
    );
}

#[test]
fn repository_config_supplies_width_includes_jobs_and_no_ignore() {
    let directory = TempDir::new().expect("temporary directory should be created");
    fs::create_dir(directory.path().join(".git")).unwrap();
    write(directory.path().join(".gitignore"), "ignored.md\n");
    write(
        directory.path().join(".wide-md.toml"),
        "width = 20\ninclude = [\"*.mkd\"]\njobs = 2\nno-ignore = true\n",
    );
    write(
        directory.path().join("configured.md"),
        "One two three four five six seven eight.\n",
    );
    write(directory.path().join("extra.mkd"), "Extra narrow\ntext.\n");
    write(
        directory.path().join("ignored.md"),
        "Ignored narrow\ntext.\n",
    );

    let output = run(directory.path(), &["."], None);

    assert!(output.status.success());
    assert_eq!(
        text(&output.stderr),
        "wide-md: 3 changed, 0 unchanged, 0 failed\n"
    );
    assert_eq!(
        fs::read_to_string(directory.path().join("configured.md")).unwrap(),
        "One two three four\nfive six seven\neight.\n"
    );
    assert_eq!(
        fs::read_to_string(directory.path().join("extra.mkd")).unwrap(),
        "Extra narrow text.\n"
    );
    assert_eq!(
        fs::read_to_string(directory.path().join("ignored.md")).unwrap(),
        "Ignored narrow text.\n"
    );
}

#[test]
fn command_line_width_overrides_repository_config() {
    let directory = TempDir::new().expect("temporary directory should be created");
    write(directory.path().join(".wide-md.toml"), "width = 10\n");
    write(
        directory.path().join("override.md"),
        "One two three four five six.\n",
    );

    let output = run(
        directory.path(),
        &["--width=20", "--jobs=1", "override.md"],
        None,
    );

    assert!(output.status.success());
    assert_eq!(
        fs::read_to_string(directory.path().join("override.md")).unwrap(),
        "One two three four\nfive six.\n"
    );
}

#[test]
fn configuration_is_discovered_in_a_parent_directory() {
    let directory = TempDir::new().expect("temporary directory should be created");
    write(directory.path().join(".wide-md.toml"), "width = 12\n");
    write(
        directory.path().join("nested/parent.md"),
        "One two three four five.\n",
    );

    let output = run(&directory.path().join("nested"), &["."], None);

    assert!(output.status.success());
    assert_eq!(
        fs::read_to_string(directory.path().join("nested/parent.md")).unwrap(),
        "One two\nthree four\nfive.\n"
    );
}

#[cfg(unix)]
#[test]
fn atomic_replacement_preserves_unix_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let directory = TempDir::new().expect("temporary directory should be created");
    let path = directory.path().join("mode.md");
    write(&path, "Preserve the\nmode.\n");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();

    let output = run(directory.path(), &["mode.md"], None);

    assert!(output.status.success());
    assert_eq!(
        fs::metadata(path).unwrap().permissions().mode() & 0o777,
        0o640
    );
}

#[cfg(unix)]
#[test]
fn explicit_symlinks_are_refused_and_discovered_symlinks_are_skipped() {
    use std::os::unix::fs::symlink;

    let directory = TempDir::new().expect("temporary directory should be created");
    write(directory.path().join("outside.md"), "Outside\ntext.\n");
    fs::create_dir(directory.path().join("tree")).unwrap();
    symlink("../outside.md", directory.path().join("tree/link.md")).unwrap();

    let explicit = run(directory.path(), &["tree/link.md"], None);
    let discovered = run(directory.path(), &["tree"], None);

    assert_eq!(explicit.status.code(), Some(2));
    assert!(text(&explicit.stderr).contains("refusing to follow symlink"));
    assert!(discovered.status.success());
    assert_eq!(
        text(&discovered.stderr),
        "wide-md: 0 changed, 0 unchanged, 0 failed\n"
    );
    assert_eq!(
        fs::read_to_string(directory.path().join("outside.md")).unwrap(),
        "Outside\ntext.\n"
    );
}

#[test]
fn processing_errors_exit_two_after_reporting_a_summary() {
    let directory = TempDir::new().expect("temporary directory should be created");

    let output = run(directory.path(), &["missing.md"], None);

    assert_eq!(output.status.code(), Some(2));
    assert!(text(&output.stderr).contains("missing.md: cannot inspect path:"));
    assert!(text(&output.stderr).ends_with("wide-md: 0 changed, 0 unchanged, 1 failed\n"));
}

#[test]
fn invalid_configuration_exits_two_before_touching_files() {
    let directory = TempDir::new().expect("temporary directory should be created");
    write(
        directory.path().join(".wide-md.toml"),
        "unknown-setting = true\n",
    );
    write(directory.path().join("safe.md"), "Leave this\nalone.\n");

    let output = run(directory.path(), &["safe.md"], None);

    assert_eq!(output.status.code(), Some(2));
    assert!(text(&output.stderr).contains("unknown field `unknown-setting`"));
    assert_eq!(
        fs::read_to_string(directory.path().join("safe.md")).unwrap(),
        "Leave this\nalone.\n"
    );
}

#[test]
fn ambiguous_stdin_and_stdout_combinations_exit_two() {
    let directory = TempDir::new().expect("temporary directory should be created");
    write(directory.path().join("file.md"), "A\nfile.\n");

    let mixed_stdin = run(directory.path(), &["-", "file.md"], Some("stdin\n"));
    let mixed_stdout = run(directory.path(), &["--stdout", "file.md", "file.md"], None);

    assert_eq!(mixed_stdin.status.code(), Some(2));
    assert!(
        text(&mixed_stdin.stderr).contains("stdin path - cannot be combined with filesystem paths")
    );
    assert_eq!(mixed_stdout.status.code(), Some(2));
    assert!(
        text(&mixed_stdout.stderr).contains(
            "--stdout accepts its file directly and cannot be combined with PATH arguments"
        )
    );
}
