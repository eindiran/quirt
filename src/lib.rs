//! Render QR codes for display in a terminal emulator.
//!
//! Encoding is done by the [`qrcode`] crate; [`Renderer`] turns the module grid into text
//! a terminal can show.
//!
//! ```
//! let code = termqr::Renderer::new().render("https://example.com")?;
//! println!("{code}");
//! # Ok::<(), termqr::Error>(())
//! ```

use std::fmt;
use std::num::NonZeroU8;

use qrcode::types::{Color, QrError};
use qrcode::{EcLevel, QrCode};

/// Longest input, in bytes, that any QR code can hold: 7089 digits at version 40, level L.
/// Longer input always fails, so readers can stop after `MAX_DATA_LEN + 1` bytes.
pub const MAX_DATA_LEN: usize = 7089;

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

/// Which modules are drawn with filled glyphs.
///
/// Filled glyphs take the terminal's foreground color. A scanner needs dark modules on a
/// light background, so pick the variant matching the terminal's theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Polarity {
    /// Light foreground on a dark background: light modules are filled.
    #[default]
    LightOnDark,
    /// Dark foreground on a light background: dark modules are filled.
    DarkOnLight,
}

/// QR error-correction level: the share of codewords that can be damaged and still decode.
///
/// Higher levels survive more damage but hold less data, so the same input needs a larger
/// code. The byte capacity listed for each level is that of version 40, the largest code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum ErrorCorrection {
    /// About 7% recoverable; at most 2953 bytes.
    Low,
    /// About 15% recoverable; at most 2331 bytes.
    #[default]
    Medium,
    /// About 25% recoverable; at most 1663 bytes.
    Quartile,
    /// About 30% recoverable; at most 1273 bytes.
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

/// Where the code's colors come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Colors {
    /// Plain glyphs in the terminal's own colors; [`Polarity`] decides which modules are
    /// filled.
    #[default]
    Plain,
    /// ANSI escapes force black modules on a bright-white background (SGR `30;107`) on
    /// every line, so the code scans whatever the terminal theme. [`Polarity`] is ignored.
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
    /// block characters two columns wide. With [`Colors::Ansi`], modules are colored
    /// spaces.
    Ascii,
}

/// Modules of light border (quiet zone) the QR spec requires around the code.
pub const DEFAULT_MARGIN: u8 = 4;

/// Renders QR codes as terminal text. Modules stay square in the usual 1:2 character cell:
/// [`Glyphs::HalfBlock`] puts two module rows on each line, [`Glyphs::Ascii`] draws each
/// module two characters wide.
///
/// ```
/// use termqr::{Colors, ErrorCorrection, Renderer};
///
/// let code = Renderer::new()
///     .error_correction(ErrorCorrection::High)
///     .colors(Colors::Ansi)
///     .render("https://example.com")?;
/// println!("{code}");
/// # Ok::<(), termqr::Error>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Renderer {
    error_correction: ErrorCorrection,
    polarity: Polarity,
    colors: Colors,
    margin: u8,
    scale: NonZeroU8,
    glyphs: Glyphs,
}

impl Default for Renderer {
    fn default() -> Self {
        Self {
            error_correction: ErrorCorrection::default(),
            polarity: Polarity::default(),
            colors: Colors::default(),
            margin: DEFAULT_MARGIN,
            scale: NonZeroU8::MIN,
            glyphs: Glyphs::default(),
        }
    }
}

impl Renderer {
    /// A renderer with level M, [`Polarity::LightOnDark`], [`Colors::Plain`], a
    /// [`DEFAULT_MARGIN`]-module quiet zone, scale 1, and [`Glyphs::HalfBlock`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the quiet zone, in modules on each side. The spec requires 4; most scanners
    /// read 1 or 2, which saves space in small terminals. With 0 the code touches the
    /// terminal background, which scanners read as part of the code unless it is light and
    /// colors are [`Colors::Ansi`] or polarity is [`Polarity::DarkOnLight`].
    #[must_use]
    pub fn margin(mut self, modules: u8) -> Self {
        self.margin = modules;
        self
    }

    /// Sets the error-correction level.
    #[must_use]
    pub fn error_correction(mut self, level: ErrorCorrection) -> Self {
        self.error_correction = level;
        self
    }

    /// Sets which modules are filled when colors are [`Colors::Plain`].
    #[must_use]
    pub fn polarity(mut self, polarity: Polarity) -> Self {
        self.polarity = polarity;
        self
    }

    /// Draws each module `factor` columns wide and `factor` half-lines tall, for codes
    /// that must scan from farther away. The margin scales with it.
    #[must_use]
    pub fn scale(mut self, factor: NonZeroU8) -> Self {
        self.scale = factor;
        self
    }

    /// Sets the characters the code is drawn with.
    #[must_use]
    pub fn glyphs(mut self, glyphs: Glyphs) -> Self {
        self.glyphs = glyphs;
        self
    }

    /// Sets where colors come from.
    #[must_use]
    pub fn colors(mut self, colors: Colors) -> Self {
        self.colors = colors;
        self
    }

