//! Every style, glyph set, margin, and scale, checked cell by cell against qrcode's matrix.

mod common;

use std::num::NonZeroU8;

use qrcode::types::Color;
use qrcode::{EcLevel, QrCode};
use quirt::{Glyphs, Renderer, Style};

#[test]
fn cells_match_the_module_matrix() {
    // No Shift JIS pairs, so qrcode encodes these as quirt does.
    for data in ["hello", "https://example.com/path?q=1", "0123456789"] {
        let code = QrCode::with_error_correction_level(data, EcLevel::M).unwrap();
        let modules = code.width();
        let dark = code.to_colors();
        for style in [Style::LightOnDark, Style::DarkOnLight, Style::Ansi] {
            let fill_dark = style != Style::LightOnDark;
            for glyphs in [Glyphs::HalfBlock, Glyphs::Ascii] {
                for margin in [0, 1, 4] {
                    for scale in [1, 2, 3] {
                        let text = Renderer::new()
                            .style(style)
                            .glyphs(glyphs)
                            .margin(margin)
                            .scale(NonZeroU8::new(scale).unwrap())
                            .render(data)
                            .unwrap()
                            .to_string();
                        let case = format!("{data} {style:?} {glyphs:?} m{margin} x{scale}");
                        let (margin, scale) = (usize::from(margin), usize::from(scale));
                        let side = (modules + 2 * margin) * scale;
                        let cells = common::filled_cells(&text);
                        // Half blocks pad an odd height with an unfilled half line.
                        let height = match glyphs {
                            Glyphs::HalfBlock => side.div_ceil(2) * 2,
                            _ => side,
                        };
                        assert_eq!(cells.len(), height, "{case}");
                        for (y, row) in cells.iter().enumerate() {
                            assert_eq!(row.len(), side, "{case} row {y}");
                            for (x, &filled) in row.iter().enumerate() {
                                let (mx, my) = (x / scale, y / scale);
                                let is_dark = (margin..margin + modules).contains(&mx)
                                    && (margin..margin + modules).contains(&my)
                                    && dark[(my - margin) * modules + mx - margin] == Color::Dark;
                                let expected = y < side && is_dark == fill_dark;
                                assert_eq!(filled, expected, "{case} cell ({x}, {y})");
                            }
                        }
                        if margin > 0 {
                            assert_eq!(common::decode(&text), data.as_bytes(), "{case}");
                        }
                    }
                }
            }
        }
    }
}
