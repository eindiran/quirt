//! `termqr` command: prints text from an argument, a file, or stdin as a QR code.
//!
//! `main` reads the input and the terminal's state once; choosing the renderer and checking
//! the code's size against the terminal are pure functions of those.

use std::ffi::OsString;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::num::NonZeroU8;
use std::path::PathBuf;

use anyhow::Context;
use clap::{CommandFactory, Parser, ValueEnum};
use termqr::{ErrorCorrection, Glyphs, Rendered, Renderer, Style};

/// Print text as a QR code in the terminal.
///
/// Encodes TEXT if given, else the file from --file, else stdin.
#[derive(Debug, Parser)]
#[command(version)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each bool is an independent command-line switch"
)]
struct Args {
    /// Text to encode, as given (no newline trimming). Several words are joined with
    /// single spaces, as `echo` does.
    #[arg(conflicts_with = "file")]
    text: Vec<OsString>,

    /// File to encode instead of TEXT; `-` reads stdin.
    #[arg(short, long, value_name = "FILE")]
    file: Option<PathBuf>,

    /// Draw plain glyphs with dark modules filled, for light-background terminals. Turns off
    /// `--color auto`; has no effect with `--color always`.
    #[arg(long)]
    invert: bool,

    /// Error-correction level. Higher levels survive more damage but make larger codes.
    #[arg(long, value_enum, ignore_case = true, default_value_t = ErrorCorrection::Medium)]
    ec_level: ErrorCorrection,

    /// When to force black-on-white with ANSI colors. `auto` colors only a terminal
    /// stdout, and not when `NO_COLOR` is set, `TERM` is `dumb`, or `--invert` is given.
    #[arg(long, value_enum, default_value_t = ColorWhen::Auto)]
    color: ColorWhen,

    /// Quiet zone in modules on each side, 1-32. The spec requires 4; most scanners read
    /// 1-2. 0 is refused: the terminal background cannot stand in for it.
    #[arg(long, value_name = "N", default_value_t = termqr::DEFAULT_MARGIN,
          allow_negative_numbers = true,
          value_parser = clap::value_parser!(u8).range(1..=i64::from(MAX_MARGIN)))]
    margin: u8,

    /// Draw each module N times larger, 1-16, to scan from farther away: N columns by N
    /// half-lines with half blocks, 2N columns by N lines with --ascii.
    #[arg(long, value_name = "N", default_value_t = 1,
          allow_negative_numbers = true,
          value_parser = clap::value_parser!(u8).range(1..=i64::from(MAX_SCALE)))]
    scale: u8,

    /// Draw with ASCII only (`##` per module, twice as wide). For terminals that draw
    /// block characters two columns wide. Chosen automatically on a terminal whose locale
    /// is not UTF-8.
    #[arg(long)]
    ascii: bool,

    /// Draw with Unicode half blocks even when the locale is not UTF-8.
    #[arg(long, conflicts_with = "ascii")]
    unicode: bool,

    /// Print even if the code is too wide or tall for the terminal.
    #[arg(long)]
    force: bool,

    /// Encode a trailing newline from a file or stdin instead of stripping it. Input with
    /// NUL bytes or a UTF-16 BOM (UTF-16, UTF-32, binary) is never stripped.
    #[arg(long, conflicts_with = "text")]
    keep_newline: bool,

    /// Print a completion script for SHELL and exit.
    #[arg(long, value_name = "SHELL", exclusive = true)]
    completions: Option<clap_complete::Shell>,

    /// Print the man page (roff) and exit.
    #[arg(long, exclusive = true)]
    man: bool,
}

/// Upper bounds for --margin and --scale. Output is checked against the terminal only when
/// stdout is one, so these keep piped output sane: the largest code at both limits is 3856
/// columns and 18 MB of text with half blocks, 7712 columns and 32 MB with ASCII and ANSI.
const MAX_MARGIN: u8 = 32;
const MAX_SCALE: u8 = 16;

