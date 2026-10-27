//! Render QR codes for display in a terminal emulator.
//!
//! Encoding is done by the [`qrcode`] crate; this crate turns the module grid into text a
//! terminal can show. The current renderer packs two module rows into each line with
//! Unicode half blocks, which keeps the code roughly square in the usual 1:2 character
//! cell.
//!
//! ```
//! use termqr::{ErrorCorrection, Polarity};
//!
//! let text = termqr::render(
//!     "https://example.com",
//!     Polarity::LightOnDark,
//!     ErrorCorrection::Medium,
//! )?;
//! println!("{text}");
//! # Ok::<(), termqr::Error>(())
//! ```

use qrcode::render::unicode::Dense1x2;
use qrcode::{EcLevel, QrCode};

/// Errors returned while building a QR code.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The input could not be encoded, usually because it exceeds QR capacity.
    #[error("failed to encode data as a qr code")]
    Encode(#[from] qrcode::types::QrError),
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

/// Renders `data` as a QR code built from Unicode half-block characters.
///
/// Uses the smallest QR version that holds `data` at `error_correction`. The output
/// includes the standard 4-module quiet zone, uses `\n` between lines, and has no trailing
/// newline.
///
/// # Errors
///
/// Returns [`Error::Encode`] if `data` does not fit in any QR version at
/// `error_correction`.
pub fn render(
    data: impl AsRef<[u8]>,
    polarity: Polarity,
    error_correction: ErrorCorrection,
) -> Result<String, Error> {
    let code = QrCode::with_error_correction_level(data, error_correction.into())?;
    let (dark, light) = match polarity {
        Polarity::LightOnDark => (Dense1x2::Light, Dense1x2::Dark),
        Polarity::DarkOnLight => (Dense1x2::Dark, Dense1x2::Light),
    };
    Ok(code
        .render::<Dense1x2>()
        .dark_color(dark)
        .light_color(light)
        .quiet_zone(true)
        .build())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn width(text: &str) -> usize {
        text.lines().next().unwrap().chars().count()
    }

    #[test]
    fn lines_have_equal_width() {
        let text = render("hello", Polarity::default(), ErrorCorrection::default()).unwrap();
        let widths: Vec<usize> = text.lines().map(|l| l.chars().count()).collect();
        assert!(!widths.is_empty());
        assert!(widths.iter().all(|&w| w == widths[0]));
    }

    #[test]
    fn polarity_inverts_quiet_zone() {
        // The first line is all quiet zone, so it is blank in one polarity and filled in
        // the other.
        let light = render("hello", Polarity::LightOnDark, ErrorCorrection::default()).unwrap();
        let dark = render("hello", Polarity::DarkOnLight, ErrorCorrection::default()).unwrap();
        let first = |s: &str| s.lines().next().unwrap().to_owned();
        assert!(first(&light).chars().all(|c| c == '\u{2588}'));
        assert!(first(&dark).chars().all(|c| c == ' '));
    }

    #[test]
    fn capacity_per_level() {
        // Version 40 byte-mode capacities from ISO/IEC 18004 table 7.
        for (level, max_bytes) in [
            (ErrorCorrection::Low, 2953),
            (ErrorCorrection::Medium, 2331),
            (ErrorCorrection::Quartile, 1663),
            (ErrorCorrection::High, 1273),
        ] {
            assert!(render(vec![0u8; max_bytes], Polarity::default(), level).is_ok());
            assert!(matches!(
                render(vec![0u8; max_bytes + 1], Polarity::default(), level),
                Err(Error::Encode(_))
            ));
        }
    }

    #[test]
    fn higher_level_needs_larger_code() {
        // 20 bytes need version 2 at L (holds 32) and version 3 at H (holds 24). Width with
        // quiet zone is 25 + 4 * version.
        let data = "https://example.com/";
        let low = render(data, Polarity::default(), ErrorCorrection::Low).unwrap();
        let high = render(data, Polarity::default(), ErrorCorrection::High).unwrap();
        assert_eq!(width(&low), 33);
        assert_eq!(width(&high), 37);
    }
}
