//! Render QR codes as terminal text.
//!
//! ```
//! use termqr::{Renderer, Style};
//!
//! let code = Renderer::new().style(Style::Ansi).render("https://example.com")?;
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

/// Longest input any QR code holds, in bytes: 7089 digits at version 40-L.
pub const MAX_DATA_LEN: usize = 7089;

/// Quiet zone the QR spec requires, in modules.
pub const DEFAULT_MARGIN: u8 = 4;

/// Errors from [`Renderer::render`].
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// `len` bytes do not fit in any version at `level`.
    #[error("{len} bytes do not fit in a qr code at error-correction level {level}")]
    TooLong {
        /// Input length in bytes.
        len: usize,
        /// Requested level.
        level: ErrorCorrection,
    },
    /// Any other encoder failure.
    #[error("failed to encode data as a qr code")]
    Encode(#[source] QrError),
}

/// Error-correction level. Higher levels survive more damage but hold less. With the `cli`
/// feature it is a `clap::ValueEnum` spelled `L`, `M`, `Q`, `H`; the variant docs are the
/// CLI help.
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
    /// All levels, lowest first.
    pub const ALL: [Self; 4] = [Self::Low, Self::Medium, Self::Quartile, Self::High];

    /// Byte-mode capacity of version 40. Digits and uppercase pack denser.
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
    /// Plain glyphs, light modules filled: for dark backgrounds.
    #[default]
    LightOnDark,
    /// Plain glyphs, dark modules filled: for light backgrounds.
    DarkOnLight,
    /// ANSI black on bright white, for any background.
    Ansi,
}

/// Characters to draw with. Both keep modules square in a 1:2 character cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Glyphs {
    /// Unicode half blocks, two module rows per line.
    #[default]
    HalfBlock,
    /// `##` per module, twice as wide; colored spaces with [`Style::Ansi`].
    Ascii,
}

/// Render settings. Defaults: level M, [`Style::LightOnDark`], [`Glyphs::HalfBlock`],
/// [`DEFAULT_MARGIN`], scale 1.
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
    /// Same as `default()`.
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

    /// Sets the style.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the glyphs.
    #[must_use]
    pub fn glyphs(mut self, glyphs: Glyphs) -> Self {
        self.glyphs = glyphs;
        self
    }

    /// Quiet zone in modules per side. Most scanners read 1-2. With 0 the terminal
    /// background is the border, so it only scans on a light one with a dark-filled style.
    #[must_use]
    pub fn margin(mut self, modules: u8) -> Self {
        self.margin = modules;
        self
    }

    /// Module size multiplier; the margin scales too.
    #[must_use]
    pub fn scale(mut self, factor: NonZeroU8) -> Self {
        self.scale = factor;
        self
    }

    /// Encodes `data` in the smallest version that holds it.
    ///
    /// # Errors
    ///
    /// [`Error::TooLong`] if no version holds `data`.
    pub fn render(&self, data: impl AsRef<[u8]>) -> Result<Rendered, Error> {
        let code = encode(data.as_ref(), self.error_correction)?;
        Ok(Rendered::new(&code, *self))
    }
}

/// qrcode's encoder minus Kanji mode, which it picks for any Shift-JIS-valid byte pair.
/// Scanners decode Kanji segments as Shift JIS, garbling UTF-8 such as Japanese.
fn encode(data: &[u8], level: ErrorCorrection) -> Result<QrCode, Error> {
    let too_long = Error::TooLong {
        len: data.len(),
        level,
    };
    // Parsing huge input through 40 versions takes seconds.
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

const ANSI_DARK_ON_LIGHT: &str = "\x1b[30;107m";
const ANSI_BG_BLACK: &str = "\x1b[40m";
const ANSI_BG_WHITE: &str = "\x1b[107m";
const ANSI_RESET: &str = "\x1b[0m";

/// A rendered code. `Display` draws it one line at a time, so memory does not grow with
/// scale. Sizes exclude ANSI escapes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Rendered {
    /// Row-major, `side` x `side` modules including the quiet zone, style applied.
    filled: Vec<bool>,
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
        // The quiet zone is light.
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

    /// Width in columns.
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

    /// Module row at scaled row `y`. `None` for the half line past an odd bottom edge.
    fn row(&self, y: usize) -> Option<&[bool]> {
        let start = y / self.scale * self.side;
        self.filled.get(start..start + self.side)
    }

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
                // `#` leaves gaps, so color spaces instead, one escape per run.
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

/// Lines joined by `\n`, no trailing newline.
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

    /// Smallest pure byte-mode code for `data`.
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
        // Scanned as "...隱槭..." when encoded in Kanji mode.
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
        // No Shift JIS pairs, so qrcode encodes these as termqr does.
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
        // "hello" is 21 modules.
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
        let first = text.lines().next().unwrap();
        assert_eq!(
            first,
            format!("{ANSI_BG_WHITE}{}{ANSI_RESET}", " ".repeat(116))
        );
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
        let line_bytes = ANSI_DARK_ON_LIGHT.len() + 3 * code.columns() + ANSI_RESET.len();
        assert!(sink.0 <= line_bytes);
        assert!(code.to_string().len() > 100 * line_bytes);
    }

    #[test]
    fn capacity_per_level() {
        // NUL bytes force byte mode.
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
        // Version 2 at L, 3 at H; width is 25 + 4 * version.
        let data = "https://example.com/";
        assert_eq!(render_at(data, ErrorCorrection::Low).unwrap().columns(), 33);
        assert_eq!(
            render_at(data, ErrorCorrection::High).unwrap().columns(),
            37
        );
    }

    #[test]
    fn oversized_input_fails_before_encoding() {
        // Alternating modes make many segments; a single run fails fast regardless.
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
