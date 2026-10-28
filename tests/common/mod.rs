//! Shared test helper: decodes termqr's text output back to the payload bytes with rqrr,
//! so tests check what a scanner would read rather than comparing termqr with itself.

/// Decodes termqr output in any form it takes (half blocks or ASCII, plain or ANSI,
/// either polarity, any scale or margin) and returns the payload bytes.
///
/// # Panics
///
/// Panics if no QR code decodes from `output`.
pub fn decode(output: &str) -> Vec<u8> {
    let cells = filled_cells(output);
    [false, true]
        .into_iter()
        .find_map(|filled_is_light| decode_cells(&cells, filled_is_light))
        .unwrap_or_else(|| panic!("no QR code decodes from:\n{output}"))
}

/// The drawn cells, `true` where a cell is filled: a block glyph or `##` in plain output,
/// a black background in ASCII ANSI output. Half blocks give two cell rows per line;
/// ASCII gives one cell per two columns.
pub fn filled_cells(output: &str) -> Vec<Vec<bool>> {
    let half_block = output.contains(['\u{2588}', '\u{2580}', '\u{2584}']);
    let mut rows = Vec::new();
    for line in output.lines() {
        let (mut top, mut bottom) = (Vec::new(), Vec::new());
        let mut black_background = false;
        let mut chars = line.chars();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                // SGR sequence: ESC [ params m. Only the background matters, for ASCII.
                let params: String = chars.by_ref().skip(1).take_while(|&p| p != 'm').collect();
                for p in params.split(';') {
                    match p {
                        "40" => black_background = true,
                        "107" | "0" | "" => black_background = false,
                        _ => {}
                    }
                }
            } else if half_block {
                let (t, b) = match c {
                    '\u{2588}' => (true, true),
                    '\u{2580}' => (true, false),
                    '\u{2584}' => (false, true),
                    _ => (false, false),
                };
                top.push(t);
                bottom.push(b);
            } else {
                assert_eq!(chars.next(), Some(c), "ASCII cells come in pairs: {line:?}");
                top.push(c == '#' || black_background);
            }
        }
        rows.push(top);
        if half_block {
            rows.push(bottom);
        }
    }
    rows
}

/// Draws `cells` as a greyscale image inside a light surround and decodes it.
/// `filled_is_light` selects the polarity: filled cells are dark unless it is set.
fn decode_cells(cells: &[Vec<bool>], filled_is_light: bool) -> Option<Vec<u8>> {
    // Surround in cells, and pixels per cell.
    const PAD: usize = 8;
    const PX: usize = 3;
    let width = cells.iter().map(Vec::len).max()? + 2 * PAD;
    let height = cells.len() + 2 * PAD;
    let mut image = rqrr::PreparedImage::prepare_from_greyscale(width * PX, height * PX, |x, y| {
        let filled = (y / PX)
            .checked_sub(PAD)
            .and_then(|row| cells.get(row))
            .and_then(|row| (x / PX).checked_sub(PAD).and_then(|col| row.get(col)));
        if filled.is_some_and(|&f| f != filled_is_light) {
            0
        } else {
            255
        }
    });
    let grids = image.detect_grids();
    let mut payload = Vec::new();
    grids.first()?.decode_to(&mut payload).ok()?;
    Some(payload)
}
