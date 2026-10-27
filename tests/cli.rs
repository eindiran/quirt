//! End-to-end tests for the `termqr` binary, compared against the library's output.
#![cfg(feature = "cli")]

use std::io::Write;
use std::process::{Command, Output, Stdio};

use termqr::{Colors, ErrorCorrection, Polarity, Renderer};

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
    let code = Renderer::new()
        .polarity(polarity)
        .error_correction(ec)
        .render(data)
        .expect("test inputs fit in a qr code");
    format!("{code}\n")
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
    let out = run(&["--keep-newline", "-f", "-"], b"hello\n");
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        expected("hello\n", Polarity::LightOnDark, ErrorCorrection::Medium)
    );
}

#[test]
fn reads_file_with_invert() {
    let path = std::env::temp_dir().join(format!("termqr-cli-{}.txt", std::process::id()));
    std::fs::write(&path, "from file\n").unwrap();
    let out = run(&["--invert", "-f", path.to_str().unwrap()], b"");
    let _ = std::fs::remove_file(&path); // best-effort cleanup of a temp file
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        expected("from file", Polarity::DarkOnLight, ErrorCorrection::Medium)
    );
}

#[test]
fn missing_file_fails_with_path_in_message() {
    let out = run(&["--file", "/nonexistent/termqr-input"], b"");
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("failed to read /nonexistent/termqr-input"));
}

#[test]
fn text_argument_is_encoded_verbatim() {
    // stdin is ignored when TEXT is given, and a trailing newline in TEXT is kept.
    for text in ["https://example.com", "hello\n"] {
        let out = run(&[text], b"ignored");
        assert!(out.status.success());
        assert_eq!(
            String::from_utf8(out.stdout).unwrap(),
            expected(text, Polarity::LightOnDark, ErrorCorrection::Medium)
        );
    }
}

#[test]
fn dash_as_text_is_literal() {
    let out = run(&["--", "-"], b"ignored");
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        expected("-", Polarity::LightOnDark, ErrorCorrection::Medium)
    );
}

#[test]
fn text_and_file_conflict() {
    let out = run(&["hello", "-f", "/etc/hosts"], b"");
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("cannot be used with"));
}

#[test]
fn empty_text_argument_is_an_error() {
    let out = run(&[""], b"ignored");
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("input is empty; nothing to encode"));
}

#[test]
fn color_auto_is_plain_when_piped() {
    let out = run(&["--color", "auto", "hello"], b"");
    assert!(!out.stdout.contains(&0x1b));
}

#[test]
fn invert_is_ignored_with_colors() {
    let plain = run(&["--color", "always", "hello"], b"");
    let inverted = run(&["--color", "always", "--invert", "hello"], b"");
    assert_eq!(plain.stdout, inverted.stdout);
}

#[test]
fn margin_flag_matches_library() {
    let out = run(&["--margin", "1", "hello"], b"");
    assert!(out.status.success());
    let code = Renderer::new().margin(1).render("hello").unwrap();
    assert_eq!(String::from_utf8(out.stdout).unwrap(), format!("{code}\n"));
    assert!(!run(&["--margin", "256", "hello"], b"").status.success());
}

#[test]
fn polarity_flag_is_gone() {
    let out = run(&["--polarity", "1", "hello"], b"");
    assert!(!out.status.success());
}

