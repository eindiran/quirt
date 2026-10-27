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
    assert!(stderr.contains(
        "input is 1274 bytes, too long for error-correction level H \
         (at most 1273 bytes of arbitrary data); it fits at --ec-level Q"
    ));
}

#[test]
fn too_long_without_lower_level_says_shorten() {
    let out = run(&["--ec-level", "L"], &[0u8; 2954]);
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("level L (at most 2953 bytes of arbitrary data); shorten the input"));
}

#[test]
fn empty_input_is_an_error() {
    for stdin in [&b""[..], b"\n", b"\r\n"] {
        let out = run(&[], stdin);
        assert!(!out.status.success());
        assert!(out.stdout.is_empty());
        let stderr = String::from_utf8(out.stderr).unwrap();
        assert!(stderr.contains("input is empty; nothing to encode"));
    }
}

#[test]
fn longest_input_with_newline_still_fits() {
    // The read limit leaves room for a trimmed "\r\n" after MAX_DATA_LEN digits.
    let mut data = vec![b'7'; termqr::MAX_DATA_LEN];
    data.extend_from_slice(b"\r\n");
    let out = run(&["--ec-level", "L"], &data);
    assert!(out.status.success());
}

#[test]
fn oversized_input_is_rejected_without_reading_it_all() {
    let mut data = vec![b'7'; termqr::MAX_DATA_LEN + 1];
    data.extend_from_slice(b"\n");
    let out = run(&["--ec-level", "L"], &data);
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("input is longer than 7089 bytes, more than any qr code holds"));
}

#[cfg(unix)]
#[test]
fn endless_stdin_fails_fast() {
    // Unbounded reading would never finish on /dev/zero.
    let zero = std::fs::File::open("/dev/zero").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_termqr"))
        .stdin(zero)
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("more than any qr code holds"));
}

/// Width-check tests against a real pseudo-terminal sized 80x24.
#[cfg(unix)]
mod pty {
    use std::ffi::OsStr;
    use std::fs::{File, OpenOptions};
    use std::io::{self, Write};
    use std::os::fd::OwnedFd;
    use std::os::unix::ffi::OsStrExt;
    use std::process::{Command, Output, Stdio};

    use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
    use rustix::termios::{Winsize, tcsetwinsize};

    /// Returns the controller (kept open so the terminal stays alive) and the terminal end.
    fn open_terminal(cols: u16) -> io::Result<(OwnedFd, File)> {
        let controller = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY)?;
        grantpt(&controller)?;
        unlockpt(&controller)?;
        let name = ptsname(&controller, Vec::new())?;
        let terminal = OpenOptions::new()
            .read(true)
            .write(true)
            .open(OsStr::from_bytes(name.as_bytes()))?;
        let size = Winsize {
            ws_row: 24,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        tcsetwinsize(&terminal, size)?;
        Ok((controller, terminal))
    }

    /// Runs termqr on 600 zero bytes, which need version 19: 101 columns.
    fn run_wide(stdout: Stdio, stderr: Stdio) -> io::Result<Output> {
        let mut child = Command::new(env!("CARGO_BIN_EXE_termqr"))
            .stdin(Stdio::piped())
            .stdout(stdout)
            .stderr(stderr)
            .spawn()?;
        child
            .stdin
            .take()
            .expect("stdin was configured as piped")
            .write_all(&[0u8; 600])?;
        child.wait_with_output()
    }

    #[test]
    fn wide_code_on_terminal_stdout_fails() {
        let (_controller, terminal) = open_terminal(80).unwrap();
        let out = run_wide(terminal.into(), Stdio::piped()).unwrap();
        assert!(!out.status.success());
        let stderr = String::from_utf8(out.stderr).unwrap();
        assert!(stderr.contains("qr code is 101 columns wide but the terminal is 80"));
    }

    #[test]
    fn terminal_on_stderr_only_is_not_checked() {
        // Models `termqr > out.txt` run from an 80-column terminal.
        let (_controller, terminal) = open_terminal(80).unwrap();
        let out = run_wide(Stdio::piped(), terminal.into()).unwrap();
        assert!(out.status.success());
        assert!(!out.stdout.is_empty());
    }
}
