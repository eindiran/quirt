//! `quirt` CLI: prints TEXT, a file, or stdin as a QR code.

use std::ffi::OsString;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::num::NonZeroU8;
use std::path::PathBuf;

use anyhow::Context;
use clap::{CommandFactory, Parser, ValueEnum};
use quirt::{ErrorCorrection, Glyphs, Rendered, Renderer, Style};

/// Print text as a QR code in the terminal.
///
/// Encodes TEXT, else --file, else stdin.
#[derive(Debug, Parser)]
#[command(version)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each bool is an independent command-line switch"
)]
struct Args {
    /// Text to encode verbatim; words are joined with spaces
    #[arg(conflicts_with = "file")]
    text: Vec<OsString>,

    /// File to encode; `-` is stdin
    #[arg(short, long, value_name = "FILE")]
    file: Option<PathBuf>,

    /// Plain glyphs with dark modules filled, for light backgrounds; turns off --color auto
    #[arg(long)]
    invert: bool,

    /// Error-correction level
    #[arg(long, value_enum, ignore_case = true, default_value_t = ErrorCorrection::Medium)]
    ec_level: ErrorCorrection,

    /// ANSI black on white; auto skips pipes, `NO_COLOR`, `TERM=dumb`, and --invert
    #[arg(long, value_enum, default_value_t = ColorWhen::Auto)]
    color: ColorWhen,

    /// Quiet zone in modules, 1-32; most scanners read 1-2
    #[arg(long, value_name = "N", default_value_t = quirt::DEFAULT_MARGIN,
          allow_negative_numbers = true,
          value_parser = clap::value_parser!(u8).range(1..=i64::from(MAX_MARGIN)))]
    margin: u8,

    /// Module size multiplier, 1-16
    #[arg(long, value_name = "N", default_value_t = 1,
          allow_negative_numbers = true,
          value_parser = clap::value_parser!(u8).range(1..=i64::from(MAX_SCALE)))]
    scale: u8,

    /// Draw with `##` (twice as wide); automatic on a terminal with a non-UTF-8 locale
    #[arg(long)]
    ascii: bool,

    /// Draw with half blocks even if the locale is not UTF-8
    #[arg(long, conflicts_with = "ascii")]
    unicode: bool,

    /// Print even if the code does not fit the terminal
    #[arg(long)]
    force: bool,

    /// Keep the trailing newline of a file or stdin
    #[arg(long, conflicts_with = "text")]
    keep_newline: bool,

    /// Print a completion script and exit
    #[arg(long, value_name = "SHELL", exclusive = true)]
    completions: Option<clap_complete::Shell>,

    /// Print the man page and exit
    #[arg(long, exclusive = true)]
    man: bool,
}

// Piped output skips the size check; these cap it at about 32 MB.
const MAX_MARGIN: u8 = 32;
const MAX_SCALE: u8 = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ColorWhen {
    Auto,
    Always,
    Never,
}

/// Stdout's terminal and the environment, read once so the decisions below are pure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Terminal {
    tty: bool,
    /// Columns and rows; `None` when not a terminal or it reports 0x0.
    size: Option<(u16, u16)>,
    /// No `NO_COLOR` and no `TERM=dumb`.
    colors_allowed: bool,
    utf8: bool,
}

