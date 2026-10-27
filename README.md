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
holds the input at that level is used.

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
echo "https://example.com" | termqr
```

| Argument | Effect |
| --- | --- |
| `[FILE]` | File to encode; stdin when omitted or `-` |
| `--polarity 0\|1` | `0` (default): light-on-dark theme; `1`: dark-on-light theme |
| `--ec-level L\|M\|Q\|H` | Error-correction level, case-insensitive; default `M` |
| `--keep-newline` | Encode the trailing `\n`/`\r\n`; stripped by default |

Input is encoded as raw bytes.

When stdout is a terminal, the CLI exits with an error if the code is wider than the
terminal, since a wrapped code cannot be scanned:

```text
Error: qr code is 185 columns wide but the terminal is 120; widen the terminal or shorten the input
```

Piped or redirected output is not checked.

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