    /// Encodes `data` in the smallest QR version that holds it and renders it, including
    /// the quiet zone. Lines are joined with `\n`, with no trailing newline.
    ///
    /// # Errors
    ///
    /// Returns [`Error::TooLong`] if `data` does not fit in any QR version at the chosen
    /// level, and [`Error::Encode`] for any other encoder failure.
    pub fn render(&self, data: impl AsRef<[u8]>) -> Result<Rendered, Error> {
        let data = data.as_ref();
        let code = QrCode::with_error_correction_level(data, self.error_correction.into())
            .map_err(|e| match e {
                QrError::DataTooLong => Error::TooLong {
                    len: data.len(),
                    level: self.error_correction,
                },
                other => Error::Encode(other),
            })?;

        let modules = code.width();
        let dark = code
            .to_colors()
            .into_iter()
            .map(|c| c == Color::Dark)
            .collect();
        Ok(Rendered {
            dark,
            modules,
            settings: *self,
        })
    }
}

/// SGR black foreground on bright-white background.
const ANSI_DARK_ON_LIGHT: &str = "\x1b[30;107m";
const ANSI_RESET: &str = "\x1b[0m";
/// SGR black and bright-white backgrounds, for [`Glyphs::Ascii`] with [`Colors::Ansi`].
const ANSI_BG_BLACK: &str = "\x1b[40m";
const ANSI_BG_WHITE: &str = "\x1b[107m";

/// A rendered code. It holds only the module grid and produces text on demand, one line
/// at a time, so `Display` output of any scale needs memory for one line only. Size
/// accessors exclude ANSI escapes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Rendered {
    /// Row-major, `modules * modules`, `true` for dark.
    dark: Vec<bool>,
    modules: usize,
    settings: Renderer,
}

impl Rendered {
    /// Width in terminal columns.
    #[must_use]
    pub fn columns(&self) -> usize {
        match self.settings.glyphs {
            Glyphs::HalfBlock => self.cells(),
            Glyphs::Ascii => 2 * self.cells(),
        }
    }

    /// Height in lines.
    #[must_use]
    pub fn lines(&self) -> usize {
        match self.settings.glyphs {
            Glyphs::HalfBlock => self.cells().div_ceil(2),
            Glyphs::Ascii => self.cells(),
        }
    }

    /// Side of the bordered, scaled grid in cells. A cell is one column by half a line
    /// with half blocks, two columns by one line with ASCII.
    fn cells(&self) -> usize {
        let margin = usize::from(self.settings.margin);
        (self.modules + 2 * margin) * usize::from(self.settings.scale.get())
    }

    /// Whether cell (x, y) gets a filled glyph. Rows past the bottom (the odd half of the
    /// last half-block line) are never filled, so they show the background.
    fn filled(&self, x: usize, y: usize) -> bool {
        if y >= self.cells() {
            return false;
        }
        let scale = usize::from(self.settings.scale.get());
        let margin = usize::from(self.settings.margin);
        let (x, y) = (x / scale, y / scale);
        let modules = self.modules;
        let is_dark = x >= margin
            && y >= margin
            && x < margin + modules
            && y < margin + modules
            && self.dark[(y - margin) * modules + (x - margin)];
        match (self.settings.colors, self.settings.polarity) {
            (Colors::Ansi, _) | (Colors::Plain, Polarity::DarkOnLight) => is_dark,
            (Colors::Plain, Polarity::LightOnDark) => !is_dark,
        }
    }

