//! Render QR codes for display in a terminal emulator.
//!
//! [`Renderer`] encodes data with the [`qrcode`] crate and returns a [`Rendered`] code,
//! which draws itself as text through `Display`.
//!
//! ```
//! let code = termqr::Renderer::new().render("https://example.com")?;
//! println!("{code}");
//! # Ok::<(), termqr::Error>(())
//! ```

use std::fmt;
use std::iter;
use std::num::NonZeroU8;

use qrcode::bits::Bits;
use qrcode::optimize::{Optimizer, Parser, Segment};
use qrcode::types::{Color, Mode, QrError, Version};
use qrcode::{EcLevel, QrCode};

/// Longest input, in bytes, that any QR code can hold: 7089 digits at version 40, level L.
/// Longer input always fails, so readers can stop after `MAX_DATA_LEN + 1` bytes.
pub const MAX_DATA_LEN: usize = 7089;

/// Modules of light border (quiet zone) the QR spec requires around the code.
pub const DEFAULT_MARGIN: u8 = 4;

/// Errors returned while building a QR code.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// `len` bytes do not fit in the largest QR version at `level`.
    #[error("{len} bytes do not fit in a qr code at error-correction level {level}")]
    TooLong {
        /// Input length in bytes.
        len: usize,
        /// Level the encode was attempted at.
        level: ErrorCorrection,
    },
    /// The encoder rejected the input for a reason other than its length.
    #[error("failed to encode data as a qr code")]
    Encode(#[source] QrError),
}

/// QR error-correction level: the share of codewords that can be damaged and still decode.
///
/// Higher levels survive more damage but hold less data, so the same input needs a larger
/// code. The byte capacity listed for each level is that of version 40, the largest code.
/// With the `cli` feature it is a `clap::ValueEnum` spelled `L`, `M`, `Q`, `H`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "cli", derive(clap::ValueEnum))]
#[non_exhaustive]
pub enum ErrorCorrection {
    /// About 7% recoverable; at most 2953 bytes.
    #[cfg_attr(feature = "cli", value(name = "L"))]
    Low,
    /// About 15% recoverable; at most 2331 bytes.
    #[default]
    #[cfg_attr(feature = "cli", value(name = "M"))]
    Medium,
    /// About 25% recoverable; at most 1663 bytes.
    #[cfg_attr(feature = "cli", value(name = "Q"))]
    Quartile,
    /// About 30% recoverable; at most 1273 bytes.
    #[cfg_attr(feature = "cli", value(name = "H"))]
    High,
}

impl ErrorCorrection {
    /// All levels, from least to most error correction.
    pub const ALL: [Self; 4] = [Self::Low, Self::Medium, Self::Quartile, Self::High];

    /// Most bytes of arbitrary data a version-40 code holds at this level. Input made only
    /// of digits or uppercase alphanumerics packs denser and can be longer.
    #[must_use]
    pub const fn max_bytes(self) -> usize {
        match self {
            Self::Low => 2953,
            Self::Medium => 2331,
            Self::Quartile => 1663,
            Self::High => 1273,
        }
    }
}

/// Formats as the standard single letter: `L`, `M`, `Q`, or `H`.
impl fmt::Display for ErrorCorrection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Low => "L",
            Self::Medium => "M",
            Self::Quartile => "Q",
            Self::High => "H",
        })
    }
}

impl From<ErrorCorrection> for EcLevel {
    fn from(level: ErrorCorrection) -> Self {
        match level {
            ErrorCorrection::Low => Self::L,
            ErrorCorrection::Medium => Self::M,
            ErrorCorrection::Quartile => Self::Q,
            ErrorCorrection::High => Self::H,
        }
    }
}

/// How modules map to colors. Scanners need dark modules on a light background.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Style {
    /// Plain glyphs in the terminal's colors with light modules filled, for terminals with
    /// a dark background.
    #[default]
    LightOnDark,
    /// Plain glyphs with dark modules filled, for terminals with a light background.
    DarkOnLight,
    /// ANSI escapes force black modules on a bright-white background on every line, so
    /// the code scans on any theme: SGR `30;107` with half blocks, `40` and `107`
    /// backgrounds with ASCII.
    Ansi,
}

/// Characters the code is drawn with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Glyphs {
    /// Unicode half blocks: one column and half a line per module.
    #[default]
    HalfBlock,
    /// ASCII only: `##` or two spaces per module, one line tall, so twice as wide as
    /// [`Glyphs::HalfBlock`]. Works in non-UTF-8 terminals and in terminals that draw the
    /// block characters two columns wide. With [`Style::Ansi`], modules are colored
    /// spaces.
    Ascii,
}

