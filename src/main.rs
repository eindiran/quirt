//! `termqr` command: reads data from a file or stdin and prints it as a QR code.

use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, ValueEnum};

/// Print data from a file or stdin as a QR code in the terminal.
#[derive(Debug, Parser)]
#[command(version)]
struct Args {
    /// File to encode. Reads stdin when omitted or `-`.
    file: Option<PathBuf>,

    /// Terminal color scheme, which decides which modules are drawn filled.
    #[arg(long, value_enum, default_value_t = Polarity::LightOnDark)]
    polarity: Polarity,

    /// Error-correction level. Higher levels survive more damage but make larger codes.
    #[arg(long, value_enum, ignore_case = true, default_value_t = EcLevel::M)]
    ec_level: EcLevel,

    /// Encode a trailing newline instead of stripping it.
    #[arg(long)]
    keep_newline: bool,
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

    let mut data = match &args.file {
        Some(path) if path.as_os_str() != "-" => {
            let file = fs::File::open(path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            read_limited(file).with_context(|| format!("failed to read {}", path.display()))?
        }
        _ => read_limited(io::stdin()).context("failed to read stdin")?,
    };
    if !args.keep_newline {
        trim_trailing_newline(&mut data);
    }
    if data.is_empty() {
        anyhow::bail!("input is empty; nothing to encode");
    }
    if data.len() > termqr::MAX_DATA_LEN {
        anyhow::bail!(
            "input is longer than {} bytes, more than any qr code holds",
            termqr::MAX_DATA_LEN
        );
    }

    let text = render(&data, args.polarity.into(), args.ec_level.into())?;
    // Only stdout matters: `terminal_size()` would fall back to stderr or stdin and apply
    // the check to output redirected to a file.
    if let Some((terminal_size::Width(cols), _)) = terminal_size::terminal_size_of(io::stdout()) {
        check_fits(&text, cols)?;
    }
    match writeln!(io::stdout(), "{text}") {
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
fn render(
    data: &[u8],
    polarity: termqr::Polarity,
    level: termqr::ErrorCorrection,
) -> anyhow::Result<String> {
    match termqr::render(data, polarity, level) {
        Err(termqr::Error::TooLong { len, level }) => {
            let lower = termqr::ErrorCorrection::ALL
                .into_iter()
                .take_while(|&l| l != level)
                .collect::<Vec<_>>();
            let hint = match lower
                .into_iter()
                .rev()
                .find(|&l| termqr::render(data, polarity, l).is_ok())
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

/// Fails if `text` has a line wider than `terminal_cols`. A wrapped QR code cannot be
/// scanned, so printing it would only produce garbage.
fn check_fits(text: &str, terminal_cols: u16) -> anyhow::Result<()> {
    let width = text.lines().map(|l| l.chars().count()).max().unwrap_or(0);
    if width > usize::from(terminal_cols) {
        anyhow::bail!(
            "qr code is {width} columns wide but the terminal is {terminal_cols}; \
             widen the terminal or shorten the input"
        );
    }
    Ok(())
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
        let text = termqr::render(
            "https://example.com",
            termqr::Polarity::default(),
            termqr::ErrorCorrection::default(),
        )
        .unwrap();
        assert!(check_fits(&text, 33).is_ok());
        let err = check_fits(&text, 32).unwrap_err().to_string();
        assert_eq!(
            err,
            "qr code is 33 columns wide but the terminal is 32; \
             widen the terminal or shorten the input"
        );
    }
}
