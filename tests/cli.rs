//! End-to-end tests for the `termqr` binary, compared against the library's output.
#![cfg(feature = "cli")]

use std::io::Write;
use std::process::{Command, Output, Stdio};

use termqr::{ErrorCorrection, Polarity};

fn run(args: &[&str], stdin: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_termqr"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("cargo builds the termqr binary for integration tests");
    child
        .stdin
        .take()
        .expect("stdin was configured as piped")
        .write_all(stdin)
        .expect("child reads all of stdin");
    child
        .wait_with_output()
        .expect("child output can be collected")
}

fn expected(data: &str, polarity: Polarity, ec: ErrorCorrection) -> String {
    let text = termqr::render(data, polarity, ec).expect("test inputs fit in a qr code");
    format!("{text}\n")
}

#[test]
fn stdin_strips_trailing_newline() {
    let out = run(&[], b"hello\n");
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        expected("hello", Polarity::LightOnDark, ErrorCorrection::Medium)
    );
}

#[test]
fn keep_newline_encodes_it() {
    let out = run(&["--keep-newline", "-"], b"hello\n");
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        expected("hello\n", Polarity::LightOnDark, ErrorCorrection::Medium)
    );
}

#[test]
fn reads_file_with_polarity() {
    let path = std::env::temp_dir().join(format!("termqr-cli-{}.txt", std::process::id()));
    std::fs::write(&path, "from file\n").unwrap();
    let out = run(&["--polarity", "1", path.to_str().unwrap()], b"");
    let _ = std::fs::remove_file(&path); // best-effort cleanup of a temp file
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        expected("from file", Polarity::DarkOnLight, ErrorCorrection::Medium)
    );
}

#[test]
fn missing_file_fails_with_path_in_message() {
    let out = run(&["/nonexistent/termqr-input"], b"");
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("failed to read /nonexistent/termqr-input"));
}

#[test]
fn piped_output_skips_width_check() {
    // Version 40 is 185 columns, wider than any default terminal; stdout is a pipe here,
    // so no width check applies.
    let data = vec![b'a'; 2000];
    let out = run(&[], &data);
    assert!(out.status.success());
}

#[test]
fn ec_level_is_case_insensitive() {
    for arg in ["H", "h"] {
        let out = run(&["--ec-level", arg], b"hello");
        assert!(out.status.success());
        assert_eq!(
            String::from_utf8(out.stdout).unwrap(),
            expected("hello", Polarity::LightOnDark, ErrorCorrection::High)
        );
    }
}

#[test]
fn input_over_level_capacity_fails() {
    // 1274 bytes fits at M but exceeds version 40 at H.
    let data = vec![b'a'; 1274];
    assert!(run(&[], &data).status.success());
    let out = run(&["--ec-level", "H"], &data);
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("failed to render input"));
}
