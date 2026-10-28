# quirt

Renders QR codes as terminal text: a Rust library and the `quirt` CLI. Not yet on
crates.io; depend on or install from git.

## Library

```toml
[dependencies]
quirt = { git = "https://github.com/eindiran/quirt" }
```

```rust
use quirt::{ErrorCorrection, Renderer, Style};

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
cargo install --git https://github.com/eindiran/quirt --features cli
quirt "https://example.com"
pbpaste | quirt
```

The binary needs the `cli` feature. From a clone, `cargo install --path . --features cli`.

`quirt --help`:

```text
Print text as a QR code in the terminal.

Encodes TEXT, else --file, else stdin.

Usage: quirt [OPTIONS] [TEXT]...

Arguments:
  [TEXT]...
          Text to encode verbatim; words are joined with spaces

Options:
  -f, --file <FILE>
          File to encode; `-` is stdin

      --invert
          Plain glyphs with dark modules filled, for light backgrounds; turns off --color auto

      --ec-level <EC_LEVEL>
          Error-correction level

          Possible values:
          - L: About 7% recoverable; at most 2953 bytes
          - M: About 15% recoverable; at most 2331 bytes
          - Q: About 25% recoverable; at most 1663 bytes
          - H: About 30% recoverable; at most 1273 bytes

          [default: M]

      --color <COLOR>
          ANSI black on white; auto skips pipes, `NO_COLOR`, `TERM=dumb`, and --invert

          [default: auto]
          [possible values: auto, always, never]

      --margin <N>
          Quiet zone in modules, 1-32; most scanners read 1-2

          [default: 4]

      --scale <N>
          Module size multiplier, 1-16

          [default: 1]

      --ascii
          Draw with `##` (twice as wide); automatic on a terminal with a non-UTF-8 locale

      --unicode
          Draw with half blocks even if the locale is not UTF-8

      --force
          Print even if the code does not fit the terminal

      --keep-newline
          Keep the trailing newline of a file or stdin

      --completions <SHELL>
          Print a completion script and exit

          [possible values: bash, elvish, fish, powershell, zsh]

      --man
          Print the man page and exit

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```

See [CLI behavior](documentation/cli-behavior.md) for what `--help` does not cover (input
limits, newline trimming, color and glyph defaults, size checks, exit status) and
[Troubleshooting](documentation/troubleshooting.md) for codes that do not display or scan.

## Development

`make check` runs fmt, clippy, `cargo audit`, `cargo deny`, the tests, and pre-commit. It
needs `pre-commit`, `cargo-audit`, `cargo-deny`, and `checkmake`; the toolchain is pinned in
`rust-toolchain.toml`. The render and Unicode tests decode output with `rqrr`. `deny.toml`
allows only the licenses already in the tree.

Prior art: `qr2term`, `tui-qrcode`, `viuer`.

## License

MIT
