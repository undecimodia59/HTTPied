//! Importing text only constructs observations/drafts. It never runs a shell or sends traffic.
use crate::model::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
pub fn curl(destination: &str, draft: &str, redact: bool) -> Result<String, String> {
    let wire = crate::replay::build(destination, draft, true).map_err(|e| e.to_string())?;
    let event = raw(&wire)?;
    let Event::Exchange(e) = event else {
        unreachable!()
    };
    let quote = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
    let mut command = format!(
        "curl --request {} {}",
        quote(&e.parsed.method),
        quote(&if redact {
            crate::exchange::redacted_url(&e.parsed.target)
        } else {
            e.parsed.target.clone()
        })
    );
    for (name, value) in &e.parsed.headers {
        if name.eq_ignore_ascii_case("content-length") {
            continue;
        }
        let value =
            String::from_utf8(value.clone()).map_err(|_| "Header contains non-UTF-8 bytes")?;
        let value = if redact {
            crate::exchange::redacted_header(name, &value)
        } else {
            value
        };
        command.push_str(&format!(" --header {}", quote(&format!("{name}: {value}"))));
    }
    let body = body(&e.original.0);
    if !body.is_empty() {
        let text = if redact {
            "[REDACTED]"
        } else {
            std::str::from_utf8(body).map_err(|_| "Binary body cannot be copied as shell text")?
        };
        if text.contains('\0') {
            return Err("NUL bytes cannot be copied as shell text".into());
        }
        command.push_str(&format!(" --data-raw {}", quote(text)));
    }
    Ok(command)
}
pub fn import_curl(text: &str) -> Result<Event, String> {
    let tokens = shlex::split(text).ok_or("Invalid cURL quoting")?;
    let mut it = tokens.into_iter();
    if it.next().as_deref() != Some("curl") {
        return Err("Command must start with curl".into());
    }
    let mut url = String::new();
    let mut method = "GET".to_owned();
    let mut headers = Vec::new();
    let mut data = String::new();
    while let Some(token) = it.next() {
        let mut argument = || {
            it.next()
                .ok_or_else(|| format!("Missing argument for {token}"))
        };
        match token.as_str() {
            "-X" | "--request" => method = argument()?,
            "-H" | "--header" => headers.push(argument()?),
            "-d" | "--data" | "--data-raw" | "--data-binary" => {
                let value = argument()?;
                if value.starts_with('@') {
                    return Err("File expansion is not supported; paste literal data".into());
                }
                data = value;
                if method == "GET" {
                    method = "POST".into();
                }
            }
            "-b" | "--cookie" => headers.push(format!("Cookie: {}", argument()?)),
            "-A" | "--user-agent" => headers.push(format!("User-Agent: {}", argument()?)),
            "-u" | "--user" => headers.push(format!(
                "Authorization: Basic {}",
                STANDARD.encode(argument()?)
            )),
            "--url" => url = argument()?,
            "-I" | "--head" => method = "HEAD".into(),
            "--compressed" => headers.push("Accept-Encoding: gzip, deflate, br".into()),
            "-s" | "--silent" | "-S" | "--show-error" => {}
            value if value.starts_with("http://") || value.starts_with("https://") => {
                if !url.is_empty() {
                    return Err("Import one URL at a time".into());
                }
                url = value.into();
            }
            _ => return Err(format!("Unsupported cURL argument: {token}")),
        }
    }
    let url = url::Url::parse(&url).map_err(|e| e.to_string())?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err("URL credentials are unsupported; use an Authorization header".into());
    }
    let path = format!(
        "{}{}",
        url.path(),
        url.query().map(|q| format!("?{q}")).unwrap_or_default()
    );
    let destination = &url[..url::Position::BeforePath];
    let joined = headers.join("\r\n");
    let draft = format!(
        "{method} {path} HTTP/1.1\r\n{}{data}",
        if joined.is_empty() {
            "\r\n".into()
        } else {
            format!("{joined}\r\n\r\n")
        }
    );
    let wire = crate::replay::build(destination, &draft, true).map_err(|e| e.to_string())?;
    raw(&wire)
}
pub fn raw(bytes: &[u8]) -> Result<Event, String> {
    if bytes.len() > 1024 * 1024 {
        return Err("Imported request exceeds 1 MiB".into());
    }
    let mut slots = [httparse::EMPTY_HEADER; 128];
    let mut parsed = httparse::Request::new(&mut slots);
    let end = match parsed.parse(bytes).map_err(|e| e.to_string())? {
        httparse::Status::Complete(n) => n,
        _ => return Err("Incomplete request".into()),
    };
    let method = parsed.method.ok_or("Missing method")?.to_owned();
    let path = parsed.path.ok_or("Missing target")?;
    let host = parsed
        .headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case("host"))
        .and_then(|h| std::str::from_utf8(h.value).ok());
    let target = if path.starts_with('/') {
        format!(
            "http://{}{path}",
            host.ok_or("Origin-form request requires Host")?
        )
    } else {
        path.to_owned()
    };
    let uri: url::Url = target.parse().map_err(|e: url::ParseError| e.to_string())?;
    if !matches!(uri.scheme(), "http" | "https") {
        return Err("Only HTTP(S) imports are supported".into());
    }
    let headers = parsed
        .headers
        .iter()
        .map(|h| (h.name.to_owned(), h.value.to_vec()))
        .collect();
    let path = format!(
        "{}{}",
        uri.path(),
        uri.query().map(|q| format!("?{q}")).unwrap_or_default()
    );
    let first = bytes
        .windows(2)
        .position(|w| w == b"\r\n")
        .ok_or("Request requires CRLF line endings")?;
    let mut outgoing = format!("{method} {path} HTTP/1.1\r\n").into_bytes();
    outgoing.extend_from_slice(&bytes[first + 2..]);
    let _ = end;
    Ok(Event::Exchange(WireExecution {
        original: OriginalCapture(bytes.to_vec()),
        parsed: ParsedMessage {
            method,
            target,
            headers,
        },
        outgoing,
        response: vec![],
        response_outgoing: vec![],
        protocol: "HTTP/1.1".into(),
        response_total_bytes: 0,
        response_status: 0,
        response_truncated: false,
        elapsed_ms: 0.0,
        first_byte_ms: 0.0,
        held_ms: 0.0,
        response_mime: String::new(),
        transformations: vec!["Imported observation; request has not been sent".into()],
        run: None,
    }))
}
pub fn body(bytes: &[u8]) -> &[u8] {
    bytes
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|n| &bytes[n + 4..])
        .unwrap_or_default()
}
fn headers(bytes: &[u8]) -> Vec<Value> {
    String::from_utf8_lossy(bytes)
        .split("\r\n\r\n")
        .next()
        .unwrap_or("")
        .lines()
        .skip(1)
        .filter_map(|l| l.split_once(':'))
        .map(|(name, value)| json!({"name":name,"value":value.trim_start()}))
        .collect()
}
pub fn har_entry(e: &WireExecution) -> Value {
    let request_body = body(&e.outgoing);
    let (request_text, request_encoding) = match std::str::from_utf8(request_body) {
        Ok(text) => (text.to_owned(), ""),
        Err(_) => (STANDARD.encode(request_body), "base64"),
    };
    let decoded = crate::exchange::decoded_bytes(&e.response, 16 * 1024 * 1024);
    let content = match decoded {
        Ok(body) => {
            json!({"size":body.len(),"mimeType":e.response_mime,"text":STANDARD.encode(body),"encoding":"base64"})
        }
        Err(reason) => {
            json!({"size":-1,"mimeType":e.response_mime,"comment":format!("Body omitted: {reason}")})
        }
    };
    json!({"startedDateTime":"1970-01-01T00:00:00.000Z","time":e.elapsed_ms,
        "request":{"method":e.parsed.method,"url":e.parsed.target,"httpVersion":"HTTP/1.1","cookies":[],"headers":headers(&e.outgoing),"queryString":[],"headersSize":-1,"bodySize":request_body.len(),"postData":{"mimeType":"application/octet-stream","text":request_text,"_encoding":request_encoding}},
        "response":{"status":e.response_status,"statusText":"","httpVersion":"HTTP/1.1","cookies":[],"headers":headers(&e.response),"content":content,"redirectURL":"","headersSize":-1,"bodySize":body(&e.response).len()},
        "cache":{},"timings":{"send":0,"wait":e.first_byte_ms,"receive":(e.elapsed_ms-e.first_byte_ms).max(0.0)},
        "_httpied":{"transforms":e.transformations,"truncated":e.response_truncated,"timestamp":"not recorded; startedDateTime is a sentinel","binaryPostData":"_encoding is an HTTPied extension"}})
}
pub fn import_har(bytes: &[u8]) -> Result<Vec<Event>, String> {
    let value: Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let entries = value
        .pointer("/log/entries")
        .and_then(Value::as_array)
        .ok_or("HAR requires log.entries")?;
    if entries.len() > 10000 {
        return Err("HAR exceeds 10,000 entries".into());
    }
    entries
        .iter()
        .map(|entry| {
            let req = &entry["request"];
            let url = req["url"].as_str().ok_or("HAR request has no URL")?;
            let method = req["method"].as_str().ok_or("HAR request has no method")?;
            let mut draft = format!("{method} / HTTP/1.1\r\n");
            if let Some(headers) = req["headers"].as_array() {
                for h in headers {
                    let name = h["name"].as_str().ok_or("Invalid HAR header")?;
                    let value = h["value"].as_str().ok_or("Invalid HAR value")?;
                    if name.contains(['\r', '\n', ':']) || value.contains(['\r', '\n']) {
                        return Err("HAR header injection".into());
                    }
                    draft.push_str(&format!("{name}: {value}\r\n"));
                }
            }
            draft.push_str("\r\n");
            let data = req
                .pointer("/postData/text")
                .and_then(Value::as_str)
                .unwrap_or("");
            let data = if req
                .pointer("/postData/_encoding")
                .or_else(|| req.pointer("/postData/encoding"))
                .and_then(Value::as_str)
                == Some("base64")
            {
                STANDARD.decode(data).map_err(|e| e.to_string())?
            } else {
                data.as_bytes().to_vec()
            };
            let uri = url::Url::parse(url).map_err(|e| e.to_string())?;
            draft = draft.replacen(
                " / HTTP/1.1",
                &format!(
                    " {}{} HTTP/1.1",
                    uri.path(),
                    uri.query().map(|q| format!("?{q}")).unwrap_or_default()
                ),
                1,
            );
            let mut wire = crate::replay::build(url, &draft, true).map_err(|e| e.to_string())?;
            // Binary payloads are appended after constructing the editable headers.
            wire.extend_from_slice(&data);
            if !data.is_empty() {
                let end = wire.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
                let mut head =
                    String::from_utf8(wire[..end].to_vec()).map_err(|e| e.to_string())?;
                head = head
                    .lines()
                    .filter(|l| !l.to_ascii_lowercase().starts_with("content-length:"))
                    .collect::<Vec<_>>()
                    .join("\r\n");
                let mut repaired =
                    format!("{head}\r\nContent-Length: {}\r\n\r\n", data.len()).into_bytes();
                repaired.extend(data);
                wire = repaired;
            }
            let mut event = raw(&wire)?;
            if let Event::Exchange(e) = &mut event {
                let response = &entry["response"];
                let status = response["status"].as_u64().unwrap_or(0);
                if status > 599 {
                    return Err("Invalid HAR status".into());
                }
                e.response_status = status as u16;
                let mut head = format!("HTTP/1.1 {status} Imported\r\n");
                if let Some(headers) = response["headers"].as_array() {
                    for h in headers {
                        let name = h["name"].as_str().ok_or("Invalid response header")?;
                        let value = h["value"].as_str().ok_or("Invalid response value")?;
                        if name.contains(['\r', '\n', ':']) || value.contains(['\r', '\n']) {
                            return Err("HAR response header injection".into());
                        }
                        if ["transfer-encoding", "content-encoding", "content-length"]
                            .iter()
                            .any(|n| name.eq_ignore_ascii_case(n))
                        {
                            continue;
                        }
                        head.push_str(&format!("{name}: {value}\r\n"));
                    }
                }
                head.push_str("\r\n");
                e.response = head.into_bytes();
                let text = response
                    .pointer("/content/text")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let data = if response
                    .pointer("/content/encoding")
                    .and_then(Value::as_str)
                    == Some("base64")
                {
                    STANDARD.decode(text).map_err(|e| e.to_string())?
                } else {
                    text.as_bytes().to_vec()
                };
                if data.len() > 16 * 1024 * 1024 {
                    return Err("HAR response exceeds 16 MiB".into());
                }
                let end = e.response.len().saturating_sub(2);
                e.response.truncate(end);
                e.response.extend_from_slice(
                    format!("Content-Length: {}\r\n\r\n", data.len()).as_bytes(),
                );
                e.response.extend(data);
                e.response_total_bytes = e.response.len() as u64;
                e.response_mime = response
                    .pointer("/content/mimeType")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .into();
                e.elapsed_ms = entry["time"]
                    .as_f64()
                    .filter(|n| n.is_finite() && *n >= 0.0)
                    .unwrap_or(0.0);
                e.transformations.push(
                    "HAR headers/body normalized; original transport bytes unavailable".into(),
                );
            }
            Ok(event)
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn har_roundtrip_keeps_binary_payloads_and_decodes_response_framing() {
        let Event::Exchange(mut exchange)=raw(b"POST https://example.test/a HTTP/1.1\r\nHost: example.test\r\nContent-Length: 2\r\n\r\n\xff\x00").unwrap() else {panic!()};
        exchange.response =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n0\r\n\r\n".to_vec();
        exchange.response_status = 200;
        let har = json!({"log":{"entries":[har_entry(&exchange)]}});
        let imported = import_har(&serde_json::to_vec(&har).unwrap()).unwrap();
        let Event::Exchange(imported) = &imported[0] else {
            panic!()
        };
        assert_eq!(body(&imported.outgoing), b"\xff\x00");
        assert_eq!(body(&imported.response), b"abc");
        assert!(!String::from_utf8_lossy(&imported.response).contains("Transfer-Encoding"));
        assert!(String::from_utf8_lossy(&imported.response).contains("Content-Length: 3"));
    }
    #[test]
    fn curl_roundtrips_quotes_without_running_commands() {
        let Event::Exchange(e) =
            import_curl("curl 'https://example.test/a?x=1' -H 'X-A: $(literal)' --data-raw 'a b'")
                .unwrap()
        else {
            panic!()
        };
        assert_eq!(e.parsed.method, "POST");
        assert!(e.original.0.ends_with(b"a b"));
        let (_, draft) = crate::replay::origin_draft(&e.original.0).unwrap();
        let command = curl("https://example.test", &draft, false).unwrap();
        assert!(command.contains("$(literal)"));
        assert!(import_curl("curl http://x --data-binary @/etc/passwd").is_err());
    }
}
