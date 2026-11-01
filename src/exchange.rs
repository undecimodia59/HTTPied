//! Loss-aware view and exchange helpers; no JavaScript or HTML execution.
use crate::model::Event;
pub fn redact(event: &mut Event) {
    if let Event::WebSocket { payload, .. } = event {
        *payload = b"[REDACTED]".to_vec();
    }
    if let Event::Exchange(e) = event {
        if let Some(run) = &mut e.run {
            run.payloads = vec!["[REDACTED]".into()];
        }
        for (name, value) in &mut e.parsed.headers {
            if [
                "authorization",
                "proxy-authorization",
                "cookie",
                "set-cookie",
            ]
            .iter()
            .any(|n| name.eq_ignore_ascii_case(n))
            {
                *value = b"[REDACTED]".to_vec();
            }
        }
        for bytes in [
            &mut e.original.0,
            &mut e.outgoing,
            &mut e.response,
            &mut e.response_outgoing,
        ] {
            if bytes.is_empty() {
                continue;
            }
            let text = String::from_utf8_lossy(bytes);
            let split = text.find("\r\n\r\n").unwrap_or(text.len());
            let head = &text[..split];
            let mut result = String::new();
            for line in head.split("\r\n") {
                if let Some((name, _)) = line.split_once(':')
                    && [
                        "authorization",
                        "proxy-authorization",
                        "cookie",
                        "set-cookie",
                    ]
                    .iter()
                    .any(|n| name.eq_ignore_ascii_case(n))
                {
                    result.push_str(&format!("{name}: [REDACTED]\r\n"));
                } else {
                    result.push_str(line);
                    result.push_str("\r\n");
                }
            }
            if split < text.len() {
                result.push_str("\r\n[Body omitted in redacted export]");
            }
            *bytes = result.into_bytes();
        }
    }
}
pub fn pretty(text: &str) -> String {
    let (head, body) = text
        .split_once("\r\n\r\n")
        .or_else(|| text.split_once("\n\n"))
        .unwrap_or(("", text));
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(body) {
        return format!(
            "{head}\r\n\r\n{}",
            serde_json::to_string_pretty(&value).unwrap_or_default()
        );
    }
    if body.trim_start().starts_with('<') {
        let mut reader = quick_xml::Reader::from_str(body);
        reader.config_mut().trim_text(true);
        let mut writer = quick_xml::Writer::new_with_indent(Vec::new(), b' ', 2);
        loop {
            match reader.read_event() {
                Ok(quick_xml::events::Event::Eof) => break,
                Ok(event) => {
                    if let Err(error) = writer.write_event(event) {
                        return format!("[XML formatting unavailable: {error}]\n{text}");
                    }
                }
                Err(error) => return format!("[XML formatting unavailable: {error}]\n{text}"),
            }
            if writer.get_ref().len() > 256 * 1024 {
                return "[Formatted XML exceeds 256 KiB]".into();
            }
        }
        if let Ok(body) = String::from_utf8(writer.into_inner()) {
            return format!("{head}\r\n\r\n{body}");
        }
    }
    text.to_owned()
}
/// A bounded line LCS keeps insertions from making every following line appear changed.
pub fn diff(left: &str, right: &str) -> String {
    let a = left.lines().take(2049).collect::<Vec<_>>();
    let b = right.lines().take(2049).collect::<Vec<_>>();
    if a.len() > 2048 || b.len() > 2048 {
        return "[Comparison exceeds 2,048 lines per side; narrow the selection.]".into();
    }
    let width = b.len() + 1;
    let mut lcs = vec![0u16; (a.len() + 1) * width];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            lcs[i * width + j] = if a[i] == b[j] {
                1 + lcs[(i + 1) * width + j + 1]
            } else {
                lcs[(i + 1) * width + j].max(lcs[i * width + j + 1])
            };
        }
    }
    let mut out = String::new();
    let (mut i, mut j) = (0, 0);
    while i < a.len() || j < b.len() {
        let (prefix, line) = if i < a.len() && j < b.len() && a[i] == b[j] {
            let line = a[i];
            i += 1;
            j += 1;
            ("  ", line)
        } else if j == b.len()
            || (i < a.len() && lcs[(i + 1) * width + j] >= lcs[i * width + j + 1])
        {
            let line = a[i];
            i += 1;
            ("- ", line)
        } else {
            let line = b[j];
            j += 1;
            ("+ ", line)
        };
        out.push_str(prefix);
        out.push_str(line);
        out.push('\n');
        if out.len() > 256 * 1024 {
            out.truncate(out.floor_char_boundary(256 * 1024));
            out.push_str("\n[Diff truncated]");
            break;
        }
    }
    out
}
pub fn comparison_view(text: &str, ignored: &str, hex: bool) -> String {
    let ignored = ignored.split(',').map(str::trim).collect::<Vec<_>>();
    let (head, body) = text
        .split_once("\r\n\r\n")
        .or_else(|| text.split_once("\n\n"))
        .unwrap_or(("", text));
    let head = head
        .lines()
        .filter(|line| {
            !line
                .split_once(':')
                .is_some_and(|(name, _)| ignored.iter().any(|n| name.eq_ignore_ascii_case(n)))
        })
        .collect::<Vec<_>>()
        .join("\n");
    if hex {
        use std::fmt::Write;
        let mut out = format!("{head}\n\n");
        for (row, bytes) in body.as_bytes().chunks(16).enumerate() {
            let _ = write!(out, "{:08x} ", row * 16);
            for byte in bytes {
                let _ = write!(out, "{byte:02x} ");
            }
            out.push('\n');
            if out.len() > 256 * 1024 {
                out.push_str("[Hex comparison truncated]");
                break;
            }
        }
        out
    } else {
        format!("{head}\n\n{body}")
    }
}

