use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn reads_markdown_from_stdin_and_writes_to_stdout() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wide-md"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("wide-md should start");

    child
        .stdin
        .take()
        .expect("stdin should be piped")
        .write_all(b"A narrow Markdown\nparagraph.\n\n```text\nkeep\nnewlines\n```\n")
        .expect("input should be writable");

    let output = child.wait_with_output().expect("wide-md should finish");

    assert!(output.status.success());
    assert_eq!(
        output.stdout,
        b"A narrow Markdown paragraph.\n\n```text\nkeep\nnewlines\n```\n"
    );
    assert!(output.stderr.is_empty());
}