    /// Appends output line `line` (without newline) to `out`.
    fn push_line(&self, line: usize, out: &mut String) {
        let cells = self.cells();
        let ansi = self.settings.colors == Colors::Ansi;
        match self.settings.glyphs {
            Glyphs::HalfBlock => {
                if ansi {
                    out.push_str(ANSI_DARK_ON_LIGHT);
                }
                for x in 0..cells {
                    out.push(
                        match (self.filled(x, 2 * line), self.filled(x, 2 * line + 1)) {
                            (true, true) => '\u{2588}',
                            (true, false) => '\u{2580}',
                            (false, true) => '\u{2584}',
                            (false, false) => ' ',
                        },
                    );
                }
                if ansi {
                    out.push_str(ANSI_RESET);
                }
            }
            Glyphs::Ascii if ansi => {
                // `#` leaves gaps in its cell, so color spaces instead; an escape is
                // emitted only where the color changes.
                let mut current = None;
                for x in 0..cells {
                    let dark = self.filled(x, line);
                    if current != Some(dark) {
                        out.push_str(if dark { ANSI_BG_BLACK } else { ANSI_BG_WHITE });
                        current = Some(dark);
                    }
                    out.push_str("  ");
                }
                out.push_str(ANSI_RESET);
            }
            Glyphs::Ascii => {
                for x in 0..cells {
                    out.push_str(if self.filled(x, line) { "##" } else { "  " });
                }
            }
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

    #[test]
    fn plain_matches_qrcode_dense1x2() {
        use qrcode::render::unicode::Dense1x2;
        for data in ["hello", "https://example.com", "0123456789"] {
            let code = QrCode::new(data).unwrap();
            for (polarity, dark, light) in [
                (Polarity::LightOnDark, Dense1x2::Light, Dense1x2::Dark),
                (Polarity::DarkOnLight, Dense1x2::Dark, Dense1x2::Light),
            ] {
                let reference = code
                    .render::<Dense1x2>()
                    .dark_color(dark)
                    .light_color(light)
                    .quiet_zone(true)
                    .build();
                let ours = Renderer::new().polarity(polarity).render(data).unwrap();
                assert_eq!(ours.to_string(), reference);
            }
        }
    }

    #[test]
    fn size_accessors() {
        // Version 1: 21 modules plus two 4-module quiet zones.
        let code = Renderer::new().render("hello").unwrap();
        assert_eq!(code.columns(), 29);
        assert_eq!(code.lines(), 15);
        assert!(code.to_string().lines().all(|l| l.chars().count() == 29));
        assert_eq!(code.to_string().lines().count(), 15);
    }

    #[test]
    fn margin_sets_quiet_zone() {
        // Version 1 is 21 modules; lines are ceil(size / 2).
        for (margin, columns, lines) in [(0, 21, 11), (1, 23, 12), (2, 25, 13), (4, 29, 15)] {
            let code = Renderer::new().margin(margin).render("hello").unwrap();
            assert_eq!((code.columns(), code.lines()), (columns, lines));
        }
        // With LightOnDark the quiet zone is filled, so margin 1 gives a filled first row.
        let code = Renderer::new().margin(1).render("hello").unwrap();
        assert!(
            code.to_string()
                .lines()
                .next()
                .unwrap()
                .starts_with('\u{2588}')
        );
    }

    #[test]
    fn scale_repeats_modules() {
        let scaled = |n| {
            Renderer::new()
                .scale(NonZeroU8::new(n).unwrap())
                .render("hello")
                .unwrap()
        };
        // 29 cells per side at scale 1; lines hold two cells.
        let three = scaled(3);
        assert_eq!((three.columns(), three.lines()), (87, 44));
        for line in three.to_string().lines() {
            let chars: Vec<char> = line.chars().collect();
            assert!(chars.chunks(3).all(|g| g.iter().all(|&c| c == g[0])));
        }
        // At scale 2 each line is exactly one module row, so no half blocks appear.
        let two = scaled(2);
        assert_eq!(two.lines(), 29);
        assert!(!two.to_string().contains(['\u{2580}', '\u{2584}']));
    }

    #[test]
    fn ascii_matches_half_block_cells() {
        let half = Renderer::new()
            .polarity(Polarity::DarkOnLight)
            .render("hello")
            .unwrap();
        let ascii = Renderer::new()
            .polarity(Polarity::DarkOnLight)
            .glyphs(Glyphs::Ascii)
            .render("hello")
            .unwrap();
        assert_eq!((ascii.columns(), ascii.lines()), (58, 29));
        assert!(ascii.to_string().is_ascii());
        let half: Vec<Vec<char>> = half
            .to_string()
            .lines()
            .map(|l| l.chars().collect())
            .collect();
        for (y, line) in ascii.to_string().lines().enumerate() {
            let line = line.as_bytes();
            for x in 0..29 {
                let glyph = half[y / 2][x];
                let filled = match glyph {
                    '\u{2588}' => true,
                    '\u{2580}' => y % 2 == 0,
                    '\u{2584}' => y % 2 == 1,
                    _ => false,
                };
                let expected: &[u8] = if filled { b"##" } else { b"  " };
                assert_eq!(&line[2 * x..2 * x + 2], expected);
            }
        }
    }

    #[test]
    fn ascii_ansi_uses_colored_spaces() {
        let code = Renderer::new()
            .glyphs(Glyphs::Ascii)
            .colors(Colors::Ansi)
            .render("hello")
            .unwrap();
        assert!(code.to_string().is_ascii());
        assert!(!code.to_string().contains('#'));
        // The first line is all quiet zone: one white run, then the reset.
        let text = code.to_string();
        let first = text.lines().next().unwrap();
        assert_eq!(
            first,
            format!("{ANSI_BG_WHITE}{}{ANSI_RESET}", " ".repeat(58))
        );
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
            .colors(Colors::Ansi)
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
    fn ansi_wraps_dark_on_light_lines() {
        let ansi = Renderer::new()
            .colors(Colors::Ansi)
            .render("hello")
            .unwrap();
        let plain = Renderer::new()
            .polarity(Polarity::DarkOnLight)
            .render("hello")
            .unwrap();
        assert_eq!(
            (ansi.columns(), ansi.lines()),
            (plain.columns(), plain.lines())
        );
        for (a, p) in ansi.to_string().lines().zip(plain.to_string().lines()) {
            assert_eq!(a, format!("{ANSI_DARK_ON_LIGHT}{p}{ANSI_RESET}"));
        }
    }

    #[test]
    fn ansi_ignores_polarity() {
        let r = Renderer::new().colors(Colors::Ansi);
        assert_eq!(
            r.polarity(Polarity::LightOnDark)
                .render("hello")
                .unwrap()
                .to_string(),
            r.polarity(Polarity::DarkOnLight)
                .render("hello")
                .unwrap()
                .to_string()
        );
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
}
