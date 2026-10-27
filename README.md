# termqr

Rust library for displaying QR codes in a terminal emulator.

Status: early scaffold. The only renderer is Unicode half blocks.

## Usage

```rust
use termqr::{ErrorCorrection, Polarity};

let text = termqr::render("https://example.com", Polarity::LightOnDark, ErrorCorrection::Medium)?;
println!("{text}");
```

`Polarity` picks which modules use filled glyphs. Scanners need dark modules on a light
background, so use `LightOnDark` on dark-themed terminals and `DarkOnLight` on light ones.

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
`anyhow`, or `terminal_size`.

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

| Argument | Effect |
| --- | --- |
| `[TEXT]` | Text to encode, verbatim; use `termqr -- -x` for text starting with `-` |
| `-f, --file FILE` | File to encode; `-` reads stdin. Conflicts with `TEXT` |
| `--polarity 0\|1` | `0` (default): light-on-dark theme; `1`: dark-on-light theme |
| `--ec-level L\|M\|Q\|H` | Error-correction level, case-insensitive; default `M` |
| `--keep-newline` | Keep a trailing `\n`/`\r\n` from a file or stdin; stripped by default |

Input is encoded as raw bytes. The CLI exits 1 with a message on stderr when:

| Condition | Message |
| --- | --- |
| Input is empty after newline trimming | `input is empty; nothing to encode` |
| Input exceeds 7089 bytes (`termqr::MAX_DATA_LEN`, digits at level L) | `input is longer than 7089 bytes, more than any qr code holds` |
| Input does not fit at the chosen level | `input is 2332 bytes, too long for error-correction level M (at most 2331 bytes of arbitrary data); it fits at --ec-level L` |
| stdout is a terminal narrower than the code | `qr code is 185 columns wide but the terminal is 120; widen the terminal or shorten the input` |

Input is read only up to `MAX_DATA_LEN` plus a trailing `\r\n` and one byte, so endless
input such as `yes | termqr` fails immediately. The width check looks only at stdout, so
`termqr > out.txt` from a terminal is not checked.

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