impl Terminal {
    fn detect() -> Self {
        let var = |name| std::env::var_os(name).filter(|v| !v.is_empty());
        // Not terminal_size(): it falls back to stderr, which would check `quirt > file`.
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

/// The first set of `LC_ALL`, `LC_CTYPE`, `LANG` decides; none means `C`. Windows does not
/// use them, so it counts as UTF-8.
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
        // clap_complete panics on write errors.
        let mut script = Vec::new();
        clap_complete::generate(shell, &mut Args::command(), "quirt", &mut script);
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

/// Buffered stdout; a closed pipe (`quirt | head`) is success.
fn write_stdout(write: impl FnOnce(&mut dyn Write) -> io::Result<()>) -> anyhow::Result<()> {
    let mut out = io::BufWriter::new(io::stdout().lock());
    match write(&mut out).and_then(|()| out.flush()) {
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        result => result.context("failed to write to stdout"),
    }
}

fn read_input(args: &Args) -> anyhow::Result<Vec<u8>> {
    let data = match &args.file {
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
    if data.len() > quirt::MAX_DATA_LEN {
        anyhow::bail!(
            "input is longer than {} bytes, more than any qr code holds",
            quirt::MAX_DATA_LEN
        );
    }
    Ok(data)
}

/// Stops just past [`quirt::MAX_DATA_LEN`], so `yes | quirt` fails fast.
fn read_limited(reader: impl Read) -> io::Result<Vec<u8>> {
    // A trimmable "\r\n" plus one byte to detect overflow.
    let limit = quirt::MAX_DATA_LEN + 3;
    let mut buf = Vec::new();
    reader
        .take(u64::try_from(limit).expect("limit fits in u64"))
        .read_to_end(&mut buf)?;
    Ok(buf)
}

fn trimmed(mut data: Vec<u8>, keep_newline: bool) -> Vec<u8> {
    if !keep_newline {
        trim_trailing_newline(&mut data);
    }
    data
}

/// Removes one trailing `\n` or `\r\n`. Skips input with a NUL byte or UTF-16 BOM:
/// UTF-16, UTF-32, or binary, where 0x0A may be half a code unit. BOM-less UTF-16 with no
/// ASCII has neither (UTF-16BE `"上"` is `4E 0A`) and still needs `--keep-newline`.
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

fn renderer(args: &Args, terminal: &Terminal) -> Renderer {
    let ansi = match args.color {
        ColorWhen::Always => true,
        ColorWhen::Never => false,
        // --invert only affects plain glyphs.
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

/// Adds the byte limit and the highest lower level that fits to `TooLong`.
fn render(data: &[u8], renderer: Renderer) -> anyhow::Result<Rendered> {
    match renderer.render(data) {
        Err(quirt::Error::TooLong { len, level }) => {
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

/// A flag change a size error can suggest.
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

/// Shrinks that would change a setting.
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

/// Why `code` overflows `cols` x (`rows` - 1 for the prompt), and the generic remedy.
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

/// Fails if `code` would wrap or scroll off, naming the fewest `shrinks` that fit.
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
    // Non-empty subsets, smallest first; the stable sort keeps `shrinks` order on ties.
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
            // UTF-16BE "a\n", UTF-16LE " \u{0ac0}", UTF-16BE "\u{5411}\u{4e0a}" with BOM.
            (b"\0a\0\n", b"\0a\0\n"),
            (b" \0\xc0\x0a", b" \0\xc0\x0a"),
            (b"a\0b\n", b"a\0b\n"),
            (b"\xfe\xff\x54\x11\x4e\x0a", b"\xfe\xff\x54\x11\x4e\x0a"),
            (b"\xff\xfe\x0a\x0a", b"\xff\xfe\x0a\x0a"),
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
        for not_utf8 in [
            [Some("C"), Some("UTF-8"), Some("en_US.UTF-8")],
            [None, None, Some("en_US.ISO8859-1")],
            [None, None, None],
        ] {
            assert_eq!(utf8(not_utf8), cfg!(windows), "{not_utf8:?}");
        }
    }

    fn args(args: &[&str]) -> Args {
        Args::parse_from(["quirt"].iter().chain(args).chain(&["x"]))
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
        // 33x17; escapes do not count.
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
        // 420 bytes: 89 columns, 83 with --margin 1, 77 at L.
        assert!(
            fit_error_for(&[b'a'; 420], Renderer::new(), &all, 80, 100)
                .unwrap()
                .ends_with("; --ec-level L makes it fit, or pass --force")
        );
        assert_eq!(
            fit_error(Renderer::new(), &all, 30, 100).unwrap(),
            "qr code is 33 columns wide but the terminal is 30; \
             --margin 1 makes it fit, or pass --force"
        );
        // 66 columns; --margin 1 gives 54 and L keeps the version, so only --scale 1 fits.
        let scaled = Renderer::new().scale(NonZeroU8::new(2).unwrap());
        assert_eq!(
            fit_error(scaled, &all, 40, 100).unwrap(),
            "qr code is 66 columns wide but the terminal is 40; \
             --scale 1 makes it fit, or pass --force"
        );
        assert!(
            fit_error(scaled, &all, 30, 100)
                .unwrap()
                .ends_with("; --margin 1 --scale 1 makes it fit, or pass --force")
        );
        assert!(
            fit_error(scaled, &all, 10, 100)
                .unwrap()
                .ends_with("; widen the terminal, shorten the input, or pass --force")
        );
    }
}
