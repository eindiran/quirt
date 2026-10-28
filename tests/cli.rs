//! `quirt` binary tests, checked against the library's output.
#![cfg(feature = "cli")]

use std::io::Write;
use std::num::NonZeroU8;
use std::process::{Command, Output, Stdio};

use quirt::{ErrorCorrection, Glyphs, Renderer, Style};

fn quirt() -> Command {
    Command::new(env!("CARGO_BIN_EXE_quirt"))
}

fn run(args: &[&str], stdin: &[u8]) -> Output {
    let mut child = quirt()
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("cargo builds the quirt binary for integration tests");
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
    String::from_utf8(out.stdout.clone()).expect("quirt writes UTF-8 or ASCII")
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn expected(renderer: Renderer, data: &str) -> String {
    format!("{}\n", renderer.render(data).expect("test inputs fit"))
}

#[test]
fn flags_match_the_library() {
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
        let out = quirt()
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
    let path = std::env::temp_dir().join(format!("quirt-cli-{}.txt", std::process::id()));
    std::fs::write(&path, "from file\n").unwrap();
    let file = path.to_str().unwrap();
    for (args, stdin, data) in [
        (&[][..], &b"hello\n"[..], "hello"),
        (&["-f", "-"], b"hello\r\n", "hello"),
        (&["--keep-newline", "-f", "-"], b"hello\n", "hello\n"),
        (&["-f", file], b"ignored", "from file"),
        (&["--keep-newline", "--file", file], b"", "from file\n"),
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
            &["--file", "/nonexistent/quirt-input"],
            vec![],
            "failed to read /nonexistent/quirt-input",
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
            [digits(quirt::MAX_DATA_LEN + 1), b"\n".to_vec()].concat(),
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
    // The read limit fits MAX_DATA_LEN plus "\r\n"; pipes skip the size check.
    let longest = [vec![b'7'; quirt::MAX_DATA_LEN], b"\r\n".to_vec()].concat();
    assert!(run(&["--ec-level", "L"], &longest).status.success());
    assert!(run(&[], &[b'a'; 2000]).status.success());
}

#[cfg(unix)]
#[test]
fn endless_stdin_fails_fast() {
    // A deadline, so a read-limit regression fails instead of hanging.
    let mut child = quirt()
        .stdin(std::fs::File::open("/dev/zero").unwrap())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while child.try_wait().unwrap().is_none() {
        if std::time::Instant::now() > deadline {
            let _ = child.kill(); // the test fails below either way
            panic!("quirt kept reading /dev/zero");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("more than any qr code holds"));
}

#[test]
fn largest_output_streams_through_a_closed_pipe() {
    // Largest output into a closed pipe must exit 0.
    let mut child = quirt()
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
        ("bash", "_quirt()"),
        ("zsh", "#compdef quirt"),
        ("fish", "complete -c quirt"),
    ] {
        let script = stdout(&run(&["--completions", shell], b""));
        assert!(script.contains(marker), "{shell} script lacks {marker:?}");
        assert!(script.contains("ec-level"));
    }
    let page = stdout(&run(&["--man"], b""));
    assert!(page.contains(".TH quirt"));
    assert!(page.contains("\\-\\-margin"));
}

#[test]
fn readme_help_is_current() {
    let block = include_str!("../README.md")
        .split_once("`quirt --help`:\n\n```text\n")
        .and_then(|(_, rest)| rest.split_once("\n```"))
        .map(|(block, _)| block)
        .expect("README has a `quirt --help` text block");
    // The README copy has trailing whitespace stripped.
    let help: Vec<String> = stdout(&run(&["--help"], b""))
        .lines()
        .map(|l| l.trim_end().to_owned())
        .collect();
    assert_eq!(
        block,
        help.join("\n"),
        "README help is stale: replace the block with `quirt --help` output"
    );
}

/// Tests with stdout on a real pseudo-terminal.
#[cfg(unix)]
mod pty {
    use std::ffi::OsStr;
    use std::fs::{File, OpenOptions};
    use std::io::{self, Read};
    use std::os::fd::OwnedFd;
    use std::os::unix::ffi::OsStrExt;
    use std::process::{Command, Output, Stdio};

    use quirt::{Renderer, Style};
    use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
    use rustix::termios::{Winsize, tcsetwinsize};

    use super::stderr;

    /// Returns the controller, which must stay open, and the terminal end, `cols` x 24.
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

    /// quirt with a fixed UTF-8, color-capable environment.
    fn quirt(args: &[&str], env: &[(&str, &str)]) -> Command {
        let mut cmd = super::quirt();
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

    /// Returns the run (status, stderr) and what reached the terminal, `\n` as `\r\n`.
    fn run_on_terminal(
        cols: u16,
        args: &[&str],
        env: &[(&str, &str)],
    ) -> io::Result<(Output, Vec<u8>)> {
        let (controller, terminal) = open_terminal(cols)?;
        // Drain the pty so quirt cannot block on a full buffer; EOF or EIO when it exits.
        let reader = std::thread::spawn(move || {
            let mut out = Vec::new();
            let _ = File::from(controller).read_to_end(&mut out); // EIO marks the end
            out
        });
        // A temporary Command, so its copy of the terminal closes after spawn.
        let child = quirt(args, env)
            .stdout(terminal)
            .stderr(Stdio::piped())
            .spawn()?;
        let out = child.wait_with_output()?;
        let written = reader.join().expect("reader thread does not panic");
        Ok((out, written))
    }

    /// Flags, environment, check on the terminal output.
    type Case<'a> = (&'a [&'a str], &'a [(&'a str, &'a str)], fn(&[u8]) -> bool);

    #[test]
    fn environment_picks_colors_and_glyphs() {
        // --margin 1 fits the ASCII code (46x23) in 80x24.
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
            // 600 bytes: 101 columns. 300 bytes: 77x39.
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
            // 58x29; --margin 1 and L are not enough, --scale 1 (29x15) is.
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
        // `quirt > out.txt` from a terminal.
        let (_controller, terminal) = open_terminal(80).unwrap();
        let out = quirt(&[&"a".repeat(600)], &[])
            .stdout(Stdio::piped())
            .stderr(terminal)
            .output()
            .unwrap();
        assert!(out.status.success());
        assert!(!out.stdout.is_empty());
    }
}
