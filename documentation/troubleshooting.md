# Troubleshooting

| Symptom | Cause | Fix |
| --- | --- | --- |
| Code is twice as wide as it should be | The terminal draws ambiguous-width characters (`█▀▄`) two columns wide (iTerm2, VTE options); undetectable, so the width check undercounts | `--ascii` |
| Lines between rows | Line spacing above 1.0 | Line spacing 1.0, or `--ascii --color always` |
| Garbled characters | Locale says UTF-8 but the terminal is not | `--ascii` |
| Light theme with `--color never` does not scan | Light modules are filled by default | `--invert` |
| Raw `ESC[30;107m` on Windows (unverified) | Legacy console without VT mode | `--color never`, or Windows Terminal |
