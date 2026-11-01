//! Bounded response capture with streaming, standards-defined HTTP/1.1 body framing.
use crate::proxy::Config;
use std::{io, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    time::timeout,
};
pub struct Capture {
    pub bytes: Vec<u8>,
    pub total: u64,
    pub status: u16,
    pub truncated: bool,
    pub first_byte_ms: f64,
    pub sent: Vec<u8>,
    pub notes: Vec<String>,
    pub held_ms: f64,
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
async fn line(
    reader: &mut BufReader<&mut (dyn crate::Transport + '_)>,
    limit: usize,
    idle: Duration,
) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let buf = timeout(idle, reader.fill_buf()).await??;
        if buf.is_empty() {
            return Err(invalid("origin closed an incomplete response"));
        }
        let n = buf
            .iter()
            .position(|b| *b == b'\n')
            .map_or(buf.len(), |i| i + 1);
        if out.len() + n > limit {
            return Err(invalid(
                "response header/chunk line exceeds configured limit",
            ));
        }
        out.extend_from_slice(&buf[..n]);
        reader.consume(n);
        if out.ends_with(b"\n") {
            return Ok(out);
        }
    }
}
struct Output<'a, 'b> {
    sink: Option<&'a mut (dyn crate::Transport + 'a)>,
    bytes: Vec<u8>,
    total: u64,
    limit: usize,
    idle: Duration,
    started: &'b mut bool,
    deferred: bool,
    pending: Vec<u8>,
    notes: Vec<String>,
    held_ms: f64,
    policy: std::sync::Arc<crate::policy::Policy>,
}
impl Output<'_, '_> {
    async fn push(&mut self, bytes: &[u8]) -> io::Result<()> {
        if self.deferred {
            if self.pending.len() + bytes.len() <= 128 * 1024 {
                self.pending.extend_from_slice(bytes);
            } else {
                self.deferred = false;
                self.notes.push(
                    "Response exceeded 128 KiB; interception/rewrite skipped and original streamed"
                        .into(),
                );
                if let Some(sink) = &mut self.sink {
                    *self.started = true;
                    timeout(self.idle, sink.write_all(&self.pending)).await??;
                    timeout(self.idle, sink.write_all(bytes)).await??;
                }
                self.pending.clear();
            }
        } else if let Some(sink) = &mut self.sink {
            *self.started = true;
            timeout(self.idle, sink.write_all(bytes)).await??;
        }
        self.total = self
            .total
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| invalid("response byte count overflow"))?;
        let keep = bytes.len().min(self.limit.saturating_sub(self.bytes.len()));
        self.bytes.extend_from_slice(&bytes[..keep]);
        Ok(())
    }
    async fn finish(
        &mut self,
        cfg: &Config,
        method: &str,
        target: &str,
        status: u16,
        mime: &str,
    ) -> io::Result<Vec<u8>> {
        if !self.deferred {
            return Ok(Vec::new());
        }
        let policy = self.policy.clone();
        let original = std::mem::take(&mut self.pending);
        let (rewritten, changes) = policy
            .rewrite(target, &original, true)
            .map_err(io::Error::other)?;
        let mut sent = if changes > 0 {
            self.notes.push(format!(
                "{changes} ordered response replacements before manual editing"
            ));
            repair(&rewritten, method == "HEAD")?
        } else {
            original.clone()
        };
        if policy.response(target, method, status, mime)
            && let Some(control) = &cfg.interception
        {
            let held_at = std::time::Instant::now();
            let decision = control
                .pause_stage(target.to_owned(), target.to_owned(), sent.clone(), true)
                .await
                .map_err(io::Error::other)?;
            self.held_ms += held_at.elapsed().as_secs_f64() * 1000.0;
            match decision {
                crate::control::Decision::Forward => {}
                crate::control::Decision::Edit(bytes) => {
                    sent = repair(&bytes, method == "HEAD")?;
                    self.notes
                        .push("Response edited by operator; original retained".into());
                }
                crate::control::Decision::Drop => {
                    return Err(io::Error::other("Response dropped by operator"));
                }
            }
        }
        if let Some(sink) = &mut self.sink {
            *self.started = true;
            timeout(self.idle, sink.write_all(&sent)).await??;
        }
        Ok(if sent != original { sent } else { Vec::new() })
    }
    async fn exact(
        &mut self,
        reader: &mut BufReader<&mut (dyn crate::Transport + '_)>,
        mut length: u64,
    ) -> io::Result<()> {
        let mut buf = [0; 16 * 1024];
        while length > 0 {
            let n = length.min(buf.len() as u64) as usize;
            let read = timeout(self.idle, reader.read(&mut buf[..n])).await??;
            if read == 0 {
                return Err(invalid(
                    "origin closed before the response body was complete",
                ));
            }
            self.push(&buf[..read]).await?;
            length -= read as u64;
        }
        Ok(())
    }
}
pub async fn read<'up, 'sink, 'flag>(
    upstream: &'up mut (dyn crate::Transport + 'up),
    sink: Option<&'sink mut (dyn crate::Transport + 'sink)>,
    method: &str,
    target: &str,
    cfg: &Config,
    started: &'flag mut bool,
) -> io::Result<Capture> {
    let mut reader = BufReader::with_capacity(16 * 1024, upstream);
    let timer = std::time::Instant::now();
    let _ = timeout(cfg.idle_timeout, reader.fill_buf()).await??;
    let first_byte_ms = timer.elapsed().as_secs_f64() * 1000.0;
    let policy = cfg
        .policy
        .as_ref()
        .map(|p| p.borrow().clone())
        .unwrap_or_default();
    let mut output = Output {
        sink,
        bytes: Vec::new(),
        total: 0,
        limit: cfg.max_capture,
        idle: cfg.idle_timeout,
        started,
        deferred: policy.has_response_rules()
            || (policy.responses
                && cfg
                    .interception
                    .as_ref()
                    .is_some_and(|control| control.enabled())),
        policy,
        held_ms: 0.0,
        pending: Vec::new(),
        notes: Vec::new(),
    };
    for _ in 0..16 {
        let mut head = Vec::new();
        loop {
            let part = line(
                &mut reader,
                cfg.max_header.saturating_sub(head.len()),
                cfg.idle_timeout,
            )
            .await?;
            let end = part == b"\r\n";
            head.extend(part);
            if end {
                break;
            }
        }
        let mut slots = [httparse::EMPTY_HEADER; 128];
        let mut response = httparse::Response::new(&mut slots);
        if !response
            .parse(&head)
            .map_err(|_| invalid("invalid origin response head"))?
            .is_complete()
        {
            return Err(invalid("incomplete origin response head"));
        }
        let status = response
            .code
            .ok_or_else(|| invalid("origin response has no status"))?;
        if status == 101 {
            return Err(invalid("protocol upgrades are not supported"));
        }
        if status < 200 {
            output.push(&head).await?;
            continue;
        }
        let mut length = None;
        let mut transfer = None;
        for header in response.headers.iter() {
            if header.name.eq_ignore_ascii_case("content-length") {
                if length.is_some() {
                    return Err(invalid("duplicate response Content-Length"));
                }
                let value = std::str::from_utf8(header.value)
                    .map_err(|_| invalid("invalid response Content-Length"))?;
                if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(invalid("invalid response Content-Length"));
                }
                length = Some(
                    value
                        .parse::<u64>()
                        .map_err(|_| invalid("response Content-Length overflow"))?,
                );
            }
            if header.name.eq_ignore_ascii_case("transfer-encoding") {
                if transfer.is_some() {
                    return Err(invalid("duplicate response Transfer-Encoding"));
                }
                transfer = Some(header.value.to_vec());
            }
        }
        if transfer.is_some() && length.is_some() {
            return Err(invalid(
                "ambiguous response: both Transfer-Encoding and Content-Length",
            ));
        }
        if let Some(encoding) = &transfer
            && !encoding.eq_ignore_ascii_case(b"chunked")
        {
            return Err(invalid("unsupported response Transfer-Encoding"));
        }
        output.push(&head).await?;
        if method == "HEAD" || status == 204 || status == 304 {
        } else if transfer.is_some() {
            loop {
                let size_line = line(&mut reader, cfg.max_header, cfg.idle_timeout).await?;
                let (used, size) = match httparse::parse_chunk_size(&size_line)
                    .map_err(|_| invalid("invalid response chunk size"))?
                {
                    httparse::Status::Complete(value) => value,
                    _ => return Err(invalid("incomplete response chunk size")),
                };
                if used != size_line.len() {
                    return Err(invalid("invalid response chunk suffix"));
                }
                output.push(&size_line).await?;
                if size == 0 {
                    let mut trailers = Vec::new();
                    loop {
                        let part = line(
                            &mut reader,
                            cfg.max_header.saturating_sub(trailers.len()),
                            cfg.idle_timeout,
                        )
                        .await?;
                        let end = part == b"\r\n";
                        trailers.extend(part);
                        if end {
                            break;
                        }
                    }
                    let mut slots = [httparse::EMPTY_HEADER; 128];
                    match httparse::parse_headers(&trailers, &mut slots)
                        .map_err(|_| invalid("invalid response trailers"))?
                    {
                        httparse::Status::Complete((_, headers)) => {
                            for header in headers {
                                if ["content-length", "transfer-encoding", "host", "connection"]
                                    .iter()
                                    .any(|name| header.name.eq_ignore_ascii_case(name))
                                {
                                    return Err(invalid(
                                        "forbidden framing field in response trailers",
                                    ));
                                }
                            }
                        }
                        _ => return Err(invalid("incomplete response trailers")),
                    }
                    output.push(&trailers).await?;
                    break;
                }
                output.exact(&mut reader, size).await?;
                let mut ending = [0; 2];
                timeout(cfg.idle_timeout, reader.read_exact(&mut ending)).await??;
                if ending != *b"\r\n" {
                    return Err(invalid("missing response chunk terminator"));
                }
                output.push(&ending).await?;
            }
        } else if let Some(length) = length {
            output.exact(&mut reader, length).await?;
        } else {
            let mut buf = [0; 16 * 1024];
            loop {
                let n = timeout(cfg.idle_timeout, reader.read(&mut buf)).await??;
                if n == 0 {
                    break;
                }
                output.push(&buf[..n]).await?;
            }
        }
        let mime = response
            .headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case("content-type"))
            .map(|h| String::from_utf8_lossy(h.value).to_string())
            .unwrap_or_default();
        let sent = output.finish(cfg, method, target, status, &mime).await?;
        return Ok(Capture {
            sent,
            held_ms: output.held_ms,
            notes: output.notes,
            truncated: output.total > output.bytes.len() as u64,
            first_byte_ms,
            bytes: output.bytes,
            total: output.total,
            status,
        });
    }
    Err(invalid("too many informational responses"))
}