/// Renders QR codes as terminal text. Modules stay square in the usual 1:2 character cell:
/// [`Glyphs::HalfBlock`] puts two module rows on each line, [`Glyphs::Ascii`] draws each
/// module two characters wide.
///
/// ```
/// use termqr::{ErrorCorrection, Renderer, Style};
///
/// let code = Renderer::new()
///     .error_correction(ErrorCorrection::High)
///     .style(Style::Ansi)
///     .render("https://example.com")?;
/// println!("{code}");
/// # Ok::<(), termqr::Error>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Renderer {
    error_correction: ErrorCorrection,
    style: Style,
    glyphs: Glyphs,
    margin: u8,
    scale: NonZeroU8,
}

impl Default for Renderer {
    fn default() -> Self {
        Self {
            error_correction: ErrorCorrection::default(),
            style: Style::default(),
            glyphs: Glyphs::default(),
            margin: DEFAULT_MARGIN,
            scale: NonZeroU8::MIN,
        }
    }
}

impl Renderer {
    /// A renderer with level M, [`Style::LightOnDark`], [`Glyphs::HalfBlock`], a
    /// [`DEFAULT_MARGIN`]-module quiet zone, and scale 1.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the error-correction level.
    #[must_use]
    pub fn error_correction(mut self, level: ErrorCorrection) -> Self {
        self.error_correction = level;
        self
    }

    /// Sets how modules map to colors.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the characters the code is drawn with.
    #[must_use]
    pub fn glyphs(mut self, glyphs: Glyphs) -> Self {
        self.glyphs = glyphs;
        self
    }

    /// Sets the quiet zone, in modules on each side. The spec requires 4; most scanners
    /// read 1 or 2, which saves space in small terminals. With 0 the code touches the
    /// terminal background, which scanners read as part of the code unless it is light and
    /// the style is [`Style::Ansi`] or [`Style::DarkOnLight`].
    #[must_use]
    pub fn margin(mut self, modules: u8) -> Self {
        self.margin = modules;
        self
    }

    /// Draws each module `factor` times larger, for codes that must scan from farther
    /// away: `factor` columns by `factor` half-lines with half blocks, twice as many columns
    /// by `factor` lines with ASCII. The margin scales with it.
    #[must_use]
    pub fn scale(mut self, factor: NonZeroU8) -> Self {
        self.scale = factor;
        self
    }

    /// Encodes `data` in the smallest QR version that holds it and renders it, including
    /// the quiet zone.
    ///
    /// # Errors
    ///
    /// Returns [`Error::TooLong`] if `data` does not fit in any QR version at the chosen
    /// level, and [`Error::Encode`] for any other encoder failure.
    pub fn render(&self, data: impl AsRef<[u8]>) -> Result<Rendered, Error> {
        let code = encode(data.as_ref(), self.error_correction)?;
        Ok(Rendered::new(&code, *self))
    }
}

/// Encodes `data` in the smallest QR version that holds it, using qrcode's segment
/// optimizer but never Kanji mode. The optimizer puts any byte pair that is valid Shift JIS
/// into Kanji segments, and scanners decode those as Shift JIS, which garbles UTF-8 text
/// such as Japanese.
fn encode(data: &[u8], level: ErrorCorrection) -> Result<QrCode, Error> {
    let too_long = Error::TooLong {
        len: data.len(),
        level,
    };
    // Nothing longer fits; checking first keeps huge input from being parsed and pushed
    // through all 40 versions.
    if data.len() > MAX_DATA_LEN {
        return Err(too_long);
    }
    let segments: Vec<Segment> = Parser::new(data)
        .map(|s| match s.mode {
            Mode::Kanji => Segment {
                mode: Mode::Byte,
                ..s
            },
            _ => s,
        })
        .collect();
    let ec = level.into();
    for v in 1..=40 {
        let version = Version::Normal(v);
        let mut bits = Bits::new(version);
        let pushed = bits
            .push_segments(data, Optimizer::new(segments.iter().copied(), version))
            .and_then(|()| bits.push_terminator(ec));
        match pushed {
            Ok(()) => return QrCode::with_bits(bits, ec).map_err(Error::Encode),
            Err(QrError::DataTooLong) => {}
            Err(e) => return Err(Error::Encode(e)),
        }
    }
    Err(too_long)
}

