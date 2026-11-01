//! Request editor serialization shared by Replay and manual interception.
use std::io;
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

/// Text editing intentionally normalizes header line endings. Body bytes are UTF-8 editor bytes.
/// `repair_length` explicitly replaces Content-Length and strips no other framing headers.
pub fn build(destination: &str, draft: &str, repair_length: bool) -> io::Result<Vec<u8>> {
    if draft.len() > 1024 * 1024 {
        return Err(invalid("editor request exceeds 1 MiB"));
    }
    let uri: http::Uri = destination
        .parse()
        .map_err(|_| invalid("invalid destination"))?;
    if !matches!(uri.scheme_str(), Some("http" | "https")) {
        return Err(invalid("Use an http:// or https:// destination"));
    }
    let authority = uri
        .authority()
        .ok_or_else(|| invalid("destination needs a host"))?;
    if authority.as_str().contains('@') {
        return Err(invalid("destination userinfo is unsupported"));
    }
    let (head, body) = if let Some((head, body)) = draft.split_once("\r\n\r\n") {
        (head.replace("\r\n", "\n"), body)
    } else if let Some((head, body)) = draft.split_once("\n\n") {
        (head.replace("\r\n", "\n"), body)
    } else {
        return Err(invalid(
            "request needs a blank line between headers and body",
        ));
    };
    let mut lines = head.lines();
    let first = lines
        .next()
        .ok_or_else(|| invalid("missing request line"))?;
    let parts: Vec<_> = first.split_whitespace().collect();
    if parts.len() != 3 || parts[2] != "HTTP/1.1" || parts[0] == "CONNECT" {
        return Err(invalid("use METHOD /path HTTP/1.1"));
    }
    let path = if parts[1].starts_with('/') {
        parts[1].to_owned()
    } else {
        let target: http::Uri = parts[1]
            .parse()
            .map_err(|_| invalid("invalid request target"))?;
        if target.scheme_str() != uri.scheme_str() || target.authority() != Some(authority) {
            return Err(invalid("request target must match the destination"));
        }
        target
            .path_and_query()
            .map_or("/", |p| p.as_str())
            .to_owned()
    };
    let mut result = format!(
        "{} {}://{}{} HTTP/1.1\r\n",
        parts[0],
        uri.scheme_str().unwrap(),
        authority,
        path
    )
    .into_bytes();
    let mut has_host = false;
    for line in lines {
        let (name, _) = line
            .split_once(':')
            .ok_or_else(|| invalid("invalid header line"))?;
        if name.eq_ignore_ascii_case("host") {
            has_host = true;
        }
        if repair_length && name.eq_ignore_ascii_case("content-length") {
            continue;
        }
        result.extend_from_slice(line.as_bytes());
        result.extend_from_slice(b"\r\n");
    }
    if !has_host {
        result.extend_from_slice(format!("Host: {authority}\r\n").as_bytes());
    }
    if repair_length && (!body.is_empty() || ["POST", "PUT", "PATCH"].contains(&parts[0])) {
        result.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
    }
    result.extend_from_slice(b"\r\n");
    result.extend_from_slice(body.as_bytes());
    Ok(result)
}

pub fn origin_draft(bytes: &[u8]) -> io::Result<(String, String)> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| invalid("binary request cannot be losslessly loaded into the text editor"))?;
    let (first, rest) = text
        .split_once("\r\n")
        .ok_or_else(|| invalid("invalid request head"))?;
    let parts: Vec<_> = first.split_whitespace().collect();
    if parts.len() != 3 {
        return Err(invalid("invalid request line"));
    }
    let uri: http::Uri = parts[1].parse().map_err(|_| invalid("invalid target"))?;
    let authority = uri
        .authority()
        .ok_or_else(|| invalid("absolute target required"))?;
    let destination = format!("{}://{authority}", uri.scheme_str().unwrap_or("http"));
    let path = uri.path_and_query().map_or("/", |p| p.as_str());
    Ok((
        destination,
        format!("{} {path} HTTP/1.1\r\n{rest}", parts[0]),
    ))
}

/// History sends the request actually executed (including manual edits), not the pre-edit capture.
pub fn executed_request(execution: &crate::model::WireExecution) -> io::Result<Vec<u8>> {
    let target: http::Uri = execution
        .parsed
        .target
        .parse()
        .map_err(|_| invalid("invalid captured target"))?;
    let authority = target
        .authority()
        .ok_or_else(|| invalid("captured target has no authority"))?;
    let text = std::str::from_utf8(&execution.outgoing)
        .map_err(|_| invalid("binary requests cannot be loaded losslessly into the editor"))?;
    build(
        &format!("{}://{authority}", target.scheme_str().unwrap_or("http")),
        text,
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_length_repair_counts_utf8_body_bytes_and_keeps_duplicates() {
        let wire = build(
            "http://localhost:80",
            "POST /x HTTP/1.1\nX-A: one\nX-A: two\nContent-Length: 99\n\nü",
            true,
        )
        .unwrap();
        let text = String::from_utf8(wire).unwrap();
        assert!(text.contains("Content-Length: 2\r\n"));
        assert!(text.contains("X-A: one\r\nX-A: two"));
        assert!(build("http://a", "GET http://b/ HTTP/1.1\n\n", true).is_err());
    }
}
