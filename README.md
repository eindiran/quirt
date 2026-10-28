# termqr

Renders QR codes as terminal text: a Rust library and the `termqr` CLI.

## Library

```rust
use termqr::{ErrorCorrection, Renderer, Style};

let code = Renderer::new()
    .error_correction(ErrorCorrection::Medium)
    .style(Style::Ansi)
    .render("https://example.com")?;
println!("{code}");
```

| Setting | Values | Default |
| --- | --- | --- |
| `style` | `LightOnDark` and `DarkOnLight` (plain glyphs, light or dark modules filled), `Ansi` (black on bright white, any theme) | `LightOnDark` |
| `glyphs` | `HalfBlock` (two module rows per line), `Ascii` (`##`, twice as wide) | `HalfBlock` |
| `error_correction` | `Low`, `Medium`, `Quartile`, `High`: at most 2953, 2331, 1663, 1273 bytes | `Medium` |
| `margin` | Quiet zone, modules per side | 4 |
| `scale` | Module size multiplier | 1 |

- `Rendered` holds the module grid and `Display` writes one line at a time, so memory does
  not grow with `scale`. `columns()` and `lines()` exclude ANSI escapes.
- Kanji mode is never used: scanners decode it as Shift JIS, which garbles UTF-8 CJK text.
- Input over `MAX_DATA_LEN` (7089 bytes) fails before encoding.
- The `cli` feature makes `ErrorCorrection` a `clap::ValueEnum`.

## CLI

```bash
cargo install termqr --features cli
termqr "https://example.com"
pbpaste | termqr
```

`termqr --help` lists the flags. Behavior that is not obvious from it:

- **Input:** `TEXT` (words joined with spaces, verbatim), else `--file`, else stdin, read up
  to 7092 bytes so endless input fails fast. Bytes are encoded as-is without an ECI header;
  scanners assume UTF-8, so convert other encodings first (`iconv -t UTF-8`).
- **Newlines:** file and stdin input loses one trailing `\n` or `\r\n` unless
  `--keep-newline`, or the input contains a NUL byte or starts with a UTF-16 BOM (UTF-16,
  UTF-32, binary). BOM-less UTF-16 with no ASCII cannot be detected (UTF-16BE `上` is
  `4E 0A`) and needs `--keep-newline`.
- **Color:** `--color auto` uses ANSI only when stdout is a terminal and none of
  `NO_COLOR`, `TERM=dumb`, or `--invert` applies.
- **Glyphs:** `--ascii` is automatic on a terminal whose locale (`LC_ALL`, `LC_CTYPE`,
  `LANG`) is not UTF-8, except on Windows; `--unicode` overrides it.
- **Size:** when stdout is a terminal, a code that would wrap or scroll off (one row is kept
  for the prompt) is an error naming the fewest of `--margin 1`, `--ec-level L`, `--scale 1`
  that fit; `--force` prints anyway. Redirected output is not checked; the `--margin` (32)
  and `--scale` (16) limits cap it at about 32 MB.
- **Exit status:** 0 on success, including a closed pipe; 1 for input and size errors; 2 for
  usage errors.
- `--completions SHELL` and `--man` print to stdout.

## Troubleshooting

| Symptom | Cause | Fix |
| --- | --- | --- |
| Code is twice as wide as it should be | The terminal draws ambiguous-width characters (`█▀▄`) two columns wide (iTerm2, VTE options); undetectable, so the width check undercounts | `--ascii` |
| Lines between rows | Line spacing above 1.0 | Line spacing 1.0, or `--ascii --color always` |
| Garbled characters | Locale says UTF-8 but the terminal is not | `--ascii` |
| Light theme with `--color never` does not scan | Light modules are filled by default | `--invert` |
| Raw `ESC[30;107m` on Windows (unverified) | Legacy console without VT mode | `--color never`, or Windows Terminal |

## Development

`make check` runs fmt, clippy, `cargo audit`, `cargo deny`, the tests, and pre-commit. It
needs `pre-commit`, `cargo-audit`, `cargo-deny`, and `checkmake`; the toolchain is pinned in
`rust-toolchain.toml`. The render and Unicode tests decode output with `rqrr`. `deny.toml`
allows only the licenses already in the tree.

Prior art: `qr2term`, `tui-qrcode`, `viuer`.

## License

MIT
