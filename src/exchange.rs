//! Loss-aware view and exchange helpers; no JavaScript or HTML execution.
use crate::model::Event;
/// Redact query/fragment values and userinfo without modifying the captured observation.
pub(crate) fn redacted_url(target: &str) -> String {
    if let Ok(mut url) = url::Url::parse(target) {
        if !url.username().is_empty() {
            let _ = url.set_username("REDACTED");
        }
        if url.password().is_some() {
            let _ = url.set_password(Some("REDACTED"));
        }
        if url.query().is_some() {
            url.set_query(Some("REDACTED"));
        }
        if url.fragment().is_some() {
            url.set_fragment(Some("REDACTED"));
        }
        url.into()
    } else {
        // Origin-form targets and relative Location headers also carry credentials.
        let end = target.find(['?', '#']).unwrap_or(target.len());
        format!(
            "{}{}",
            &target[..end],
            if end < target.len() { "?REDACTED" } else { "" }
        )
    }
}
pub(crate) fn sensitive_header(name: &str) -> bool {
    [
        "authorization",
        "proxy-authorization",
        "cookie",
        "set-cookie",
        "x-api-key",
        "api-key",
        "x-auth-token",
        "x-access-token",
    ]
    .iter()
    .any(|sensitive| name.eq_ignore_ascii_case(sensitive))
}
pub(crate) fn redacted_header(name: &str, value: &str) -> String {
    if sensitive_header(name) {
        "[REDACTED]".into()
    } else if ["location", "content-location", "referer"]
        .iter()
        .any(|n| name.eq_ignore_ascii_case(n))
    {
        redacted_url(value)
    } else {
        value.to_owned()
    }
}
fn redacted_message(bytes: &[u8]) -> Vec<u8> {
    if bytes.is_empty() {
        return vec![];
    }
    let text = String::from_utf8_lossy(bytes);
    let head = text
        .split_once("\r\n\r\n")
        .or_else(|| text.split_once("\n\n"))
        .map_or(text.as_ref(), |(head, _)| head);
    let mut lines = head.lines();
    let mut result = String::new();
    if let Some(first) = lines.next() {
        if first.starts_with("HTTP/") {
            result.push_str(first);
        } else {
            let parts = first.split_whitespace().collect::<Vec<_>>();
            if parts.len() == 3 {
                result.push_str(&format!(
                    "{} {} {}",
                    parts[0],
                    redacted_url(parts[1]),
                    parts[2]
                ));
            } else {
                result.push_str("[Request line omitted in redacted export]");
            }
        }
        result.push_str("\r\n");
    }
    for line in lines {
        // Obsolete folded fields may continue a credential. Omit all continuations.
        if line.starts_with([' ', '\t']) {
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            // The exported body is replaced; its old framing/encoding no longer applies.
            if ["content-length", "transfer-encoding", "content-encoding"]
                .iter()
                .any(|n| name.eq_ignore_ascii_case(n))
            {
                continue;
            }
            result.push_str(&format!(
                "{name}: {}\r\n",
                redacted_header(name, value.trim())
            ));
        }
    }
    result.push_str("\r\n[Body omitted in redacted export]");
    result.into_bytes()
}
pub fn redact(event: &mut Event) {
    match event {
        Event::WebSocket {
            payload, target, ..
        } => {
            *payload = b"[REDACTED]".to_vec();
            *target = redacted_url(target);
        }
        Event::Exchange(e) => {
            e.parsed.target = redacted_url(&e.parsed.target);
            if let Some(run) = &mut e.run {
                run.payloads = vec!["[REDACTED]".into()];
            }
            for (name, value) in &mut e.parsed.headers {
                *value = redacted_header(name, &String::from_utf8_lossy(value)).into_bytes();
            }
            for bytes in [
                &mut e.original.0,
                &mut e.outgoing,
                &mut e.response,
                &mut e.response_outgoing,
            ] {
                *bytes = redacted_message(bytes);
            }
        }
        _ => {}
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

pub use crate::decoding::{auto_preview, decoded, decoded_bytes, decoded_message, needs_decoding};
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
    fn redaction_removes_credentials_from_every_exported_representation() {
        let mut event = crate::interchange::raw(b"POST https://user:password@local/path?token=query-secret HTTP/1.1\r\nHost: local\r\nAuthorization: auth-secret\r\nX-Api-Key: key-secret\r\nCookie: cookie-secret\r\nReferer: https://local/?session=referer-secret\r\nContent-Length: 11\r\n\r\nbody-secret").unwrap();
        let Event::Exchange(e) = &mut event else {
            panic!()
        };
        e.response = b"HTTP/1.1 302 Found\r\nLocation: /next?token=redirect-secret\r\nSet-Cookie: response-secret\r\nContent-Encoding: gzip\r\n\r\nresponse-body-secret".to_vec();
        e.response_outgoing = e.response.clone();
        let original = serde_json::to_string(&event).unwrap();
        redact(&mut event);
        let Event::Exchange(e) = &event else { panic!() };
        let json = serde_json::to_string(&event).unwrap();
        let har = crate::interchange::har_entry(e).to_string();
        for secret in [
            "user",
            "password",
            "query-secret",
            "auth-secret",
            "key-secret",
            "cookie-secret",
            "referer-secret",
            "body-secret",
            "redirect-secret",
            "response-secret",
            "response-body-secret",
        ] {
            assert!(!e.parsed.target.contains(secret), "{secret}");
            for bytes in [
                &e.original.0,
                &e.outgoing,
                &e.response,
                &e.response_outgoing,
            ] {
                assert!(!String::from_utf8_lossy(bytes).contains(secret), "{secret}");
            }
            assert!(!json.contains(secret) && !har.contains(secret), "{secret}");
        }
        assert!(original.contains("query-secret"));
        assert!(String::from_utf8_lossy(&e.outgoing).contains("/path?REDACTED"));
        assert!(!String::from_utf8_lossy(&e.response).contains("Content-Encoding"));
    }
    #[test]
    fn redaction_omits_folded_credentials_and_websocket_url_secrets() {
        let bytes =
            redacted_message(b"GET /?token=secret HTTP/1.1\nAuthorization: one\n two\n\npayload");
        let text = String::from_utf8(bytes).unwrap();
        assert!(
            !text.contains("secret")
                && !text.contains("one")
                && !text.contains("two")
                && !text.contains("payload")
        );
        let mut event = Event::WebSocket {
            connection: 1,
            target: "wss://local/socket?token=secret".into(),
            direction: "client".into(),
            kind: "text".into(),
            payload: b"payload".to_vec(),
            total: 7,
            truncated: false,
            closed: false,
        };
        redact(&mut event);
        let json = serde_json::to_string(&event).unwrap();
        assert!(!json.contains("secret"));
        assert!(matches!(event, Event::WebSocket { payload, .. } if payload == b"[REDACTED]"));
    }
    #[test]
    fn compressed_javascript_decodes_without_changing_captured_headers_or_bytes() {
        use std::io::Write;
        let js = "/*! local JS fixture */\nwindow.fixture = 'decoded ✓';\n";
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(js.as_bytes()).unwrap();
        let compressed = encoder.finish().unwrap();
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/javascript; charset=utf-8\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\n\r\n",
            compressed.len()
        );
        let mut wire = header.as_bytes().to_vec();
        wire.extend_from_slice(&compressed);
        let original = wire.clone();
        assert!(needs_decoding(&wire));
        assert_eq!(decoded_message(&wire).unwrap(), format!("{header}{js}"));
        assert_eq!(wire, original);
        wire.pop();
        assert!(decoded_message(&wire).is_err());
        let mut chunked =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Encoding: gzip\r\n\r\n"
                .to_vec();
        chunked.extend_from_slice(format!("{:x}\r\n", compressed.len()).as_bytes());
        chunked.extend_from_slice(&compressed);
        chunked.extend_from_slice(b"\r\n0\r\n\r\n");
        assert!(decoded_message(&chunked).unwrap().ends_with(js));
        assert!(
            decoded_message(b"HTTP/1.1 200 OK\r\nContent-Encoding: unknown\r\n\r\ndata")
                .unwrap_err()
                .contains("Unsupported")
        );
        assert!(!needs_decoding(b"HTTP/1.1 200 OK\r\n\r\nplain"));
    }
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