/// Values of --color, following the `ls`/`grep` convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ColorWhen {
    Auto,
    Always,
    Never,
}

/// What stdout and the environment say about the terminal, read once by [`Terminal::detect`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Terminal {
    /// Whether stdout is a terminal.
    tty: bool,
    /// Columns and rows of stdout's terminal; `None` when it is not one or reports 0.
    size: Option<(u16, u16)>,
    /// Neither `NO_COLOR` (<https://no-color.org>, set and non-empty) nor `TERM=dumb`
    /// asks for no color.
    colors_allowed: bool,
    /// Whether the locale is UTF-8; see [`locale_is_utf8`].
    utf8: bool,
}

impl Terminal {
    fn detect() -> Self {
        let var = |name| std::env::var_os(name).filter(|v| !v.is_empty());
        // Only stdout matters: `terminal_size()` would fall back to stderr or stdin and
        // apply the size check to output redirected to a file.
        let size = terminal_size::terminal_size_of(io::stdout())
            .map(|(terminal_size::Width(w), terminal_size::Height(h))| (w, h));
        Self {
            tty: io::stdout().is_terminal(),
            size,
            colors_allowed: var("NO_COLOR").is_none() && var("TERM").is_none_or(|t| t != "dumb"),
            utf8: locale_is_utf8(["LC_ALL", "LC_CTYPE", "LANG"].map(var)),
        }
    }
}

/// Whether the character-type locale is UTF-8, given the non-empty values of `LC_ALL`,
/// `LC_CTYPE`, and `LANG` in that order: the first one present decides, and with none it
/// is POSIX `C`, which is not. Windows consoles do not use these variables (they are
/// normally unset there), so it counts as UTF-8.
fn locale_is_utf8(vars: [Option<OsString>; 3]) -> bool {
    cfg!(windows)
        || vars.into_iter().flatten().next().is_some_and(|v| {
            let v = v.to_string_lossy().to_ascii_lowercase();
            v.contains("utf-8") || v.contains("utf8")
        })
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    if let Some(shell) = args.completions {
        // clap_complete panics on write errors, so render to memory first.
        let mut script = Vec::new();
        clap_complete::generate(shell, &mut Args::command(), "termqr", &mut script);
        return write_stdout(|out| out.write_all(&script));
    }
    if args.man {
        return write_stdout(|out| clap_mangen::Man::new(Args::command()).render(out));
    }

    let data = read_input(&args)?;
    let terminal = Terminal::detect();
    let renderer = renderer(&args, &terminal);
    let code = render(&data, renderer)?;
    if let (false, Some((cols, rows))) = (args.force, terminal.size) {
        check_fits(&data, renderer, &code, &shrinks(&args), cols, rows)?;
    }
    write_stdout(|out| writeln!(out, "{code}"))
}

/// Writes to a buffered stdout, treating a closed pipe (`termqr | head`) as success.
fn write_stdout(write: impl FnOnce(&mut dyn Write) -> io::Result<()>) -> anyhow::Result<()> {
    let mut out = io::BufWriter::new(io::stdout().lock());
    match write(&mut out).and_then(|()| out.flush()) {
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        result => result.context("failed to write to stdout"),
    }
}

/// The bytes to encode: TEXT words joined with spaces, or the file or stdin with one
/// trailing newline trimmed. Fails if they are empty or longer than any code holds.
fn read_input(args: &Args) -> anyhow::Result<Vec<u8>> {
    let data = match &args.file {
        // Arguments are taken verbatim: a newline in them was put there on purpose.
        _ if !args.text.is_empty() => args
            .text
            .iter()
            .map(|t| t.as_encoded_bytes())
            .collect::<Vec<_>>()
            .join(&b' '),
        Some(path) if path.as_os_str() != "-" => {
            let context = || format!("failed to read {}", path.display());
            let file = fs::File::open(path).with_context(context)?;
            trimmed(read_limited(file).with_context(context)?, args.keep_newline)
        }
        _ => trimmed(
            read_limited(io::stdin()).context("failed to read stdin")?,
            args.keep_newline,
        ),
    };
    if data.is_empty() {
        anyhow::bail!("input is empty; nothing to encode");
    }
    if data.len() > termqr::MAX_DATA_LEN {
        anyhow::bail!(
            "input is longer than {} bytes, more than any qr code holds",
            termqr::MAX_DATA_LEN
        );
    }
    Ok(data)
}