/// Decode captured transfer/content encodings into a separate bounded body view.
pub fn decoded(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() > 2 * 1024 * 1024 {
        return Err("Decoder input exceeds 2 MiB; use a smaller capture".into());
    }
    let output = decoded_bytes(bytes, 256 * 1024)?;
    let text = std::str::from_utf8(&output)
        .map_err(|_| "Decoded body is binary; use Hex for the original bytes")?;
    Ok(format!(
        "[Decoded body; original bytes preserved]\r\n\r\n{text}"
    ))
}
pub fn decoded_bytes(bytes: &[u8], limit: usize) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let split = bytes
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("Message has no complete head")?;
    let head = String::from_utf8_lossy(&bytes[..split]);
    let header = |name: &str| {
        head.lines()
            .skip(1)
            .filter_map(|l| l.split_once(':'))
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.trim().to_ascii_lowercase())
            .unwrap_or_default()
    };
    let mut body = bytes[split + 4..].to_vec();
    if header("transfer-encoding") == "chunked" {
        let mut output = Vec::new();
        let mut rest = body.as_slice();
        loop {
            let end = rest
                .windows(2)
                .position(|w| w == b"\r\n")
                .ok_or("Truncated chunk size")?;
            let line = std::str::from_utf8(&rest[..end])
                .map_err(|_| "Invalid chunk size")?
                .split(';')
                .next()
                .unwrap_or("");
            let size = usize::from_str_radix(line, 16).map_err(|_| "Invalid chunk size")?;
            rest = &rest[end + 2..];
            if size == 0 {
                break;
            }
            if size > rest.len().saturating_sub(2) || rest.get(size..size + 2) != Some(b"\r\n") {
                return Err("Truncated chunk body".into());
            }
            if output.len() + size > limit {
                return Err("Decoded body exceeds the view/export limit".into());
            }
            output.extend_from_slice(&rest[..size]);
            rest = &rest[size + 2..];
        }
        body = output;
    }
    let encoding = header("content-encoding");
    let mut output = Vec::new();
    match encoding.as_str() {
        "" | "identity" => output = body,
        "gzip" => {
            flate2::read::GzDecoder::new(body.as_slice())
                .take(limit as u64 + 1)
                .read_to_end(&mut output)
                .map_err(|e| e.to_string())?;
        }
        "deflate" => {
            if flate2::read::ZlibDecoder::new(body.as_slice())
                .take(limit as u64 + 1)
                .read_to_end(&mut output)
                .is_err()
            {
                output.clear();
                flate2::read::DeflateDecoder::new(body.as_slice())
                    .take(limit as u64 + 1)
                    .read_to_end(&mut output)
                    .map_err(|e| e.to_string())?;
            }
        }
        "br" => {
            brotli::Decompressor::new(body.as_slice(), 4096)
                .take(limit as u64 + 1)
                .read_to_end(&mut output)
                .map_err(|e| e.to_string())?;
        }
        _ => return Err(format!("Unsupported Content-Encoding: {encoding}")),
    }
    if output.len() > limit {
        return Err("Decoded body exceeds the view/export limit".into());
    }
    Ok(output)
}
pub fn parameters(bytes: &[u8], target: &str) -> String {
    let mut out = String::new();
    if let Ok(url) = url::Url::parse(target) {
        for (key, value) in url.query_pairs() {
            out.push_str(&format!("Query  {key} = {value}\n"));
        }
    }
    let head = String::from_utf8_lossy(bytes)
        .split("\r\n\r\n")
        .next()
        .unwrap_or("")
        .to_owned();
    for (name, value) in head.lines().skip(1).filter_map(|l| l.split_once(':')) {
        if name.eq_ignore_ascii_case("cookie") {
            for cookie in value.split(';') {
                out.push_str(&format!("Cookie  {}\n", cookie.trim()));
            }
        }
        if name.eq_ignore_ascii_case("set-cookie") {
            out.push_str(&format!("Set-Cookie  {}\n", value.trim()));
        }
    }
    if head
        .to_lowercase()
        .contains("application/x-www-form-urlencoded")
    {
        for (key, value) in url::form_urlencoded::parse(crate::interchange::body(bytes)) {
            out.push_str(&format!("Form  {key} = {value}\n"));
        }
    }
    if out.is_empty() {
        out.push_str("No query parameters, form fields or cookies in this message.");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decoding_limits_expansion_and_preserves_the_capture() {
        use std::io::Write;
        let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gzip.write_all(&vec![b'a'; 300 * 1024]).unwrap();
        let mut wire = b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\n\r\n".to_vec();
        wire.extend(gzip.finish().unwrap());
        let original = wire.clone();
        assert!(decoded(&wire).unwrap_err().contains("limit"));
        assert_eq!(wire, original);
        assert_eq!(decoded_bytes(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3;foo=bar\r\nabc\r\n0\r\n\r\n",100).unwrap(),b"abc");
    }
    #[test]
    fn comparison_tracks_insertions_and_ignores_only_selected_headers() {
        assert_eq!(
            diff("a\nb\nc\n", "a\nnew\nb\nc\n"),
            "  a\n+ new\n  b\n  c\n"
        );
        let view = comparison_view(
            "HTTP/1.1 200 OK\r\nDate: today\r\nX-Keep: yes\r\n\r\nDate: body",
            "date",
            false,
        );
        assert!(!view.contains("today"));
        assert!(view.contains("Date: body"));
        assert!(view.contains("X-Keep"));
        assert!(pretty("<root><child>value</child></root>").contains("\n  <child>"));
    }
}
