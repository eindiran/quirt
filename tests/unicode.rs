//! Unusual Unicode in common encodings must decode byte for byte through every input path.

mod common;

use quirt::{Glyphs, Renderer};

/// Escapes mark invisible or confusable characters.
const SAMPLES: &[(&str, &str)] = &[
    ("ascii", "Hello, world! ~`|"),
    (
        "whitespace",
        "a\u{00a0}b\u{1680}c\u{2002}d\u{2003}e\u{2007}f\u{2009}g\u{200a}h\u{200b}i\u{200d}\
         j\u{2028}k\u{2029}l\u{202f}m\u{205f}n\u{3000}o\u{feff}p\tq\u{000b}r\u{000c}s\u{0085}t",
    ),
    (
        "controls",
        "\u{0000}\u{0001}\u{0007}\u{001b}[31mred\u{001b}[0m\u{007f}\r",
    ),
    ("latin", "naïve café, Ångström, ß, ÿ, ±§¶"),
    ("cjk", "日本語のテキスト 简体中文 繁體中文 한국어"),
    ("cjk_extension_b", "\u{20000}\u{2070e}\u{210c1}\u{2a6a5}"),
    ("halfwidth_katakana", "ｱｲｳｴｵ ｶﾞ"),
    ("emoji", "🎉🚀💩"),
    // ZWJ family, rainbow flag, skin tone, keycap, flag.
    (
        "emoji_sequences",
        "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467} \u{1f3f3}\u{fe0f}\u{200d}\u{1f308} \
         \u{1f44d}\u{1f3fd} 1\u{fe0f}\u{20e3} \u{1f1ef}\u{1f1f5}",
    ),
    (
        "combining",
        "Z\u{0334}\u{0321}\u{031b}\u{0353}a\u{0337}l\u{0338}g\u{0335}o e\u{0301} \u{0e01}\u{0e34}",
    ),
    ("rtl", "שלום \u{202e}abc\u{202c} مرحبا"),
    // Ends in U+0AC0: UTF-16LE ends with byte 0x0A.
    ("indic", "नमस्ते ਕਖ ગુજરાતી"),
    ("linear_a", "\u{10600}\u{10601}\u{10602}\u{10740}"),
    // Linear B, cuneiform, Egyptian hieroglyph, Old Italic, Gothic, Deseret.
    (
        "ancient",
        "\u{10000}\u{12000}\u{13000}\u{10300}\u{10330}\u{10400}",
    ),
    (
        "math_music",
        "\u{1d518}\u{1d52b}\u{1d526}\u{1d520} \u{1d11e} \u{222e} \u{2135}\u{2080}",
    ),
    // Tag character, private use, replacement, noncharacters, last scalar value.
    (
        "specials",
        "\u{e0041}\u{e000}\u{fffd}\u{fffe}\u{ffff}\u{10ffff}",
    ),
    ("segments", "Order 0123456789 ID ABC-123 日本"),
    ("mongolian_cherokee", "ᠮᠣᠩᠭᠣᠯ ᏣᎳᎩ"),
];