/// Reads at most enough of `reader` to tell whether it exceeds [`termqr::MAX_DATA_LEN`],
/// so `yes | termqr` or `termqr /dev/zero` fails fast instead of exhausting memory.
fn read_limited(reader: impl Read) -> io::Result<Vec<u8>> {
    // Room for a trailing "\r\n" that gets trimmed, plus one byte to detect overflow.
    let limit = termqr::MAX_DATA_LEN + 3;
    let mut buf = Vec::new();
    reader
        .take(u64::try_from(limit).expect("limit fits in u64"))
        .read_to_end(&mut buf)?;
    Ok(buf)
}

/// Applies [`trim_trailing_newline`] unless `keep_newline` is set.
fn trimmed(mut data: Vec<u8>, keep_newline: bool) -> Vec<u8> {
    if !keep_newline {
        trim_trailing_newline(&mut data);
    }
    data
}

/// Removes one trailing `\n` or `\r\n`, as left by `echo` or a text editor. Scanners
/// would otherwise include it in the decoded text.
///
/// Input that contains a NUL byte or starts with a UTF-16 byte-order mark is left alone:
/// that is UTF-16, UTF-32, or binary data, where a final 0x0A can be half of a code unit
/// (UTF-16BE `"\n"` is `00 0A`, UTF-16LE `" ી"` is `20 00 C0 0A`). Text in ASCII-compatible
/// encodings never contains NUL. UTF-16 text with no ASCII characters and no BOM has
/// neither and can still lose a final 0x0A (UTF-16BE `"上"` is `4E 0A`); it needs
/// `--keep-newline`.
fn trim_trailing_newline(data: &mut Vec<u8>) {
    if data.contains(&0) || data.starts_with(b"\xfe\xff") || data.starts_with(b"\xff\xfe") {
        return;
    }
    if data.last() == Some(&b'\n') {
        data.pop();
        if data.last() == Some(&b'\r') {
            data.pop();
        }
    }
}

/// The renderer the flags and the terminal ask for.
fn renderer(args: &Args, terminal: &Terminal) -> Renderer {
    let ansi = match args.color {
        ColorWhen::Always => true,
        ColorWhen::Never => false,
        // --invert only means something for plain glyphs, so asking for it opts out.
        ColorWhen::Auto => !args.invert && terminal.tty && terminal.colors_allowed,
    };
    let style = match (ansi, args.invert) {
        (true, _) => Style::Ansi,
        (false, true) => Style::DarkOnLight,
        (false, false) => Style::LightOnDark,
    };
    let ascii = args.ascii || (!args.unicode && terminal.tty && !terminal.utf8);
    Renderer::new()
        .error_correction(args.ec_level)
        .style(style)
        .glyphs(if ascii {
            Glyphs::Ascii
        } else {
            Glyphs::HalfBlock
        })
        .margin(args.margin)
        .scale(NonZeroU8::new(args.scale).expect("clap enforces scale >= 1"))
}

/// Renders `data`, turning a too-long error into one that names the limit and the highest
/// lower level that would fit, if any.
fn render(data: &[u8], renderer: Renderer) -> anyhow::Result<Rendered> {
    match renderer.render(data) {
        Err(termqr::Error::TooLong { len, level }) => {
            // Walk down from `level`: lower levels hold more, so the first fit is the highest.
            let hint = ErrorCorrection::ALL
                .into_iter()
                .rev()
                .skip_while(|&l| l != level)
                .skip(1)
                .find(|&l| renderer.error_correction(l).render(data).is_ok())
                .map_or_else(
                    || "shorten the input".to_owned(),
                    |l| format!("it fits at --ec-level {l}"),
                );
            anyhow::bail!(
                "input is {len} bytes, too long for error-correction level {level} \
                 (at most {} bytes of arbitrary data); {hint}",
                level.max_bytes()
            )
        }
        result => result.context("failed to render input"),
    }
}

