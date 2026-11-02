//! Bounded byte-preserving transformations for the session-only Coder workspace.
use base64::{Engine, engine::general_purpose};
pub const INPUT_LIMIT: usize = 128 * 1024;
pub const OUTPUT_LIMIT: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    Base64,
    Base64Url,
    Url,
    Form,
    Html,
    Hex,
}
impl Codec {
    pub const ALL: [Self; 6] = [
        Self::Base64,
        Self::Base64Url,
        Self::Url,
        Self::Form,
        Self::Html,
        Self::Hex,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Base64 => "Base64",
            Self::Base64Url => "Base64url",
            Self::Url => "URL percent",
            Self::Form => "URL form (+)",
            Self::Html => "HTML entities",
            Self::Hex => "Hex",
        }
    }
}

pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(text, "{byte:02x}");
    }
    text
}
pub fn unhex(text: &str) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut high = None;
    for byte in text.bytes().filter(|b| !b.is_ascii_whitespace()) {
        let value = digit(byte)
            .ok_or("Hex requires pairs of hexadecimal digits (whitespace is allowed)")?;
        if let Some(high) = high.take() {
            bytes.push(high * 16 + value);
        } else {
            high = Some(value);
        }
        if bytes.len() > INPUT_LIMIT {
            return Err("Input exceeds 128 KiB".into());
        }
    }
    if high.is_some() {
        return Err("Hex has an incomplete byte; use two digits per byte".into());
    }
    Ok(bytes)
}
fn digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
fn utf8(input: &[u8]) -> Result<&str, String> {
    std::str::from_utf8(input).map_err(|_| {
        "This operation requires UTF-8 text; use Base64, URL or Hex for binary data".into()
    })
}
pub fn transform(input: &[u8], codec: Codec, encode: bool) -> Result<Vec<u8>, String> {
    if input.len() > INPUT_LIMIT {
        return Err("Input exceeds 128 KiB".into());
    }
    let result = match (codec, encode) {
        (Codec::Base64 | Codec::Base64Url, true) => {
            let engine = if codec == Codec::Base64 {
                &general_purpose::STANDARD
            } else {
                &general_purpose::URL_SAFE_NO_PAD
            };
            engine.encode(input).into_bytes()
        }
        (Codec::Base64 | Codec::Base64Url, false) => {
            let compact: Vec<_> = input
                .iter()
                .copied()
                .filter(|b| !b.is_ascii_whitespace())
                .collect();
            let (padded, unpadded) = if codec == Codec::Base64 {
                (
                    &general_purpose::STANDARD,
                    &general_purpose::STANDARD_NO_PAD,
                )
            } else {
                (
                    &general_purpose::URL_SAFE,
                    &general_purpose::URL_SAFE_NO_PAD,
                )
            };
            padded
                .decode(&compact)
                .or_else(|_| unpadded.decode(&compact))
                .map_err(|e| format!("Invalid {}: {e}", codec.label()))?
        }
        (Codec::Hex, true) => hex(input).into_bytes(),
        (Codec::Hex, false) => unhex(utf8(input)?)?,
        (Codec::Url | Codec::Form, true) => {
            use std::fmt::Write;
            let mut output = String::new();
            for byte in input {
                if byte.is_ascii_alphanumeric() || b"-._~".contains(byte) {
                    output.push(*byte as char);
                } else if codec == Codec::Form && *byte == b' ' {
                    output.push('+');
                } else {
                    let _ = write!(output, "%{byte:02X}");
                }
            }
            output.into_bytes()
        }
        (Codec::Url | Codec::Form, false) => {
            let mut output = Vec::new();
            let mut at = 0;
            while at < input.len() {
                match input[at] {
                    b'%' => {
                        let a = input.get(at + 1).copied().and_then(digit);
                        let b = input.get(at + 2).copied().and_then(digit);
                        let (Some(a), Some(b)) = (a, b) else {
                            return Err(format!(
                                "Invalid percent escape at byte {at}; expected %HH"
                            ));
                        };
                        output.push(a * 16 + b);
                        at += 3;
                    }
                    b'+' if codec == Codec::Form => {
                        output.push(b' ');
                        at += 1;
                    }
                    byte => {
                        output.push(byte);
                        at += 1;
                    }
                }
            }
            output
        }
        (Codec::Html, true) => quick_xml::escape::escape(utf8(input)?)
            .into_owned()
            .into_bytes(),
        (Codec::Html, false) => quick_xml::escape::unescape(utf8(input)?)
            .map_err(|e| format!("Invalid HTML entity: {e}"))?
            .into_owned()
            .into_bytes(),
    };
    if result.len() > OUTPUT_LIMIT {
        return Err("Output exceeds 1 MiB".into());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn codecs_roundtrip_unicode_and_binary_without_loss() {
        for input in [
            "Hello, Київ <>&\"' +/✓".as_bytes(),
            &[0, 255, 128, 1][..],
            &[][..],
        ] {
            for codec in Codec::ALL {
                if codec == Codec::Html && std::str::from_utf8(input).is_err() {
                    continue;
                }
                let encoded = transform(input, codec, true).unwrap();
                assert_eq!(
                    transform(&encoded, codec, false).unwrap(),
                    input,
                    "{codec:?}"
                );
            }
        }
    }
    #[test]
    fn standards_variants_and_named_entities_are_explicit() {
        assert_eq!(transform(b"a+b%20c", Codec::Url, false).unwrap(), b"a+b c");
        assert_eq!(transform(b"a+b%2Bc", Codec::Form, false).unwrap(), b"a b+c");
        assert_eq!(
            transform(b"&copy;&nbsp;&#x1F600;&#65;", Codec::Html, false).unwrap(),
            "©\u{a0}😀A".as_bytes()
        );
        assert_eq!(
            transform(b" /?+=", Codec::Url, true).unwrap(),
            b"%20%2F%3F%2B%3D"
        );
        assert_eq!(
            transform(b" /?+=", Codec::Form, true).unwrap(),
            b"+%2F%3F%2B%3D"
        );
        for value in [b"Zg==".as_slice(), b"Zg", b" Zg==\r\n"] {
            assert_eq!(transform(value, Codec::Base64, false).unwrap(), b"f");
        }
        assert_eq!(transform(b"_w", Codec::Base64Url, false).unwrap(), [255]);
        assert_eq!(
            transform(b"00 fF\n41", Codec::Hex, false).unwrap(),
            [0, 255, 65]
        );
    }
    #[test]
    fn invalid_and_oversized_input_is_rejected() {
        for (text, codec) in [
            ("%", Codec::Url),
            ("%GG", Codec::Form),
            ("abc", Codec::Hex),
            ("xx", Codec::Hex),
            ("A===", Codec::Base64),
            ("&unknown;", Codec::Html),
        ] {
            assert!(transform(text.as_bytes(), codec, false).is_err(), "{text}");
        }
        assert!(transform(&[255], Codec::Html, true).is_err());
        assert!(transform(&vec![0; INPUT_LIMIT + 1], Codec::Base64, true).is_err());
        assert!(unhex(&"00".repeat(INPUT_LIMIT + 1)).is_err());
    }
}