/// Encoding, text, and the bytes Python's codecs produce.
const LEGACY: &[(&str, &str, &[u8])] = &[
    (
        "windows-1252",
        "naïve café €5 “quotes” — dash… \u{a9}\u{ae}™",
        b"na\xefve caf\xe9 \x805 \x93quotes\x94 \x97 dash\x85 \xa9\xae\x99",
    ),
    ("iso-8859-15", "Šœ € Ÿ ž", b"\xa6\xbd \xa4 \xbe \xb8"),
    (
        "shift_jis",
        "日本語のテキスト ｱｲｳ ー",
        b"\x93\xfa\x96{\x8c\xea\x82\xcc\x83e\x83L\x83X\x83g \xb1\xb2\xb3 \x81[",
    ),
    (
        "euc-jp",
        "日本語のテキスト ｶﾅ",
        b"\xc6\xfc\xcb\xdc\xb8\xec\xa4\xce\xa5\xc6\xa5\xad\xa5\xb9\xa5\xc8 \x8e\xb6\x8e\xc5",
    ),
    // Contains ESC bytes.
    (
        "iso-2022-jp",
        "日本語 mixed ASCII",
        b"\x1b$BF|K\x5c8l\x1b(B mixed ASCII",
    ),
    (
        "gb18030",
        "简体中文 日本 \u{1f389} \u{10600}\u{10601} Ω",
        b"\xbc\xf2\xcc\xe5\xd6\xd0\xce\xc4 \xc8\xd5\xb1\xbe \x949\xbd5 \x901\x9c6\x901\x9c7 \xa6\xb8",
    ),
    (
        "gbk",
        "简体中文测试",
        b"\xbc\xf2\xcc\xe5\xd6\xd0\xce\xc4\xb2\xe2\xca\xd4",
    ),
    (
        "big5",
        "繁體中文測試",
        b"\xc1c\xc5\xe9\xa4\xa4\xa4\xe5\xb4\xfa\xb8\xd5",
    ),
    (
        "euc-kr",
        "한국어 텍스트",
        b"\xc7\xd1\xb1\xb9\xbe\xee \xc5\xd8\xbd\xba\xc6\xae",
    ),
    (
        "koi8-r",
        "Привет, мир",
        b"\xf0\xd2\xc9\xd7\xc5\xd4, \xcd\xc9\xd2",
    ),
    (
        "iso-8859-5",
        "Привет, мир",
        b"\xbf\xe0\xd8\xd2\xd5\xe2, \xdc\xd8\xe0",
    ),
    (
        "windows-1251",
        "Здравствуй, мир",
        b"\xc7\xe4\xf0\xe0\xe2\xf1\xf2\xe2\xf3\xe9, \xec\xe8\xf0",
    ),
    (
        "cp437",
        "│┌┐└┘ ░▒▓█ ß ½",
        b"\xb3\xda\xbf\xc0\xd9 \xb0\xb1\xb2\xdb \xe1 \xab",
    ),
    ("mac-roman", "café ≠ ∞ π ﬁ", b"caf\x8e \xad \xb0 \xb9 \xde"),
];

struct Payload {
    name: String,
    bytes: Vec<u8>,
}

/// `text` as UTF-8/16/32 in both byte orders, with and without BOM, and Latin-1 if it fits.
/// The flag marks ASCII-compatible encodings.
fn encodings(text: &str) -> Vec<(&'static str, bool, Vec<u8>)> {
    let utf16 = |big_endian: bool| -> Vec<u8> {
        text.encode_utf16()
            .flat_map(|u| {
                if big_endian {
                    u.to_be_bytes()
                } else {
                    u.to_le_bytes()
                }
            })
            .collect()
    };
    let utf32 = |big_endian: bool| -> Vec<u8> {
        text.chars()
            .map(u32::from)
            .flat_map(|u| {
                if big_endian {
                    u.to_be_bytes()
                } else {
                    u.to_le_bytes()
                }
            })
            .collect()
    };
    let mut out = vec![
        ("utf-8", true, text.as_bytes().to_vec()),
        (
            "utf-8-bom",
            true,
            [b"\xef\xbb\xbf", text.as_bytes()].concat(),
        ),
        ("utf-16le", false, utf16(false)),
        ("utf-16be", false, utf16(true)),
        (
            "utf-16le-bom",
            false,
            [&b"\xff\xfe"[..], &utf16(false)].concat(),
        ),
        (
            "utf-16be-bom",
            false,
            [&b"\xfe\xff"[..], &utf16(true)].concat(),
        ),
        ("utf-32le", false, utf32(false)),
        ("utf-32be", false, utf32(true)),
    ];
    let latin1: Option<Vec<u8>> = text.chars().map(|c| u8::try_from(c).ok()).collect();
    if let Some(bytes) = latin1 {
        out.push(("iso-8859-1", true, bytes));
    }
    out
}

fn payloads() -> Vec<Payload> {
    let mut out = Vec::new();
    for (sample, text) in SAMPLES {
        for (encoding, _, bytes) in encodings(text) {
            out.push(Payload {
                name: format!("{sample}/{encoding}"),
                bytes,
            });
        }
    }
    for (encoding, text, bytes) in LEGACY {
        out.push(Payload {
            name: format!("{text}/{encoding}"),
            bytes: bytes.to_vec(),
        });
    }
    out
}

#[test]
fn every_payload_round_trips_through_the_library() {
    let payloads = payloads();
    assert!(payloads.len() > 150, "matrix shrank to {}", payloads.len());
    for p in &payloads {
        for glyphs in [Glyphs::HalfBlock, Glyphs::Ascii] {
            let code = Renderer::new().glyphs(glyphs).render(&p.bytes).unwrap();
            assert_eq!(
                common::decode(&code.to_string()),
                p.bytes,
                "{} {glyphs:?}",
                p.name
            );
        }
    }
}

#[cfg(feature = "cli")]
mod cli {
    use std::io::Write;
    use std::path::PathBuf;
    use std::process::{Command, Output, Stdio};