/// A flag change the size error can suggest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shrink {
    Margin,
    EcLevel,
    Scale,
}

impl Shrink {
    fn flag(self) -> &'static str {
        match self {
            Self::Margin => "--margin 1",
            Self::EcLevel => "--ec-level L",
            Self::Scale => "--scale 1",
        }
    }

    fn apply(self, renderer: Renderer) -> Renderer {
        match self {
            Self::Margin => renderer.margin(1),
            Self::EcLevel => renderer.error_correction(ErrorCorrection::Low),
            Self::Scale => renderer.scale(NonZeroU8::MIN),
        }
    }
}

/// The shrinks that change something: those whose setting is not already the smallest.
fn shrinks(args: &Args) -> Vec<Shrink> {
    [
        (Shrink::Margin, args.margin > 1),
        (Shrink::EcLevel, args.ec_level != ErrorCorrection::Low),
        (Shrink::Scale, args.scale > 1),
    ]
    .into_iter()
    .filter_map(|(shrink, applies)| applies.then_some(shrink))
    .collect()
}

/// Describes why `code` does not fit: wider than `cols`, taller than the `rows - 1` lines
/// above the next prompt, or both. Returns the description and the generic remedy.
fn size_problem(code: &Rendered, cols: u16, rows: u16) -> Option<(String, &'static str)> {
    let width = code.columns();
    let height = code.lines();
    let visible = usize::from(rows).saturating_sub(1);
    let wide = format!("{width} columns wide but the terminal is {cols}");
    let tall = format!("{height} lines tall but the terminal shows {visible} above the prompt");
    match (width > usize::from(cols), height > visible) {
        (true, true) => Some((
            format!(
                "qr code is {wide}, and {}",
                tall.replace("the terminal", "it")
            ),
            "enlarge the terminal",
        )),
        (true, false) => Some((format!("qr code is {wide}"), "widen the terminal")),
        (false, true) => Some((format!("qr code is {tall}"), "enlarge the terminal")),
        (false, false) => None,
    }
}