/// SGR black foreground on bright-white background, for half blocks.
const ANSI_DARK_ON_LIGHT: &str = "\x1b[30;107m";
/// SGR black and bright-white backgrounds, for ASCII.
const ANSI_BG_BLACK: &str = "\x1b[40m";
const ANSI_BG_WHITE: &str = "\x1b[107m";
const ANSI_RESET: &str = "\x1b[0m";

/// A rendered code. It holds the bordered module grid and produces text one line at a
/// time, so `Display` output of any scale needs memory for one line only. Size accessors
/// exclude ANSI escapes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Rendered {
    /// Row-major `side * side` modules, quiet zone included, `true` where the glyph is
    /// filled. The style is already applied.
    filled: Vec<bool>,
    /// Modules per side, quiet zone included.
    side: usize,
    scale: usize,
    glyphs: Glyphs,
    ansi: bool,
}

impl Rendered {
    fn new(code: &QrCode, settings: Renderer) -> Self {
        let modules = code.width();
        let margin = usize::from(settings.margin);
        let side = modules + 2 * margin;
        // Every style but LightOnDark fills the dark modules. The quiet zone is light.
        let fill_dark = settings.style != Style::LightOnDark;
        let mut filled = vec![!fill_dark; side * side];
        for (i, color) in code.to_colors().into_iter().enumerate() {
            let (y, x) = (margin + i / modules, margin + i % modules);
            filled[y * side + x] = (color == Color::Dark) == fill_dark;
        }
        Self {
            filled,
            side,
            scale: usize::from(settings.scale.get()),
            glyphs: settings.glyphs,
            ansi: settings.style == Style::Ansi,
        }
    }

    /// Width in terminal columns.
    #[must_use]
    pub fn columns(&self) -> usize {
        match self.glyphs {
            Glyphs::HalfBlock => self.side * self.scale,
            Glyphs::Ascii => 2 * self.side * self.scale,
        }
    }

    /// Height in lines.
    #[must_use]
    pub fn lines(&self) -> usize {
        match self.glyphs {
            Glyphs::HalfBlock => (self.side * self.scale).div_ceil(2),
            Glyphs::Ascii => self.side * self.scale,
        }
    }

    /// The module row drawn at scaled row `y`, or `None` past the bottom: the lower half
    /// of the last half-block line when the scaled side is odd, which shows the background.
    fn row(&self, y: usize) -> Option<&[bool]> {
        let start = y / self.scale * self.side;
        self.filled.get(start..start + self.side)
    }

    /// Appends output line `line` (without newline) to `out`.
    fn push_line(&self, line: usize, out: &mut String) {
        match self.glyphs {
            Glyphs::HalfBlock => {
                if self.ansi {
                    out.push_str(ANSI_DARK_ON_LIGHT);
                }
                let top = self.row(2 * line).expect("line is below lines()");
                let bottom = self.row(2 * line + 1);
                for (x, &upper) in top.iter().enumerate() {
                    let glyph = match (upper, bottom.is_some_and(|b| b[x])) {
                        (true, true) => '\u{2588}',
                        (true, false) => '\u{2580}',
                        (false, true) => '\u{2584}',
                        (false, false) => ' ',
                    };
                    out.extend(iter::repeat_n(glyph, self.scale));
                }
            }
            Glyphs::Ascii if self.ansi => {
                // `#` leaves gaps in its cell, so draw colored spaces, with an escape only
                // where the color changes.
                for run in self
                    .row(line)
                    .expect("line is below lines()")
                    .chunk_by(|a, b| a == b)
                {
                    out.push_str(if run[0] { ANSI_BG_BLACK } else { ANSI_BG_WHITE });
                    out.extend(iter::repeat_n(' ', 2 * run.len() * self.scale));
                }
            }
            Glyphs::Ascii => {
                for &filled in self.row(line).expect("line is below lines()") {
                    let cell = if filled { "##" } else { "  " };
                    for _ in 0..self.scale {
                        out.push_str(cell);
                    }
                }
            }
        }
        if self.ansi {
            out.push_str(ANSI_RESET);
        }
    }
}

