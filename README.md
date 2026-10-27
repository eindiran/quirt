# termqr

Rust library for displaying QR codes in a terminal emulator.

Status: early development. Renders with Unicode half blocks or ASCII.

## Usage

```rust
use termqr::{Colors, ErrorCorrection, Renderer};

let code = Renderer::new()
    .error_correction(ErrorCorrection::Medium)
    .colors(Colors::Ansi)
    .render("https://example.com")?;
println!("{code}");
```

`render` returns a `Rendered` with `columns()` and `lines()`, which exclude ANSI escapes.
It keeps only the module grid; `Display` produces the text one line at a time, so memory
does not grow with `scale` or `margin`.

| Setting | Values | Default |
| --- | --- | --- |
| `colors` | `Plain`: terminal's own colors. `Ansi`: SGR `30;107` forces black on bright white | `Plain` |
| `polarity` | `LightOnDark`: light modules filled. `DarkOnLight`: dark modules filled. Only used with `Plain` | `LightOnDark` |
| `error_correction` | See below | `Medium` |
| `glyphs` | `HalfBlock`: Unicode half blocks. `Ascii`: `##` per module, twice as wide; colored spaces with `Ansi` | `HalfBlock` |
| `scale` | Columns and half-lines per module (`NonZeroU8`); the margin scales too | 1 |
| `margin` | Quiet zone in modules per side (`u8`). The spec requires 4; most scanners read 1-2. 0 scans only on a light background with `Ansi` or `DarkOnLight` | `DEFAULT_MARGIN` (4) |

Scanners need dark modules on a light background. With `Plain`, pick the `Polarity`
matching the terminal theme; `Ansi` works on any theme.

Data is split into numeric, alphanumeric, and byte segments, never Kanji: scanners decode
Kanji segments as Shift JIS, which garbles UTF-8 text such as Japanese.

`ErrorCorrection` trades capacity for damage tolerance. The smallest QR version that
holds the input at that level is used; input that fits no version returns
`Error::TooLong { len, level }`. `ErrorCorrection::max_bytes()` gives the limits below.

| Level | Recoverable | Max bytes (version 40) |
| --- | --- | --- |
| `Low` (L) | ~7% | 2953 |
| `Medium` (M, default) | ~15% | 2331 |
| `Quartile` (Q) | ~25% | 1663 |
| `High` (H) | ~30% | 1273 |

## CLI

The `termqr` binary is behind the `cli` feature, so library users do not pull in `clap`,
`clap_complete`, `clap_mangen`, `anyhow`, or `terminal_size`.

```bash
cargo install termqr --features cli
```

```bash
termqr "https://example.com"
```

```bash
pbpaste | termqr
```

Input comes from `TEXT` if given, else `--file`, else stdin.

Install completions and the man page, e.g. for zsh:

```bash
termqr --completions zsh > "${fpath[1]}/_termqr"
```

```bash
termqr --man > /usr/local/share/man/man1/termqr.1
```

| Argument | Effect |
| --- | --- |
| `[TEXT]...` | Text to encode, verbatim; several words are joined with spaces. Use `termqr -- -x` for text starting with `-` |
| `-f, --file FILE` | File to encode; `-` reads stdin. Conflicts with `TEXT` |
| `--invert` | Plain glyphs with dark modules filled, for light-background terminals. Turns off `--color auto`; no effect with `--color always` |
| `--color auto\|always\|never` | Force black-on-white with ANSI colors. `auto` (default) colors only a terminal stdout, and not when `NO_COLOR` is set, `TERM=dumb`, or `--invert` is given |
| `--ec-level L\|M\|Q\|H` | Error-correction level, case-insensitive; default `M` |
| `--margin N` | Quiet zone in modules per side, 1-32; default 4. 0 is refused because the terminal background is not a reliable quiet zone |
| `--scale N` | Draw each module N columns by N half-lines, 1-16; default 1 |
| `--ascii` | ASCII only: `##` per module, twice as wide. Chosen automatically on a terminal whose locale (`LC_ALL`, `LC_CTYPE`, `LANG`) is not UTF-8; never on Windows |
| `--unicode` | Half blocks even when the locale is not UTF-8 |
| `--force` | Print even if the code is too wide or tall for the terminal |
| `--completions SHELL` | Print a completion script (`bash`, `zsh`, `fish`, `elvish`, `powershell`) and exit |
| `--man` | Print the man page (roff) and exit |
| `--keep-newline` | Keep a trailing `\n`/`\r\n` from a file or stdin; stripped by default. Conflicts with `TEXT` |