/// Fails if `code` does not fit the terminal: a wrapped code, or one whose top scrolled
/// away, cannot be scanned. The error names the fewest `shrinks` that make it fit, if any.
fn check_fits(
    data: &[u8],
    renderer: Renderer,
    code: &Rendered,
    shrinks: &[Shrink],
    cols: u16,
    rows: u16,
) -> anyhow::Result<()> {
    let Some((problem, remedy)) = size_problem(code, cols, rows) else {
        return Ok(());
    };
    // Every non-empty subset of the shrinks (at most 7), smallest first; the first that
    // fits is the suggestion. The sort is stable, so ties keep the order of `shrinks`.
    let mut subsets: Vec<Vec<Shrink>> = (1..1u32 << shrinks.len())
        .map(|mask| {
            (0..shrinks.len())
                .filter(|i| mask >> i & 1 == 1)
                .map(|i| shrinks[i])
                .collect()
        })
        .collect();
    subsets.sort_by_key(Vec::len);
    let fix = subsets.into_iter().find(|chosen| {
        chosen
            .iter()
            .fold(renderer, |r, s| s.apply(r))
            .render(data)
            .is_ok_and(|c| size_problem(&c, cols, rows).is_none())
    });
    match fix {
        Some(chosen) => {
            let flags: Vec<&str> = chosen.iter().map(|s| s.flag()).collect();
            anyhow::bail!(
                "{problem}; {} makes it fit, or pass --force",
                flags.join(" ")
            )
        }
        None => anyhow::bail!("{problem}; {remedy}, shorten the input, or pass --force"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_one_newline() {
        for (input, expected) in [
            (&b"a\n"[..], &b"a"[..]),
            (b"a\r\n", b"a"),
            (b"a\n\n", b"a\n"),
            (b"a", b"a"),
            (b"a\r", b"a\r"),
            (b"", b""),
            // UTF-16BE "a\n" and UTF-16LE " \u{0ac0}": 0x0A is part of a code unit.
            (b"\0a\0\n", b"\0a\0\n"),
            (b" \0\xc0\x0a", b" \0\xc0\x0a"),
            (b"a\0b\n", b"a\0b\n"),
            // UTF-16BE "\u{5411}\u{4e0a}" with BOM has no NUL but ends in 0x0A.
            (b"\xfe\xff\x54\x11\x4e\x0a", b"\xfe\xff\x54\x11\x4e\x0a"),
            (b"\xff\xfe\x0a\x0a", b"\xff\xfe\x0a\x0a"),
            // A UTF-8 BOM is ASCII-compatible.
            (b"\xef\xbb\xbfa\n", b"\xef\xbb\xbfa"),
        ] {
            let mut data = input.to_vec();
            trim_trailing_newline(&mut data);
            assert_eq!(data, expected);
        }
    }

    #[test]
    fn locale_precedence() {
        let utf8 = |vars: [Option<&str>; 3]| locale_is_utf8(vars.map(|v| v.map(OsString::from)));
        assert!(utf8([None, None, Some("en_US.UTF-8")]));
        assert!(utf8([None, Some("UTF-8"), Some("C")]));
        assert!(utf8([Some("C.utf8"), None, None]));
        // Windows ignores the variables and always counts as UTF-8.
        for not_utf8 in [
            [Some("C"), Some("UTF-8"), Some("en_US.UTF-8")],
            [None, None, Some("en_US.ISO8859-1")],
            [None, None, None],
        ] {
            assert_eq!(utf8(not_utf8), cfg!(windows), "{not_utf8:?}");
        }
    }

    /// Parses `termqr ARGS x`.
    fn args(args: &[&str]) -> Args {
        Args::parse_from(["termqr"].iter().chain(args).chain(&["x"]))
    }

    #[test]
    fn renderer_follows_flags_and_terminal() {
        let tty = Terminal {
            tty: true,
            size: Some((80, 24)),
            colors_allowed: true,
            utf8: true,
        };
        let pipe = Terminal {
            tty: false,
            size: None,
            ..tty
        };
        let no_color = Terminal {
            colors_allowed: false,
            ..tty
        };
        let latin1 = Terminal { utf8: false, ..tty };
        let base = Renderer::new();
        let ansi = base.style(Style::Ansi);
        for (flags, terminal, expected) in [
            (&[][..], tty, ansi),
            (&[], pipe, base),
            (&[], no_color, base),
            (&["--color", "always"], pipe, ansi),
            (&["--color", "always"], no_color, ansi),
            (&["--color", "never"], tty, base),
            (&["--invert"], tty, base.style(Style::DarkOnLight)),
            (&["--invert", "--color", "always"], tty, ansi),
            (&[], latin1, ansi.glyphs(Glyphs::Ascii)),
            (&["--unicode"], latin1, ansi),
            // The locale only matters on a terminal.
            (
                &[],
                Terminal {
                    utf8: false,
                    ..pipe
                },
                base,
            ),
            (&["--ascii"], pipe, base.glyphs(Glyphs::Ascii)),
            (
                &["--margin", "2", "--scale", "3", "--ec-level", "q"],
                pipe,
                base.margin(2)
                    .scale(NonZeroU8::new(3).unwrap())
                    .error_correction(ErrorCorrection::Quartile),
            ),
        ] {
            assert_eq!(
                renderer(&args(flags), &terminal),
                expected,
                "{flags:?} {terminal:?}"
            );
        }
    }

    #[test]
    fn shrinks_skip_settings_already_smallest() {
        assert_eq!(shrinks(&args(&[])), [Shrink::Margin, Shrink::EcLevel]);
        assert_eq!(
            shrinks(&args(&["--margin", "1", "--ec-level", "L", "--scale", "2"])),
            [Shrink::Scale]
        );
    }

    const URL: &[u8] = b"https://example.com";

    /// Runs `check_fits` for `data` and returns the error message, if any.
    fn fit_error_for(
        data: &[u8],
        renderer: Renderer,
        shrinks: &[Shrink],
        cols: u16,
        rows: u16,
    ) -> Option<String> {
        let code = renderer.render(data).unwrap();
        check_fits(data, renderer, &code, shrinks, cols, rows)
            .err()
            .map(|e| e.to_string())
    }

    fn fit_error(renderer: Renderer, shrinks: &[Shrink], cols: u16, rows: u16) -> Option<String> {
        fit_error_for(URL, renderer, shrinks, cols, rows)
    }

    #[test]
    fn width_check_boundary() {
        // Version 2 plus quiet zone: 33 columns. ANSI escapes do not count.
        let r = Renderer::new().style(Style::Ansi);
        assert_eq!(fit_error(r, &[], 33, 100), None);
        assert_eq!(
            fit_error(r, &[], 32, 100).unwrap(),
            "qr code is 33 columns wide but the terminal is 32; \
             widen the terminal, shorten the input, or pass --force"
        );
    }

    #[test]
    fn height_check_leaves_a_line_for_the_prompt() {
        // Version 2 plus quiet zone: 33 modules, so 17 lines.
        let r = Renderer::new();
        assert_eq!(fit_error(r, &[], 100, 18), None);
        assert_eq!(
            fit_error(r, &[], 100, 17).unwrap(),
            "qr code is 17 lines tall but the terminal shows 16 above the prompt; \
             enlarge the terminal, shorten the input, or pass --force"
        );
    }

    #[test]
    fn size_error_reports_both_dimensions() {
        // Scale 2: 66 columns, 33 lines.
        let scaled = Renderer::new().scale(NonZeroU8::new(2).unwrap());
        assert_eq!(
            fit_error(scaled, &[], 30, 10).unwrap(),
            "qr code is 66 columns wide but the terminal is 30, and 33 lines tall but it \
             shows 9 above the prompt; enlarge the terminal, shorten the input, or pass --force"
        );
    }

    #[test]
    fn size_error_names_the_fewest_flags_that_fit() {
        let all = [Shrink::Margin, Shrink::EcLevel, Shrink::Scale];
        // 420 bytes: 89 columns at M, 83 with --margin 1, 77 at L (a smaller version).
        assert!(
            fit_error_for(&[b'a'; 420], Renderer::new(), &all, 80, 100)
                .unwrap()
                .ends_with("; --ec-level L makes it fit, or pass --force")
        );
        // 33 columns; --margin 1 makes 27.
        assert_eq!(
            fit_error(Renderer::new(), &all, 30, 100).unwrap(),
            "qr code is 33 columns wide but the terminal is 30; \
             --margin 1 makes it fit, or pass --force"
        );
        // Scale 2 is 66 columns. --margin 1 gives 54 and --ec-level L keeps version 2,
        // so only --scale 1 (33) fits in 40.
        let scaled = Renderer::new().scale(NonZeroU8::new(2).unwrap());
        assert_eq!(
            fit_error(scaled, &all, 40, 100).unwrap(),
            "qr code is 66 columns wide but the terminal is 40; \
             --scale 1 makes it fit, or pass --force"
        );
        // In 30 columns both --margin 1 and --scale 1 are needed (27).
        assert!(
            fit_error(scaled, &all, 30, 100)
                .unwrap()
                .ends_with("; --margin 1 --scale 1 makes it fit, or pass --force")
        );
        // Nothing fits in 10 columns.
        assert!(
            fit_error(scaled, &all, 10, 100)
                .unwrap()
                .ends_with("; widen the terminal, shorten the input, or pass --force")
        );
    }
}
