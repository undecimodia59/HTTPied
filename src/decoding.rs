//! Bounded display decoding. Captured headers and bytes are never rewritten.
use std::io::Read;
const INPUT_LIMIT: usize = 2 * 1024 * 1024;
const OUTPUT_LIMIT: usize = 256 * 1024;
const PREVIEW_LIMIT: usize = 64 * 1024;
pub struct AutoPreview {
    pub text: String,
    pub note: Option<String>,
    pub decoded: bool,
}
fn split(bytes: &[u8]) -> Result<usize, String> {
    bytes
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| "Message has no complete head".into())
}
fn values(head: &str, name: &str) -> Vec<String> {
    head.lines()
        .skip(1)
        .filter_map(|line| line.split_once(':'))
        .filter(|(key, _)| key.eq_ignore_ascii_case(name))
        .flat_map(|(_, value)| value.split(',').map(|v| v.trim().to_ascii_lowercase()))
        .collect()
}
fn preview(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(PREVIEW_LIMIT)]);
    text[..text.floor_char_boundary(PREVIEW_LIMIT.min(text.len()))].to_owned()
}
fn read_limited(reader: impl Read, limit: usize) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    reader
        .take(limit as u64 + 1)
        .read_to_end(&mut output)
        .map_err(|e| format!("Cannot decode body: {e}"))?;
    if output.len() > limit {
        return Err("Decoded body exceeds the view/export limit".into());
    }
    Ok(output)
}
fn unchunk(mut rest: &[u8], limit: usize) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    loop {
        let end = rest
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or("Truncated chunk size")?;
        let size = std::str::from_utf8(&rest[..end])
            .map_err(|_| "Invalid chunk size")?
            .split(';')
            .next()
            .unwrap_or("");
        let size = usize::from_str_radix(size.trim(), 16).map_err(|_| "Invalid chunk size")?;
        rest = &rest[end + 2..];
        if size == 0 {
            // Require the trailer section's terminating CRLF, including for an empty section.
            loop {
                let end = rest
                    .windows(2)
                    .position(|w| w == b"\r\n")
                    .ok_or("Truncated chunk trailers")?;
                if end == 0 {
                    return Ok(output);
                }
                if !rest[..end].contains(&b':') {
                    return Err("Invalid chunk trailer".into());
                }
                rest = &rest[end + 2..];
            }
        }
        if size > rest.len().saturating_sub(2) || rest.get(size..size + 2) != Some(b"\r\n") {
            return Err("Truncated chunk body".into());
        }
        if size > limit.saturating_sub(output.len()) {
            return Err("Decoded body exceeds the view/export limit".into());
        }
        output.extend_from_slice(&rest[..size]);
        rest = &rest[size + 2..];
    }
}
fn stage(body: Vec<u8>, encoding: &str, limit: usize, transfer: bool) -> Result<Vec<u8>, String> {
    let bytes = body.as_slice();
    match encoding {
        "identity" => {
            if body.len() <= limit {
                Ok(body)
            } else {
                Err("Decoded body exceeds the view/export limit".into())
            }
        }
        "chunked" if transfer => unchunk(bytes, limit),
        "gzip" | "x-gzip" => read_limited(flate2::read::MultiGzDecoder::new(bytes), limit),
        "deflate" => {
            let zlib = bytes.len() >= 2
                && bytes[0] & 15 == 8
                && bytes[0] >> 4 <= 7
                && u16::from_be_bytes([bytes[0], bytes[1]]).is_multiple_of(31);
            if zlib {
                read_limited(flate2::read::ZlibDecoder::new(bytes), limit)
            } else {
                read_limited(flate2::read::DeflateDecoder::new(bytes), limit)
            }
        }
        "br" => read_limited(brotli::Decompressor::new(bytes, 4096), limit),
        "zstd" => {
            let mut decoder = zstd::stream::read::Decoder::new(bytes).map_err(|e| e.to_string())?;
            decoder.window_log_max(23).map_err(|e| e.to_string())?;
            read_limited(decoder, limit)
        }
        _ => Err(format!(
            "Unsupported {}: {encoding}",
            if transfer {
                "Transfer-Encoding"
            } else {
                "Content-Encoding"
            }
        )),
    }
}
fn payload(bytes: &[u8], limit: usize, sniff: bool) -> Result<(Vec<u8>, bool), String> {
    if bytes.len() > INPUT_LIMIT {
        return Err("Decoder input exceeds 2 MiB".into());
    }
    let offset = split(bytes)?;
    let head = String::from_utf8_lossy(&bytes[..offset]);
    let mut body = bytes[offset + 4..].to_vec();
    let transfer = values(&head, "transfer-encoding");
    let mut content = values(&head, "content-encoding");
    if transfer.len() > 4 || content.len() > 4 {
        return Err("Too many encoding layers (maximum four per header type)".into());
    }
    let mut transformed = false;
    for encoding in transfer.iter().rev() {
        transformed |= encoding != "identity";
        body = stage(body, encoding, limit, true)?;
    }
    if sniff && content.is_empty() {
        if body.starts_with(&[0x1f, 0x8b]) {
            content.push("gzip".into());
        } else if body.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
            content.push("zstd".into());
        }
    }
    for encoding in content.iter().rev() {
        transformed |= encoding != "identity";
        body = stage(body, encoding, limit, false)?;
    }
    if body.len() > limit {
        return Err("Decoded body exceeds the view/export limit".into());
    }
    Ok((body, transformed))
}
/// Header-directed decoding for export; magic sniffing is restricted to display views.
pub fn decoded_bytes(bytes: &[u8], limit: usize) -> Result<Vec<u8>, String> {
    payload(bytes, limit.min(INPUT_LIMIT), false).map(|p| p.0)
}
fn text(bytes: &[u8]) -> Result<(String, bool), String> {
    let (body, transformed) = payload(bytes, OUTPUT_LIMIT, true)?;
    let offset = split(bytes)?;
    let head = String::from_utf8_lossy(&bytes[..offset]);
    let charset = head
        .lines()
        .skip(1)
        .filter_map(|line| line.split_once(':'))
        .filter(|(key, _)| key.eq_ignore_ascii_case("content-type"))
        .flat_map(|(_, value)| value.split(';').skip(1))
        .filter_map(|part| part.trim().split_once('='))
        .find(|(key, _)| key.trim().eq_ignore_ascii_case("charset"))
        .map(|(_, label)| label.trim().trim_matches(['"', '\'']));
    let encoding = if let Some(label) = charset {
        encoding_rs::Encoding::for_label(label.as_bytes())
            .ok_or_else(|| format!("Unsupported charset: {label}"))?
    } else if body.starts_with(&[0xff, 0xfe]) {
        encoding_rs::UTF_16LE
    } else if body.starts_with(&[0xfe, 0xff]) {
        encoding_rs::UTF_16BE
    } else {
        encoding_rs::UTF_8
    };
    let (text, _, errors) = encoding.decode(&body);
    if errors
        || text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\r' | '\n' | '\t' | '\x0c'))
    {
        return Err("Body is binary or is not readable text in its declared charset".into());
    }
    if text.len() > OUTPUT_LIMIT {
        return Err("Decoded text exceeds 256 KiB".into());
    }
    Ok((
        text.into_owned(),
        transformed || encoding != encoding_rs::UTF_8,
    ))
}
pub fn decoded(bytes: &[u8]) -> Result<String, String> {
    Ok(format!(
        "[Decoded body; original bytes preserved]\r\n\r\n{}",
        text(bytes)?.0
    ))
}
pub fn decoded_message(bytes: &[u8]) -> Result<String, String> {
    Ok(format!(
        "{}\r\n\r\n{}",
        String::from_utf8_lossy(&bytes[..split(bytes)?]),
        text(bytes)?.0
    ))
}
pub fn needs_decoding(bytes: &[u8]) -> bool {
    text(bytes).is_ok_and(|(_, transformed)| transformed)
}
pub fn auto_preview(bytes: &[u8]) -> AutoPreview {
    match text(bytes) {
        Ok((body, transformed)) => {
            let offset = split(bytes).unwrap_or(0);
            let message = format!(
                "{}\r\n\r\n{body}",
                String::from_utf8_lossy(&bytes[..offset])
            );
            AutoPreview {
                text: preview(message.as_bytes()),
                note: transformed.then(|| {
                    "Decoded body · captured headers · Raw / Hex retain encoded bytes".into()
                }),
                decoded: transformed,
            }
        }
        Err(error) => AutoPreview {
            text: preview(bytes),
            note: (!bytes.is_empty()).then(|| format!("Showing captured bytes · {error}")),
            decoded: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    fn compress(codec: &str, input: &[u8]) -> Vec<u8> {
        match codec {
            "gzip" => {
                let mut e =
                    flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
                e.write_all(input).unwrap();
                e.finish().unwrap()
            }
            "deflate" => {
                let mut e =
                    flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
                e.write_all(input).unwrap();
                e.finish().unwrap()
            }
            "raw-deflate" => {
                let mut e =
                    flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
                e.write_all(input).unwrap();
                e.finish().unwrap()
            }
            "br" => {
                let mut output = Vec::new();
                {
                    let mut e = brotli::CompressorWriter::new(&mut output, 4096, 4, 20);
                    e.write_all(input).unwrap();
                }
                output
            }
            "zstd" => zstd::stream::encode_all(input, 1).unwrap(),
            _ => unreachable!(),
        }
    }
    fn wire(headers: &str, body: &[u8]) -> Vec<u8> {
        let mut bytes = format!("HTTP/1.1 200 OK\r\n{headers}\r\n").into_bytes();
        bytes.extend(body);
        bytes
    }
    #[test]
    fn auto_decodes_all_codecs_chains_and_binary_falls_back_to_capture() {
        let js = "window.encoding = '✓';\n";
        for codec in ["gzip", "deflate", "raw-deflate", "br", "zstd"] {
            let header = if codec == "raw-deflate" {
                "deflate"
            } else {
                codec
            };
            let bytes = wire(
                &format!("Content-Encoding: {header}\r\n"),
                &compress(codec, js.as_bytes()),
            );
            let original = bytes.clone();
            let view = auto_preview(&bytes);
            assert!(view.decoded, "{codec}: {:?}", view.note);
            assert!(view.text.ends_with(js));
            assert_eq!(bytes, original);
            let binary = wire(
                &format!("Content-Encoding: {header}\r\n"),
                &compress(codec, &[0, 0xff, 1, 2]),
            );
            let view = auto_preview(&binary);
            assert!(!view.decoded);
            assert_eq!(view.text, preview(&binary));
            assert!(view.note.unwrap().contains("binary"));
        }
        let body = compress("br", &compress("gzip", js.as_bytes()));
        for headers in [
            "Content-Encoding: gzip, br\r\n",
            "Content-Encoding: gzip\r\nContent-Encoding: br\r\n",
        ] {
            assert!(auto_preview(&wire(headers, &body)).text.ends_with(js));
        }
        let gzip = compress("gzip", js.as_bytes());
        let body = format!("{:x}\r\n", gzip.len())
            .into_bytes()
            .into_iter()
            .chain(gzip.iter().copied())
            .chain(b"\r\n0\r\nX-Trailer: yes\r\n\r\n".iter().copied())
            .collect::<Vec<_>>();
        assert!(
            auto_preview(&wire(
                "Transfer-Encoding: chunked\r\nContent-Encoding: gzip\r\n",
                &body
            ))
            .text
            .ends_with(js)
        );
    }
    #[test]
    fn sniffing_and_charset_conversion_are_display_only() {
        for codec in ["gzip", "zstd"] {
            let body = compress(codec, b"readable data");
            let bytes = wire("", &body);
            assert!(auto_preview(&bytes).text.ends_with("readable data"));
            assert_eq!(decoded_bytes(&bytes, 4096).unwrap(), body);
        }
        let bytes = wire(
            "Content-Type: text/plain; charset=\"windows-1251\"\r\n",
            &[0xcf, 0xf0, 0xe8, 0xe2, 0xe5, 0xf2],
        );
        assert!(auto_preview(&bytes).text.ends_with("Привет"));
        let bytes = wire("", &[0xff, 0xfe, b'H', 0, b'i', 0]);
        assert!(auto_preview(&bytes).text.ends_with("Hi"));
        let bytes = wire("Content-Type: text/plain; charset=made-up\r\n", b"hello");
        assert_eq!(auto_preview(&bytes).text, preview(&bytes));
    }
    #[test]
    fn corrupt_truncated_unsupported_and_expanding_bodies_keep_raw_fallback() {
        for bytes in [
            wire("Content-Encoding: gzip\r\n", b"broken"),
            wire("Content-Encoding: unknown\r\n", b"hello"),
            wire("Transfer-Encoding: chunked\r\n", b"3\r\nabc\r\n0\r\n"),
            wire("Content-Encoding: gzip,gzip,gzip,gzip,gzip\r\n", b"body"),
            wire(
                "Content-Encoding: gzip\r\n",
                &compress("gzip", &vec![b'a'; OUTPUT_LIMIT + 1]),
            ),
        ] {
            let view = auto_preview(&bytes);
            assert_eq!(view.text, preview(&bytes));
            assert!(view.note.unwrap().starts_with("Showing captured bytes"));
        }
        for codec in ["gzip", "zstd", "br"] {
            let mut bytes = compress(codec, b"a moderately sized body with a proper end");
            bytes.truncate(bytes.len() / 2);
            assert!(
                decoded_message(&wire(&format!("Content-Encoding: {codec}\r\n"), &bytes)).is_err(),
                "{codec}"
            );
        }
    }
}