Input is encoded as raw bytes. The CLI exits 1 with a message on stderr when:

| Condition | Message |
| --- | --- |
| Input is empty after newline trimming | `input is empty; nothing to encode` |
| Input exceeds 7089 bytes (`termqr::MAX_DATA_LEN`, digits at level L) | `input is longer than 7089 bytes, more than any qr code holds` |
| Input does not fit at the chosen level | `input is 2332 bytes, too long for error-correction level M (at most 2331 bytes of arbitrary data); it fits at --ec-level L` |
| stdout is a terminal narrower than the code | `qr code is 185 columns wide but the terminal is 120; widen the terminal, shorten the input, or pass --force` |
| stdout is a terminal with fewer than `lines + 1` rows (one is kept for the prompt) | `qr code is 39 lines tall but the terminal shows 23 above the prompt; enlarge the terminal, shorten the input, or pass --force` |

When a size check fails, the message names the fewest of `--margin 1`, `--ec-level L`, and
`--scale 1` that would make the code fit, for example `qr code is 58 columns wide but the
terminal is 40; --scale 1 makes it fit, or pass --force`. Only changes from the current
settings are suggested.

Input is read only up to `MAX_DATA_LEN` plus a trailing `\r\n` and one byte, so endless
input such as `yes | termqr` fails immediately. The size checks look only at stdout, so
`termqr > out.txt` from a terminal is not checked.

## Troubleshooting

| Symptom | Cause | Fix |
| --- | --- | --- |
| Code is stretched to about twice its width | The terminal draws East Asian Ambiguous characters (`█▀▄`) two columns wide, as with iTerm2's "Treat ambiguous-width characters as double width" or VTE's `ambiguous-width = wide`. Terminals do not report this, so the width check also undercounts | `--ascii` |
| Thin lines between rows | Line spacing above 1.0 separates the half blocks | Set line spacing to 1.0, or `--ascii --color always` (colored spaces) |
| Garbled characters | The locale says UTF-8 but the terminal is not | `--ascii` |
| Scans fail on a light theme with `--color never` | Light modules are filled by default | `--invert`, or drop `--color never` |

## Development

Toolchain is pinned in `rust-toolchain.toml`. Requires `pre-commit`, `cargo-audit`,
`cargo-deny`, and `checkmake`.

| Command | Action |
| --- | --- |
| `make test` | `cargo test --all-features` |
| `make lint` | clippy with `-D warnings`, then `cargo audit` |
| `make deny` | `cargo deny check` against `deny.toml` |
| `make format` | `cargo fmt` |
| `make check` | format check, lint, deny, test, `pre-commit run --all-files` |

`deny.toml` allows only the licenses currently in the dependency tree. A new dependency
under another license fails `make deny` until its license is added to `licenses.allow`.

Install hooks with `pre-commit install`.

## Prior art

| Crate | Notes |
| --- | --- |
| `qr2term` | Half-block terminal output, few options |
| `qrcode` | Encoder with a `unicode::Dense1x2` renderer; used here for encoding |
| `tui-qrcode` | Ratatui widget |
| `viuer` | Generic image display over Kitty, iTerm2, Sixel, or half blocks |

## License

MIT. See `LICENSE`.
