//! `termqr` command: prints text from an argument, a file, or stdin as a QR code.

use std::ffi::OsString;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, ValueEnum};

/// Print text as a QR code in the terminal.
///
/// Encodes TEXT if given, else the file from --file, else stdin.
#[derive(Debug, Parser)]
#[command(version)]
struct Args {
    /// Text to encode, as given (no newline trimming).
    #[arg(conflicts_with = "file")]
    text: Option<OsString>,

    /// File to encode instead of TEXT; `-` reads stdin.
    #[arg(short, long, value_name = "FILE")]
    file: Option<PathBuf>,

    /// Terminal color scheme, which decides which modules are drawn filled.
    #[arg(long, value_enum, default_value_t = Polarity::LightOnDark)]
    polarity: Polarity,

    /// Error-correction level. Higher levels survive more damage but make larger codes.
    #[arg(long, value_enum, ignore_case = true, default_value_t = EcLevel::M)]
    ec_level: EcLevel,

    /// When to force black-on-white with ANSI colors. `auto` colors only a terminal
    /// stdout, and not when `NO_COLOR` is set. `--polarity` applies only without colors.
    #[arg(long, value_enum, default_value_t = ColorWhen::Auto)]
    color: ColorWhen,

    /// Encode a trailing newline from a file or stdin instead of stripping it.
    #[arg(long)]
    keep_newline: bool,
}

/// Values of --color, following the `ls`/`grep` convention.
#[derive(Debug, Clone, Copy, ValueEnum)]
enum ColorWhen {
    Auto,
    Always,
    Never,
}

/// CLI mirror of [`termqr::ErrorCorrection`], spelled with the standard single letters.
#[derive(Debug, Clone, Copy, ValueEnum)]
enum EcLevel {
    /// ~7% recoverable, up to 2953 bytes
    #[value(name = "L")]
    L,
    /// ~15% recoverable, up to 2331 bytes
    #[value(name = "M")]
    M,
    /// ~25% recoverable, up to 1663 bytes
    #[value(name = "Q")]
    Q,
    /// ~30% recoverable, up to 1273 bytes
    #[value(name = "H")]
    H,
}

impl From<EcLevel> for termqr::ErrorCorrection {
    fn from(level: EcLevel) -> Self {
        match level {
            EcLevel::L => Self::Low,
            EcLevel::M => Self::Medium,
            EcLevel::Q => Self::Quartile,
            EcLevel::H => Self::High,
        }
    }
}

/// CLI mirror of [`termqr::Polarity`], which is `#[non_exhaustive]` and has no clap derive.
/// Spelled `0`/`1` on the command line.
#[derive(Debug, Clone, Copy, ValueEnum)]
enum Polarity {
    /// Light-on-dark terminal theme
    #[value(name = "0")]
    LightOnDark,
    /// Dark-on-light terminal theme
    #[value(name = "1")]
    DarkOnLight,
}

impl From<Polarity> for termqr::Polarity {
    fn from(p: Polarity) -> Self {
        match p {
            Polarity::LightOnDark => Self::LightOnDark,
            Polarity::DarkOnLight => Self::DarkOnLight,
        }
    }
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let data = match (args.text, &args.file) {
        // The argument is taken verbatim: a newline in it was put there on purpose.
        (Some(text), _) => text.into_encoded_bytes(),
        (None, Some(path)) if path.as_os_str() != "-" => {
            let file = fs::File::open(path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            let data =
                read_limited(file).with_context(|| format!("failed to read {}", path.display()))?;
            trimmed(data, args.keep_newline)
        }
        (None, _) => trimmed(
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

    let ansi = match args.color {
        ColorWhen::Always => true,
        ColorWhen::Never => false,
        ColorWhen::Auto => io::stdout().is_terminal() && !no_color(),
    };
    let colors = if ansi {
        termqr::Colors::Ansi
    } else {
        termqr::Colors::Plain
    };
    let renderer = termqr::Renderer::new()
        .error_correction(args.ec_level.into())
        .polarity(args.polarity.into())
        .colors(colors);
    let code = render(&data, renderer)?;
    // Only stdout matters: `terminal_size()` would fall back to stderr or stdin and apply
    // the check to output redirected to a file.
    if let Some((terminal_size::Width(cols), _)) = terminal_size::terminal_size_of(io::stdout()) {
        check_fits(&code, cols)?;
    }
    match writeln!(io::stdout(), "{code}") {
        // The reader closed early (`termqr | head`); that is not a failure of ours.
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        result => result.context("failed to write to stdout"),
    }
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

/// Renders `data`, turning a too-long error into one that names the limit and the highest
/// lower level that would fit, if any.
fn render(data: &[u8], renderer: termqr::Renderer) -> anyhow::Result<termqr::Rendered> {
    match renderer.render(data) {
        Err(termqr::Error::TooLong { len, level }) => {
            let lower = termqr::ErrorCorrection::ALL
                .into_iter()
                .take_while(|&l| l != level)
                .collect::<Vec<_>>();
            let hint = match lower
                .into_iter()
                .rev()
                .find(|&l| renderer.error_correction(l).render(data).is_ok())
            {
                Some(l) => format!("it fits at --ec-level {l}"),
                None => "shorten the input".to_owned(),
            };
            anyhow::bail!(
                "input is {len} bytes, too long for error-correction level {level} \
                 (at most {} bytes of arbitrary data); {hint}",
                level.max_bytes()
            )
        }
        result => result.context("failed to render input"),
    }
}

/// Whether `NO_COLOR` (<https://no-color.org>) asks for no color: set and non-empty.
fn no_color() -> bool {
    std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty())
}

/// Fails if `code` is wider than `terminal_cols`. A wrapped QR code cannot be scanned, so
/// printing it would only produce garbage.
fn check_fits(code: &termqr::Rendered, terminal_cols: u16) -> anyhow::Result<()> {
    let width = code.columns();
    if width > usize::from(terminal_cols) {
        anyhow::bail!(
            "qr code is {width} columns wide but the terminal is {terminal_cols}; \
             widen the terminal or shorten the input"
        );
    }
    Ok(())
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
fn trim_trailing_newline(data: &mut Vec<u8>) {
    if data.last() == Some(&b'\n') {
        data.pop();
        if data.last() == Some(&b'\r') {
            data.pop();
        }
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
        ] {
            let mut data = input.to_vec();
            trim_trailing_newline(&mut data);
            assert_eq!(data, expected);
        }
    }

    #[test]
    fn width_check_boundary() {
        // Version 2 plus quiet zone: 33 columns.
        let code = termqr::Renderer::new()
            .colors(termqr::Colors::Ansi)
            .render("https://example.com")
            .unwrap();
        assert!(check_fits(&code, 33).is_ok());
        let err = check_fits(&code, 32).unwrap_err().to_string();
        assert_eq!(
            err,
            "qr code is 33 columns wide but the terminal is 32; \
             widen the terminal or shorten the input"
        );
    }
}
