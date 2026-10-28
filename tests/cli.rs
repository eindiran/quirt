//! End-to-end tests for the `termqr` binary, compared against the library's output.
#![cfg(feature = "cli")]

use std::io::Write;
use std::num::NonZeroU8;
use std::process::{Command, Output, Stdio};

use termqr::{ErrorCorrection, Glyphs, Renderer, Style};

fn termqr() -> Command {
    Command::new(env!("CARGO_BIN_EXE_termqr"))
}

/// Runs termqr with `args`, feeding `stdin`, with stdout and stderr piped.
fn run(args: &[&str], stdin: &[u8]) -> Output {
    let mut child = termqr()
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

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("termqr writes UTF-8 or ASCII")
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// What `renderer` prints for `data`, with the trailing newline termqr adds.
fn expected(renderer: Renderer, data: &str) -> String {
    format!("{}\n", renderer.render(data).expect("test inputs fit"))
}

#[test]
fn flags_match_the_library() {
    // Piped stdout: no colors or ASCII unless asked for.
    let base = Renderer::new();
    for (flags, renderer) in [
        (&[][..], base),
        (&["--invert"], base.style(Style::DarkOnLight)),
        (&["--color", "always"], base.style(Style::Ansi)),
        (&["--color", "always", "--invert"], base.style(Style::Ansi)),
        (&["--ascii"], base.glyphs(Glyphs::Ascii)),
        (&["--margin", "1"], base.margin(1)),
        (&["--scale", "2"], base.scale(NonZeroU8::new(2).unwrap())),
        (
            &["--ec-level", "H"],
            base.error_correction(ErrorCorrection::High),
        ),
        (
            &["--ec-level", "h"],
            base.error_correction(ErrorCorrection::High),
        ),
        (&["--margin", "32"], base.margin(32)),
        (&["--scale", "16"], base.scale(NonZeroU8::new(16).unwrap())),
    ] {
        let out = run(&[flags, &["hello"]].concat(), b"");
        assert!(out.status.success(), "{flags:?}: {}", stderr(&out));
        assert_eq!(stdout(&out), expected(renderer, "hello"), "{flags:?}");
    }
}

#[test]
fn color_always_overrides_no_color() {
    for no_color in ["", "1"] {
        let out = termqr()
            .args(["--color", "always", "hello"])
            .env("NO_COLOR", no_color)
            .output()
            .unwrap();
        assert_eq!(
            stdout(&out),
            expected(Renderer::new().style(Style::Ansi), "hello")
        );
    }
}

#[test]
fn input_sources() {
    let plain = Renderer::new();
    let path = std::env::temp_dir().join(format!("termqr-cli-{}.txt", std::process::id()));
    std::fs::write(&path, "from file\n").unwrap();
    let file = path.to_str().unwrap();
    for (args, stdin, data) in [
        // stdin and files lose one trailing newline unless --keep-newline.
        (&[][..], &b"hello\n"[..], "hello"),
        (&["-f", "-"], b"hello\r\n", "hello"),
        (&["--keep-newline", "-f", "-"], b"hello\n", "hello\n"),
        (&["-f", file], b"ignored", "from file"),
        (&["--keep-newline", "--file", file], b"", "from file\n"),
        // TEXT is verbatim, ignores stdin, and joins words with spaces.
        (&["https://example.com"], b"ignored", "https://example.com"),
        (&["hello\n"], b"", "hello\n"),
        (&["hello", "big", "world"], b"", "hello big world"),
        (&["--", "-"], b"ignored", "-"),
        (&["--", "--man"], b"", "--man"),
    ] {
        let out = run(args, stdin);
        assert!(out.status.success(), "{args:?}: {}", stderr(&out));
        assert_eq!(stdout(&out), expected(plain, data), "{args:?}");
    }
    let _ = std::fs::remove_file(&path); // best-effort cleanup of a temp file
}

#[test]
fn usage_errors_exit_2() {
    for (args, message) in [
        (&["--margin", "0", "x"][..], "0 is not in 1..=32"),
        (&["--margin", "33", "x"], "33 is not in 1..=32"),
        (&["--margin", "-1", "x"], "-1 is not in 1..=32"),
        (&["--scale", "0", "x"], "0 is not in 1..=16"),
        (&["--scale", "17", "x"], "17 is not in 1..=16"),
        (&["--keep-newline", "x"], "cannot be used with"),
        (&["x", "-f", "/etc/hosts"], "cannot be used with"),
        (&["--ascii", "--unicode", "x"], "cannot be used with"),
        (&["--man", "x"], "cannot be used with"),
        (&["--completions", "bash", "--ascii"], "cannot be used with"),
        (&["--completions", "tcsh"], "invalid value 'tcsh'"),
        (
            &["--polarity", "1", "x"],
            "unexpected argument '--polarity'",
        ),
    ] {
        let out = run(args, b"");
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(stderr(&out).contains(message), "{args:?}: {}", stderr(&out));
        assert!(out.stdout.is_empty());
    }
}

#[test]
fn input_errors_exit_1() {
    let digits = |n| vec![b'7'; n];
    for (args, stdin, message) in [
        (&[][..], b"".to_vec(), "input is empty; nothing to encode"),
        (&[], b"\n".to_vec(), "input is empty; nothing to encode"),
        (&[], b"\r\n".to_vec(), "input is empty; nothing to encode"),
        (&[""], vec![], "input is empty; nothing to encode"),
        (
            &["--file", "/nonexistent/termqr-input"],
            vec![],
            "failed to read /nonexistent/termqr-input",
        ),
        (
            &["--ec-level", "H"],
            vec![b'a'; 1274],
            "input is 1274 bytes, too long for error-correction level H \
             (at most 1273 bytes of arbitrary data); it fits at --ec-level Q",
        ),
        (
            &["--ec-level", "L"],
            vec![0; 2954],
            "level L (at most 2953 bytes of arbitrary data); shorten the input",
        ),
        (
            &["--ec-level", "L"],
            [digits(termqr::MAX_DATA_LEN + 1), b"\n".to_vec()].concat(),
            "input is longer than 7089 bytes, more than any qr code holds",
        ),
    ] {
        let out = run(args, &stdin);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert!(stderr(&out).contains(message), "{args:?}: {}", stderr(&out));
        assert!(out.stdout.is_empty());
    }
}

#[test]
fn input_size_limits() {
    // The read limit leaves room for a trimmed "\r\n" after MAX_DATA_LEN digits, and
    // piped output is never size-checked, so the 185-column version 40 prints.
    let longest = [vec![b'7'; termqr::MAX_DATA_LEN], b"\r\n".to_vec()].concat();
    assert!(run(&["--ec-level", "L"], &longest).status.success());
    assert!(run(&[], &[b'a'; 2000]).status.success());
}

#[cfg(unix)]
#[test]
fn endless_stdin_fails_fast() {
    // Unbounded reading would never finish on /dev/zero; give it 10 s, so a regression
    // fails instead of hanging the test run.
    let mut child = termqr()
        .stdin(std::fs::File::open("/dev/zero").unwrap())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while child.try_wait().unwrap().is_none() {
        if std::time::Instant::now() > deadline {
            let _ = child.kill(); // the test fails below either way
            panic!("termqr kept reading /dev/zero");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("more than any qr code holds"));
}

#[test]
fn largest_output_streams_through_a_closed_pipe() {
    // Largest code at both limits, piped into a reader that stops at once: termqr must
    // exit 0 on the broken pipe. That streaming keeps memory low is checked by the
    // library's display_writes_one_line_at_a_time.
    let mut child = termqr()
        .args([
            "--scale",
            "16",
            "--margin",
            "32",
            "--ec-level",
            "L",
            "-f",
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(&[0u8; 2953]).unwrap();
    drop(child.stdout.take());
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
}

#[test]
fn generators() {
    for (shell, marker) in [
        ("bash", "_termqr()"),
        ("zsh", "#compdef termqr"),
        ("fish", "complete -c termqr"),
    ] {
        let script = stdout(&run(&["--completions", shell], b""));
        assert!(script.contains(marker), "{shell} script lacks {marker:?}");
        assert!(script.contains("ec-level"));
    }
    let page = stdout(&run(&["--man"], b""));
    assert!(page.contains(".TH termqr"));
    assert!(page.contains("\\-\\-margin"));
}

/// Tests against a real pseudo-terminal sized `cols` x 24 on stdout.
#[cfg(unix)]
mod pty {
    use std::ffi::OsStr;
    use std::fs::{File, OpenOptions};
    use std::io::{self, Read};
    use std::os::fd::OwnedFd;
    use std::os::unix::ffi::OsStrExt;
    use std::process::{Command, Output, Stdio};

    use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
    use rustix::termios::{Winsize, tcsetwinsize};
    use termqr::{Renderer, Style};

    use super::stderr;

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

    /// A termqr command with the terminal-facing environment fixed: a UTF-8 locale, a color
    /// `TERM`, and no `NO_COLOR`, so results do not depend on the caller's environment.
    fn termqr(args: &[&str], env: &[(&str, &str)]) -> Command {
        let mut cmd = super::termqr();
        cmd.args(args)
            .env("LANG", "en_US.UTF-8")
            .env("TERM", "xterm-256color")
            .env_remove("LC_ALL")
            .env_remove("LC_CTYPE")
            .env_remove("NO_COLOR")
            .envs(env.iter().copied())
            .stdin(Stdio::null());
        cmd
    }

    /// Runs termqr with stdout on a `cols` x 24 terminal and returns its exit status and
    /// stderr, plus everything it wrote to the terminal (where `\n` becomes `\r\n`).
    fn run_on_terminal(
        cols: u16,
        args: &[&str],
        env: &[(&str, &str)],
    ) -> io::Result<(Output, Vec<u8>)> {
        let (controller, terminal) = open_terminal(cols)?;
        // Read the controller side concurrently so output cannot fill the pty buffer and
        // block termqr. The read ends with EOF or EIO once the child, the last holder of
        // the terminal, exits.
        let reader = std::thread::spawn(move || {
            let mut out = Vec::new();
            let _ = File::from(controller).read_to_end(&mut out); // EIO marks the end
            out
        });
        // The Command holds a copy of the terminal; it is a temporary, dropped after spawn.
        let child = termqr(args, env)
            .stdout(terminal)
            .stderr(Stdio::piped())
            .spawn()?;
        let out = child.wait_with_output()?;
        let written = reader.join().expect("reader thread does not panic");
        Ok((out, written))
    }

    /// Flags, environment overrides, and a check on what reached the terminal.
    type Case<'a> = (&'a [&'a str], &'a [(&'a str, &'a str)], fn(&[u8]) -> bool);

    #[test]
    fn environment_picks_colors_and_glyphs() {
        // --margin 1 keeps the ASCII code (46x23) inside the 80x24 terminal.
        let ansi = |out: &[u8]| out.starts_with(b"\x1b[30;107m");
        let plain_blocks = |out: &[u8]| !out.contains(&0x1b) && !out.is_ascii();
        let ascii = |out: &[u8]| out.is_ascii();
        let cases: &[Case] = &[
            (&[], &[], ansi),
            (&[], &[("NO_COLOR", "1")], plain_blocks),
            (&[], &[("NO_COLOR", "")], ansi),
            (&[], &[("TERM", "dumb")], plain_blocks),
            (&["--color", "never"], &[("LANG", "C")], ascii),
            (&["--color", "never"], &[("LANG", "en_US.ISO8859-1")], ascii),
            (&["--color", "never"], &[("LC_ALL", "C")], ascii),
            (
                &["--color", "never", "--unicode"],
                &[("LANG", "C")],
                plain_blocks,
            ),
        ];
        for (flags, env, check) in cases {
            let args = [flags, &["--margin", "1", "hello"][..]].concat();
            let (out, written) = run_on_terminal(80, &args, env).unwrap();
            assert!(out.status.success(), "{flags:?} {env:?}");
            let out = written;
            assert!(
                check(&out),
                "{flags:?} {env:?}: {:?}",
                String::from_utf8_lossy(&out)
            );
        }
    }

    #[test]
    fn invert_turns_off_auto_color_on_terminal() {
        let (out, written) = run_on_terminal(80, &["--invert", "hello"], &[]).unwrap();
        assert!(out.status.success());
        let out = written;
        let code = Renderer::new()
            .style(Style::DarkOnLight)
            .render("hello")
            .unwrap();
        let expected = format!("{code}\n").replace('\n', "\r\n");
        assert_eq!(String::from_utf8(out).unwrap(), expected);
    }

    #[test]
    fn size_errors_on_terminal_stdout() {
        let bytes = |n| "\u{1}".repeat(n);
        for (cols, args, message) in [
            // 600 bytes: version 19, 101 columns. 300 bytes: version 13, 77x39.
            (
                80,
                vec![bytes(600)],
                "qr code is 101 columns wide but the terminal is 80",
            ),
            (
                80,
                vec![bytes(300)],
                "qr code is 39 lines tall but the terminal shows 23",
            ),
            // --scale 2 makes "hello" 58x29. In 40x24 --margin 1 (46 columns) and
            // --ec-level L (still version 1) are not enough; --scale 1 (29x15) fits.
            (
                40,
                vec!["--scale".into(), "2".into(), "hello".into()],
                "qr code is 58 columns wide but the terminal is 40, and 29 lines tall but it \
                 shows 23 above the prompt; --scale 1 makes it fit",
            ),
        ] {
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            let (out, written) = run_on_terminal(cols, &args, &[]).unwrap();
            assert_eq!(out.status.code(), Some(1));
            assert!(written.is_empty());
            assert!(stderr(&out).contains(message), "{}", stderr(&out));
        }
    }

    #[test]
    fn force_skips_size_checks() {
        let text = "a".repeat(600);
        let (out, written) = run_on_terminal(80, &["--force", &text], &[]).unwrap();
        assert!(out.status.success() && !written.is_empty());
        let (out, _) = run_on_terminal(80, &[&text], &[]).unwrap();
        assert!(!out.status.success());
    }

    #[test]
    fn terminal_on_stderr_only_is_not_checked() {
        // Models `termqr > out.txt` run from an 80-column terminal.
        let (_controller, terminal) = open_terminal(80).unwrap();
        let out = termqr(&[&"a".repeat(600)], &[])
            .stdout(Stdio::piped())
            .stderr(terminal)
            .output()
            .unwrap();
        assert!(out.status.success());
        assert!(!out.stdout.is_empty());
    }
}