/// Writes the lines joined with `\n`, with no trailing newline.
impl fmt::Display for Rendered {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut buf = String::new();
        for line in 0..self.lines() {
            if line > 0 {
                f.write_str("\n")?;
            }
            buf.clear();
            self.push_line(line, &mut buf);
            f.write_str(&buf)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render_at(data: impl AsRef<[u8]>, level: ErrorCorrection) -> Result<Rendered, Error> {
        Renderer::new().error_correction(level).render(data)
    }

    /// Byte-mode reference code for `data`: the smallest version that holds it, built
    /// without qrcode's segment optimizer.
    fn byte_mode_code(data: &[u8], level: EcLevel) -> QrCode {
        (1..=40)
            .find_map(|v| {
                let mut bits = Bits::new(Version::Normal(v));
                bits.push_byte_data(data).ok()?;
                bits.push_terminator(level).ok()?;
                QrCode::with_bits(bits, level).ok()
            })
            .unwrap()
    }

    /// `code` drawn by qrcode's own half-block renderer, light or dark modules filled.
    fn dense1x2(code: &QrCode, style: Style) -> String {
        use qrcode::render::unicode::Dense1x2;
        let (dark, light) = match style {
            Style::LightOnDark => (Dense1x2::Light, Dense1x2::Dark),
            _ => (Dense1x2::Dark, Dense1x2::Light),
        };
        code.render::<Dense1x2>()
            .dark_color(dark)
            .light_color(light)
            .quiet_zone(true)
            .build()
    }

    #[test]
    fn shift_jis_lookalikes_are_not_kanji_encoded() {
        // qrcode's optimizer puts byte pairs that look like Shift JIS into Kanji mode, and
        // scanners decode those as Shift JIS: the UTF-8 text came out as "...隱槭...".
        // Real Shift JIS input gets byte mode too, since its encoding is unknowable.
        let utf8 = "日本語のテキスト".as_bytes();
        let shift_jis = b"\x93\xfa\x96{\x8c\xea\x82\xcc";
        assert_ne!(
            dense1x2(&QrCode::new(utf8).unwrap(), Style::LightOnDark),
            dense1x2(&byte_mode_code(utf8, EcLevel::M), Style::LightOnDark),
            "qrcode no longer picks Kanji mode here; pick another input"
        );
        for data in [utf8, shift_jis] {
            let ours = Renderer::new().render(data).unwrap();
            let reference = byte_mode_code(data, EcLevel::M);
            assert_eq!(ours.to_string(), dense1x2(&reference, Style::LightOnDark));
        }
    }

    #[test]
    fn plain_styles_match_qrcode_dense1x2() {
        // Inputs without Shift JIS byte pairs, covering byte, numeric, and alphanumeric
        // segments, where termqr and qrcode encode identically.
        for data in [
            "hello",
            "https://example.com",
            "0123456789",
            "HTTPS://EXAMPLE.COM/A1",
        ] {
            let code = QrCode::new(data).unwrap();
            for style in [Style::LightOnDark, Style::DarkOnLight] {
                let ours = Renderer::new().style(style).render(data).unwrap();
                assert_eq!(ours.to_string(), dense1x2(&code, style), "{data} {style:?}");
            }
        }
    }

    #[test]
    fn size_accessors() {
        // "hello" is version 1: 21 modules. Columns and lines for (glyphs, margin, scale).
        for (glyphs, margin, scale, columns, lines) in [
            (Glyphs::HalfBlock, 4, 1, 29, 15),
            (Glyphs::HalfBlock, 0, 1, 21, 11),
            (Glyphs::HalfBlock, 1, 1, 23, 12),
            (Glyphs::HalfBlock, 4, 3, 87, 44),
            (Glyphs::Ascii, 4, 1, 58, 29),
            (Glyphs::Ascii, 1, 2, 92, 46),
        ] {
            let code = Renderer::new()
                .glyphs(glyphs)
                .margin(margin)
                .scale(NonZeroU8::new(scale).unwrap())
                .render("hello")
                .unwrap();
            assert_eq!((code.columns(), code.lines()), (columns, lines));
            let text = code.to_string();
            assert_eq!(text.lines().count(), lines);
            assert!(text.lines().all(|l| l.chars().count() == columns));
        }
    }

    #[test]
    fn ansi_half_blocks_wrap_dark_on_light_lines() {
        let ansi = Renderer::new().style(Style::Ansi).render("hello").unwrap();
        let plain = Renderer::new()
            .style(Style::DarkOnLight)
            .render("hello")
            .unwrap();
        assert_eq!(ansi.lines(), plain.lines());
        for (a, p) in ansi.to_string().lines().zip(plain.to_string().lines()) {
            assert_eq!(a, format!("{ANSI_DARK_ON_LIGHT}{p}{ANSI_RESET}"));
        }
    }

    #[test]
    fn ansi_ascii_colors_runs_of_spaces() {
        let code = Renderer::new()
            .style(Style::Ansi)
            .glyphs(Glyphs::Ascii)
            .scale(NonZeroU8::new(2).unwrap())
            .render("hello")
            .unwrap();
        let text = code.to_string();
        assert!(text.is_ascii() && !text.contains('#'));
        // The first line is all quiet zone: one white run, then the reset.
        let first = text.lines().next().unwrap();
        assert_eq!(
            first,
            format!("{ANSI_BG_WHITE}{}{ANSI_RESET}", " ".repeat(116))
        );
        // Colors alternate: no escape repeats the color already in effect.
        for line in text.lines() {
            let colors: Vec<&str> = line
                .split('\x1b')
                .filter_map(|s| s.split_once('m').map(|(code, _)| code))
                .filter(|&code| code != "[0")
                .collect();
            assert!(colors.windows(2).all(|w| w[0] != w[1]), "{line:?}");
        }
    }

    #[test]
    fn display_writes_one_line_at_a_time() {
        // Records the largest single write, so the test fails if `Display` ever buffers
        // the whole code.
        struct MaxChunk(usize);
        impl fmt::Write for MaxChunk {
            fn write_str(&mut self, s: &str) -> fmt::Result {
                self.0 = self.0.max(s.len());
                Ok(())
            }
        }
        let code = Renderer::new()
            .scale(NonZeroU8::new(16).unwrap())
            .style(Style::Ansi)
            .render("hello")
            .unwrap();
        let mut sink = MaxChunk(0);
        fmt::write(&mut sink, format_args!("{code}")).unwrap();
        // At most one line: escapes plus `columns` glyphs of up to 3 UTF-8 bytes each.
        let line_bytes = ANSI_DARK_ON_LIGHT.len() + 3 * code.columns() + ANSI_RESET.len();
        assert!(sink.0 <= line_bytes);
        assert!(code.to_string().len() > 100 * line_bytes);
    }

    #[test]
    fn capacity_per_level() {
        // 0x00 bytes force byte mode, so `max_bytes` is the exact limit.
        for level in ErrorCorrection::ALL {
            let max = level.max_bytes();
            assert!(render_at(vec![0u8; max], level).is_ok());
            let err = render_at(vec![0u8; max + 1], level).unwrap_err();
            assert!(
                matches!(err, Error::TooLong { len, level: l } if len == max + 1 && l == level)
            );
        }
    }

    #[test]
    fn too_long_message() {
        let err = render_at(vec![0u8; 2332], ErrorCorrection::Medium);
        assert_eq!(
            err.unwrap_err().to_string(),
            "2332 bytes do not fit in a qr code at error-correction level M"
        );
    }

    #[test]
    fn max_data_len_is_digits_at_low() {
        assert!(render_at(vec![b'7'; MAX_DATA_LEN], ErrorCorrection::Low).is_ok());
        assert!(render_at(vec![b'7'; MAX_DATA_LEN + 1], ErrorCorrection::Low).is_err());
    }

    #[test]
    fn higher_level_needs_larger_code() {
        // 20 bytes need version 2 at L (holds 32) and version 3 at H (holds 24). Width with
        // quiet zone is 25 + 4 * version.
        let data = "https://example.com/";
        assert_eq!(render_at(data, ErrorCorrection::Low).unwrap().columns(), 33);
        assert_eq!(
            render_at(data, ErrorCorrection::High).unwrap().columns(),
            37
        );
    }

    #[test]
    fn oversized_input_fails_before_encoding() {
        // Encoding parses every byte and tries all 40 versions, which took seconds and
        // hundreds of MB for tens of MiB of input. Anything over MAX_DATA_LEN cannot fit.
        // Alternating modes makes many segments; one long run would fail fast anyway.
        let data: Vec<u8> = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123a"
            .iter()
            .copied()
            .cycle()
            .take(4 << 20)
            .collect();
        let start = std::time::Instant::now();
        let err = render_at(&data, ErrorCorrection::Low).unwrap_err();
        assert!(matches!(err, Error::TooLong { len, .. } if len == data.len()));
        assert!(start.elapsed() < std::time::Duration::from_millis(500));
    }

    #[test]
    fn empty_data_is_version_1() {
        assert_eq!(Renderer::new().render("").unwrap().columns(), 29);
    }
}