/// Reframe a manually edited response; encoded chunk framing must be preserved by forwarding original.
pub fn repair(bytes: &[u8], head_only: bool) -> io::Result<Vec<u8>> {
    if bytes.len() > 128 * 1024 {
        return Err(invalid("Edited response exceeds 128 KiB"));
    }
    let mut slots = [httparse::EMPTY_HEADER; 128];
    let mut response = httparse::Response::new(&mut slots);
    let end = match response
        .parse(bytes)
        .map_err(|_| invalid("Invalid edited response"))?
    {
        httparse::Status::Complete(n) => n,
        _ => return Err(invalid("Incomplete edited response")),
    };
    let status = response
        .code
        .ok_or_else(|| invalid("Missing response status"))?;
    if response
        .headers
        .iter()
        .any(|h| h.name.eq_ignore_ascii_case("transfer-encoding"))
    {
        return Err(invalid(
            "Chunked response editing is unavailable; forward original",
        ));
    }
    let body = &bytes[end..];
    if (head_only || status == 204 || status == 304 || status < 200) && !body.is_empty() {
        return Err(invalid("This response must not include a body"));
    }
    let mut out = format!("HTTP/1.1 {status} {}\r\n", response.reason.unwrap_or("")).into_bytes();
    for header in response.headers.iter() {
        if ["content-length", "connection"]
            .iter()
            .any(|n| header.name.eq_ignore_ascii_case(n))
        {
            continue;
        }
        out.extend_from_slice(header.name.as_bytes());
        out.extend_from_slice(b": ");
        out.extend_from_slice(header.value);
        out.extend_from_slice(b"\r\n");
    }
    if head_only {
        if let Some(header) = response
            .headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case("content-length"))
        {
            out.extend_from_slice(b"Content-Length: ");
            out.extend_from_slice(header.value);
            out.extend_from_slice(b"\r\n");
        }
    } else if status >= 200 && status != 204 && status != 304 {
        out.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
    }
    out.extend_from_slice(b"Connection: close\r\n\r\n");
    out.extend_from_slice(body);
    Ok(out)
}