    use super::{Payload, SAMPLES, common, encodings, payloads};

    /// Runs quirt on `bytes` written to a temp file named after `tag`.
    fn run_file(tag: &str, bytes: &[u8], flags: &[&str]) -> Output {
        let path: PathBuf =
            std::env::temp_dir().join(format!("quirt-unicode-{}-{tag}", std::process::id()));
        std::fs::write(&path, bytes).expect("temp dir is writable");
        let out = Command::new(env!("CARGO_BIN_EXE_quirt"))
            .args(flags)
            .arg("--file")
            .arg(&path)
            .output()
            .expect("quirt runs");
        let _ = std::fs::remove_file(&path); // best-effort cleanup of a temp file
        out
    }

    fn decoded(out: &Output, name: &str) -> Vec<u8> {
        assert!(
            out.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        common::decode(&String::from_utf8(out.stdout.clone()).expect("quirt writes UTF-8 or ASCII"))
    }

    /// No payload ends in a newline, so none may be trimmed (UTF-16LE `indic` ends in 0x0A).
    #[test]
    fn every_payload_round_trips_from_a_file() {
        for (i, p) in payloads().iter().enumerate() {
            let out = run_file(&format!("file-{i}"), &p.bytes, &[]);
            assert_eq!(decoded(&out, &p.name), p.bytes, "{}", p.name);
        }
    }

    #[test]
    fn every_render_mode_round_trips_each_encoding() {
        let modes: &[&[&str]] = &[
            &["--ascii"],
            &["--color", "always"],
            &["--color", "always", "--ascii"],
            &["--invert"],
            &["--scale", "2", "--margin", "1"],
            &["--ec-level", "H"],
        ];
        // One astral-plane sample in each std encoding, plus the legacy fixtures.
        let legacy = super::LEGACY.iter().map(|(encoding, text, bytes)| Payload {
            name: format!("{text}/{encoding}"),
            bytes: bytes.to_vec(),
        });
        let p: Vec<Payload> = payloads()
            .into_iter()
            .filter(|p| p.name.starts_with("emoji_sequences/"))
            .chain(legacy)
            .collect();
        for (i, p) in p.iter().enumerate() {
            for (j, flags) in modes.iter().enumerate() {
                let out = run_file(&format!("mode-{i}-{j}"), &p.bytes, flags);
                assert_eq!(decoded(&out, &p.name), p.bytes, "{} {flags:?}", p.name);
            }
        }
    }

    /// Only ASCII-compatible text loses a trailing newline. Text with NUL (`controls`) counts
    /// as binary.
    #[test]
    fn trailing_newline_is_trimmed_only_in_ascii_compatible_encodings() {
        for (sample, text) in SAMPLES {
            let plain = encodings(text);
            let with_newline = encodings(&format!("{text}\n"));
            for ((encoding, ascii_compatible, bytes), (_, _, input)) in
                plain.iter().zip(&with_newline)
            {
                let name = format!("{sample}/{encoding}");
                let expected = if *ascii_compatible && !bytes.contains(&0) {
                    bytes
                } else {
                    input
                };
                let out = run_file(&format!("nl-{name}").replace('/', "-"), input, &[]);
                assert_eq!(&decoded(&out, &name), expected, "{name}");
            }
        }
        for (encoding, text, bytes) in super::LEGACY {
            let input = [*bytes, b"\n"].concat();
            let out = run_file(&format!("nl-legacy-{encoding}"), &input, &[]);
            assert_eq!(decoded(&out, encoding), *bytes, "{text}/{encoding}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn every_nul_free_payload_round_trips_as_an_argument() {
        use std::os::unix::ffi::OsStrExt;
        for p in payloads().iter().filter(|p| !p.bytes.contains(&0)) {
            let out = Command::new(env!("CARGO_BIN_EXE_quirt"))
                .arg("--")
                .arg(std::ffi::OsStr::from_bytes(&p.bytes))
                .output()
                .unwrap();
            assert_eq!(decoded(&out, &p.name), p.bytes, "{}", p.name);
        }
    }

    #[test]
    fn payloads_round_trip_through_stdin() {
        for p in payloads().iter().filter(|p| p.name.starts_with("cjk/")) {
            let mut child = Command::new(env!("CARGO_BIN_EXE_quirt"))
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child.stdin.take().unwrap().write_all(&p.bytes).unwrap();
            let out = child.wait_with_output().unwrap();
            assert_eq!(decoded(&out, &p.name), p.bytes, "{}", p.name);
        }
    }
}
