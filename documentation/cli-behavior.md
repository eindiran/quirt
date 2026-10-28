# CLI behavior

Behavior of `quirt` that is not obvious from `quirt --help`.

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