#[test]
fn color_always_forces_ansi_even_with_no_color() {
    let ansi = Renderer::new()
        .colors(Colors::Ansi)
        .render("hello")
        .unwrap();
    for no_color in ["", "1"] {
        let out = Command::new(env!("CARGO_BIN_EXE_termqr"))
            .args(["--color", "always", "hello"])
            .env("NO_COLOR", no_color)
            .output()
            .unwrap();
        assert!(out.status.success());
        assert_eq!(String::from_utf8(out.stdout).unwrap(), format!("{ansi}\n"));
    }
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

/// Size-check and color tests against a real pseudo-terminal sized 80x24.
#[cfg(unix)]
mod pty {
    use std::ffi::OsStr;
    use std::fs::{File, OpenOptions};
    use std::io::{self, Read, Write};
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

    /// Runs termqr on `len` zero bytes. 600 bytes need version 19 (101 columns, 51 lines);
    /// 300 need version 13 (77 columns, 39 lines).
    fn run_zeros(len: usize, stdout: Stdio, stderr: Stdio) -> io::Result<Output> {
        let mut child = Command::new(env!("CARGO_BIN_EXE_termqr"))
            .stdin(Stdio::piped())
            .stdout(stdout)
            .stderr(stderr)
            .spawn()?;
        child
            .stdin
            .take()
            .expect("stdin was configured as piped")
            .write_all(&vec![0u8; len])?;
        child.wait_with_output()
    }

    /// Runs termqr with stdout on an 80-column terminal and returns its exit status and
    /// everything it wrote there. The pty turns `\n` into `\r\n`.
    fn run_on_terminal(args: &[&str], env: &[(&str, &str)]) -> io::Result<(bool, Vec<u8>)> {
        let (controller, terminal) = open_terminal(80)?;
        // Read the controller side concurrently so output cannot fill the pty buffer. The
        // read ends with EOF or EIO once the child, the last holder of the terminal, exits.
        let reader = std::thread::spawn(move || {
            let mut out = Vec::new();
            let _ = File::from(controller).read_to_end(&mut out); // EIO marks the end
            out
        });
        let mut child = {
            // Scoped so the Command, holding a copy of the terminal, drops after spawning.
            let mut cmd = Command::new(env!("CARGO_BIN_EXE_termqr"));
            cmd.args(args)
                .stdin(Stdio::null())
                .stdout(terminal)
                .stderr(Stdio::null());
            cmd.env_remove("NO_COLOR");
            for (k, v) in env {
                cmd.env(k, v);
            }
            cmd.spawn()?
        };
        let status = child.wait()?;
        let out = reader.join().expect("reader thread does not panic");
        Ok((status.success(), out))
    }

    #[test]
    fn color_auto_on_terminal_is_ansi() {
        let (ok, out) = run_on_terminal(&["hello"], &[]).unwrap();
        assert!(ok);
        assert!(out.starts_with(b"\x1b[30;107m"));
    }

    #[test]
    fn color_auto_respects_no_color() {
        let (ok, out) = run_on_terminal(&["hello"], &[("NO_COLOR", "1")]).unwrap();
        assert!(ok);
        assert!(!out.is_empty());
        assert!(!out.contains(&0x1b));
    }

    #[test]
    fn wide_code_on_terminal_stdout_fails() {
        let (_controller, terminal) = open_terminal(80).unwrap();
        let out = run_zeros(600, terminal.into(), Stdio::piped()).unwrap();
        assert!(!out.status.success());
        let stderr = String::from_utf8(out.stderr).unwrap();
        assert!(stderr.contains("qr code is 101 columns wide but the terminal is 80"));
    }

    #[test]
    fn tall_code_on_terminal_stdout_fails() {
        let (_controller, terminal) = open_terminal(80).unwrap();
        let out = run_zeros(300, terminal.into(), Stdio::piped()).unwrap();
        assert!(!out.status.success());
        let stderr = String::from_utf8(out.stderr).unwrap();
        assert!(stderr.contains("qr code is 39 lines tall but the terminal shows 23"));
    }

    #[test]
    fn force_skips_size_checks() {
        // 600 bytes need 101 columns, more than the terminal's 80.
        let text = "a".repeat(600);
        let (ok, out) = run_on_terminal(&["--force", &text], &[]).unwrap();
        assert!(ok);
        assert!(!out.is_empty());
        let (ok, _) = run_on_terminal(&[&text], &[]).unwrap();
        assert!(!ok);
    }

    #[test]
    fn terminal_on_stderr_only_is_not_checked() {
        // Models `termqr > out.txt` run from an 80-column terminal.
        let (_controller, terminal) = open_terminal(80).unwrap();
        let out = run_zeros(600, Stdio::piped(), terminal.into()).unwrap();
        assert!(out.status.success());
        assert!(!out.stdout.is_empty());
    }
}
