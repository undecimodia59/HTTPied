//! Protocol adapters expose decoded streams/messages with explicit normalization annotations.
use crate::{model::*, proxy::Config};
use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use std::{
    io,
    pin::Pin,
    sync::atomic::{AtomicU64, Ordering},
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, ReadBuf};
use tracing::Instrument;
static CONNECTION: AtomicU64 = AtomicU64::new(1);
fn error(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}
struct Prefix<'a> {
    prefix: std::io::Cursor<Vec<u8>>,
    socket: &'a mut dyn crate::Transport,
}
impl AsyncRead for Prefix<'_> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.prefix.position() < self.prefix.get_ref().len() as u64 {
            Pin::new(&mut self.prefix).poll_read(cx, buf)
        } else {
            Pin::new(&mut *self.socket).poll_read(cx, buf)
        }
    }
}
impl AsyncWrite for Prefix<'_> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut *self.socket).poll_write(cx, buf)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut *self.socket).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut *self.socket).poll_shutdown(cx)
    }
}
async fn publish(cfg: &Config, event: Event) -> io::Result<()> {
    if let Some(events) = &cfg.events {
        events.send(event).await.map_err(error)?;
    }
    Ok(())
}
fn ws_event(
    connection: u64,
    target: &str,
    direction: &str,
    message: &tokio_tungstenite::tungstenite::Message,
) -> Event {
    use tokio_tungstenite::tungstenite::Message;
    let kind = match message {
        Message::Text(_) => "Text",
        Message::Binary(_) => "Binary",
        Message::Ping(_) => "Ping",
        Message::Pong(_) => "Pong",
        Message::Close(_) => "Close",
        Message::Frame(_) => "Frame",
    };
    let data = message.clone().into_data();
    let total = data.len();
    Event::WebSocket {
        connection,
        target: target.into(),
        direction: direction.into(),
        kind: kind.into(),
        payload: data[..total.min(64 * 1024)].to_vec(),
        total,
        truncated: total > 64 * 1024,
        closed: message.is_close(),
    }
}
pub(crate) async fn websocket(
    client: &mut dyn crate::Transport,
    upstream: Box<dyn crate::Transport>,
    original: Vec<u8>,
    parsed: ParsedMessage,
    cfg: &Config,
) -> io::Result<Event> {
    use tokio_tungstenite::{
        accept_hdr_async_with_config, client_async_with_config,
        tungstenite::{
            handshake::server::{Request, Response},
            protocol::WebSocketConfig,
        },
    };
    let connection = CONNECTION.fetch_add(1, Ordering::Relaxed);
    let mut request = http::Request::builder()
        .method("GET")
        .uri(parsed.target.replacen("http", "ws", 1));
    for (name, value) in &parsed.headers {
        // This adapter does not negotiate compression; the library handles control frames.
        if name.eq_ignore_ascii_case("sec-websocket-extensions") {
            continue;
        }
        request = request.header(name, value.as_slice());
    }
    let request = request.body(()).map_err(error)?;
    let mut outgoing = format!(
        "GET {} HTTP/1.1\r\n",
        request.uri().path_and_query().map_or("/", |p| p.as_str())
    )
    .into_bytes();
    for (name, value) in request.headers() {
        outgoing.extend_from_slice(name.as_str().as_bytes());
        outgoing.extend_from_slice(b": ");
        outgoing.extend_from_slice(value.as_bytes());
        outgoing.extend_from_slice(b"\r\n");
    }
    outgoing.extend_from_slice(b"\r\n");
    let limits = WebSocketConfig::default()
        .max_message_size(Some(1024 * 1024))
        .max_frame_size(Some(256 * 1024))
        .max_write_buffer_size(2 * 1024 * 1024);
    let (mut origin, response) = match tokio::time::timeout(
        cfg.connect_timeout,
        client_async_with_config(request, upstream, Some(limits)),
    )
    .await
    .map_err(error)?
    {
        Ok(value) => value,
        Err(e) => {
            client.write_all(b"HTTP/1.1 502 WebSocket handshake failed\r\nConnection: close\r\nContent-Length: 0\r\n\r\n").await?;
            return Err(error(e));
        }
    };
    let headers = response.headers().clone();
    let mut response_bytes = b"HTTP/1.1 101 Switching Protocols\r\n".to_vec();
    for (name, value) in &headers {
        response_bytes.extend_from_slice(name.as_str().as_bytes());
        response_bytes.extend_from_slice(b": ");
        response_bytes.extend_from_slice(value.as_bytes());
        response_bytes.extend_from_slice(b"\r\n");
    }
    response_bytes.extend_from_slice(b"\r\n");
    #[allow(clippy::result_large_err)]
    // tungstenite requires this concrete handshake response error type.
    let callback = move |_request: &Request, mut response: Response| {
        for (name, value) in &headers {
            if ![
                "upgrade",
                "connection",
                "sec-websocket-accept",
                "sec-websocket-extensions",
            ]
            .contains(&name.as_str())
            {
                response.headers_mut().append(name, value.clone());
            }
        }
        Ok(response)
    };
    let prefixed = Prefix {
        prefix: std::io::Cursor::new(original.clone()),
        socket: client,
    };
    let mut downstream = tokio::time::timeout(
        cfg.connect_timeout,
        accept_hdr_async_with_config(prefixed, callback, Some(limits)),
    )
    .await
    .map_err(error)?
    .map_err(error)?;
    publish(
        cfg,
        Event::Exchange(WireExecution {
            original: OriginalCapture(original),
            parsed: parsed.clone(),
            outgoing,
            response_total_bytes: response_bytes.len() as u64,
            response: response_bytes,
            response_outgoing: vec![],
            protocol: "WebSocket handshake".into(),
            response_status: 101,
            response_truncated: false,
            elapsed_ms: 0.0,
            first_byte_ms: 0.0,
            held_ms: 0.0,
            response_mime: String::new(),
            transformations: vec![
                "WebSocket handshake headers normalized; per-message compression not negotiated"
                    .into(),
            ],
            run: None,
        }),
    )
    .await?;
    loop {
        let direction;
        let message = tokio::select! {
            message=downstream.next()=>{direction="client → server";message},
            message=origin.next()=>{direction="server → client";message},
            _=tokio::time::sleep(cfg.tunnel_idle_timeout)=>return Err(error(format!("WebSocket {} connection {connection}: idle timeout", parsed.target))),
        };
        let Some(message) = message else {
            break;
        };
        let message = message.map_err(|e| {
            error(format!(
                "WebSocket {} connection {connection}, {direction} receive: {e}",
                parsed.target
            ))
        })?;
        publish(
            cfg,
            ws_event(connection, &parsed.target, direction, &message),
        )
        .await?;
        let closed = message.is_close();
        // tungstenite automatically responds to Ping/Close. flush() sends those replies.
        if direction == "client → server" {
            tokio::time::timeout(cfg.idle_timeout, origin.send(message))
                .await
                .map_err(|e| {
                    error(format!(
                        "WebSocket {} connection {connection}, send to server timeout: {e}",
                        parsed.target
                    ))
                })?
                .map_err(|e| {
                    error(format!(
                        "WebSocket {} connection {connection}, send to server: {e}",
                        parsed.target
                    ))
                })?;
            let _ = downstream.flush().await;
        } else {
            tokio::time::timeout(cfg.idle_timeout, downstream.send(message))
                .await
                .map_err(|e| {
                    error(format!(
                        "WebSocket {} connection {connection}, send to client timeout: {e}",
                        parsed.target
                    ))
                })?
                .map_err(|e| {
                    error(format!(
                        "WebSocket {} connection {connection}, send to client: {e}",
                        parsed.target
                    ))
                })?;
            let _ = origin.flush().await;
        }
        if closed {
            break;
        }
    }
    Ok(Event::WebSocket {
        connection,
        target: parsed.target,
        direction: "connection".into(),
        kind: "Closed".into(),
        payload: vec![],
        total: 0,
        truncated: false,
        closed: true,
    })
}

pub async fn h2<T: crate::Transport>(
    io: T,
    authority: String,
    cfg: Config,
    first: Option<Box<dyn crate::Transport>>,
) -> io::Result<()> {
    tracing::debug!(%authority,preconnected=first.is_some(),"downstream HTTP/2 handshake started");
    let mut connection = h2::server::Builder::new()
        .max_concurrent_streams(16)
        .initial_window_size(64 * 1024)
        .max_header_list_size(cfg.max_header as u32)
        .handshake(io)
        .await
        .map_err(error)?;
    tracing::debug!(%authority,"downstream HTTP/2 handshake completed");
    let first = std::sync::Arc::new(tokio::sync::Mutex::new(first));
    let mut tasks = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            stream=connection.accept()=>match stream{
                Some(Ok((request,mut respond)))=>{
                    if tasks.len()>=16{tracing::warn!(%authority,"HTTP/2 concurrent stream limit reached; resetting stream");respond.send_reset(h2::Reason::REFUSED_STREAM);continue;}
                    let span=tracing::info_span!("h2_stream",stream_id=request.body().stream_id().as_u32(),method=%request.method(),uri=%request.uri(),%authority);
                    let cfg=cfg.clone();let authority=authority.clone();let first=first.clone();
                    tasks.spawn(async move{
                        let started=std::time::Instant::now();
                        tracing::debug!("HTTP/2 stream execution started");
                        let result=h2_stream(request,&mut respond,&authority,&cfg,first).await;
                        if let Err(e)=result{tracing::warn!(elapsed_ms=started.elapsed().as_millis(),error_kind=?e.kind(),error_debug=?e,error=%e,"HTTP/2 stream failed; sending INTERNAL_ERROR reset");respond.send_reset(h2::Reason::INTERNAL_ERROR);let _=publish(&cfg,Event::Error(format!("HTTP/2 {authority}: {e}"))).await;}else{tracing::debug!(elapsed_ms=started.elapsed().as_millis(),"HTTP/2 stream execution completed");}
                    }.instrument(span));
                },Some(Err(e))=>return Err(error(e)),None=>break,
            },
            result=tasks.join_next(),if !tasks.is_empty()=>{if let Some(Err(e))=result{return Err(error(e));}},
            _=tokio::time::sleep(cfg.tunnel_idle_timeout),if tasks.is_empty()=>return Err(error("HTTP/2 idle timeout")),
        }
    }
    while tasks.join_next().await.is_some() {}
    tracing::debug!(%authority,"HTTP/2 connection closed; stream tasks joined");
    Ok(())
}
async fn h2_stream(
    request: http::Request<h2::RecvStream>,
    respond: &mut h2::server::SendResponse<Bytes>,
    authority: &str,
    cfg: &Config,
    first: std::sync::Arc<tokio::sync::Mutex<Option<Box<dyn crate::Transport>>>>,
) -> io::Result<()> {
    let (parts, mut stream) = request.into_parts();
    let stream_id = stream.stream_id().as_u32();
    if parts.method == http::Method::CONNECT {
        return Err(error("Extended CONNECT is unsupported"));
    }
    let target_authority = parts
        .uri
        .authority()
        .map(|a| a.as_str())
        .unwrap_or(authority);
    let expected: http::uri::Authority = authority.parse().map_err(error)?;
    let actual: http::uri::Authority = target_authority.parse().map_err(error)?;
    if !expected.host().eq_ignore_ascii_case(actual.host())
        || expected.port_u16().unwrap_or(443) != actual.port_u16().unwrap_or(443)
    {
        return Err(error("HTTP/2 authority differs from CONNECT target"));
    }
    let path = parts.uri.path_and_query().map_or("/", |p| p.as_str());
    let target = format!("https://{authority}{path}");
    let mut body = Vec::new();
    while let Some(chunk) = tokio::time::timeout(cfg.idle_timeout, stream.data())
        .await
        .map_err(error)?
    {
        let chunk = chunk.map_err(error)?;
        if body.len() + chunk.len() > cfg.max_body {
            return Err(error("HTTP/2 request body exceeds capture/editing limit"));
        }
        body.extend_from_slice(&chunk);
        stream
            .flow_control()
            .release_capacity(chunk.len())
            .map_err(error)?;
    }
    if stream.trailers().await.map_err(error)?.is_some() {
        return Err(error(
            "HTTP/2 request trailers cannot be represented by this editor",
        ));
    }
    let mut wire = format!(
        "{} {target} HTTP/1.1\r\nHost: {authority}\r\n",
        parts.method
    )
    .into_bytes();
    let cookies = parts.headers.get_all(http::header::COOKIE);
    let cookie_fields = cookies.iter().count();
    let mut wrote_cookie = false;
    for (name, value) in &parts.headers {
        if [
            "host",
            "content-length",
            "connection",
            "transfer-encoding",
            "te",
        ]
        .contains(&name.as_str())
        {
            continue;
        }
        // HTTP/2 can split Cookie for compression. HTTP/1 requires those values joined
        // with semicolon-space (RFC 9113, 8.2.3), preserving cookie names and value order.
        if name == http::header::COOKIE && cookie_fields > 1 {
            if wrote_cookie {
                continue;
            }
            wrote_cookie = true;
            wire.extend_from_slice(b"cookie: ");
            for (index, value) in cookies.iter().enumerate() {
                if index > 0 {
                    wire.extend_from_slice(b"; ");
                }
                wire.extend_from_slice(value.as_bytes());
            }
            wire.extend_from_slice(b"\r\n");
            continue;
        }
        wire.extend_from_slice(name.as_str().as_bytes());
        wire.extend_from_slice(b": ");
        wire.extend_from_slice(value.as_bytes());
        wire.extend_from_slice(b"\r\n");
    }
    if !body.is_empty() {
        wire.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
    }
    wire.extend_from_slice(b"\r\n");
    wire.extend_from_slice(&body);
    let upstream = first.lock().await.take();
    tracing::debug!(
        preconnected = upstream.is_some(),
        request_bytes = wire.len(),
        "HTTP/2 stream selecting upstream transport"
    );
    let method = parts.method.to_string();
    let (mut sink, source) = tokio::io::duplex(64 * 1024);
    let execute = async {
        let result = crate::proxy::execute_stream(wire, cfg, true, upstream, Some(&mut sink)).await;
        let _ = sink.shutdown().await;
        result
    };
    let (mut execution, ()) =
        tokio::try_join!(execute, h2_response(source, respond, &method, cfg))?;
    execution.original.0.clear();
    execution.protocol = format!("HTTP/2 stream {stream_id}");
    execution.transformations.push(format!("HTTP/2 stream {stream_id} decoded and forwarded as HTTP/1.1; original HPACK/frame bytes unavailable"));
    if cookie_fields > 1 {
        execution.transformations.push(format!(
            "{cookie_fields} HTTP/2 Cookie fields joined with '; ' for HTTP/1.1"
        ));
        tracing::debug!(cookie_fields, "HTTP/2 Cookie fields joined for HTTP/1.1");
    }
    publish(cfg, Event::Exchange(execution)).await
}
async fn adapter_line<R: tokio::io::AsyncBufRead + Unpin>(
    reader: &mut R,
    limit: usize,
    cfg: &Config,
) -> io::Result<Vec<u8>> {
    use tokio::io::AsyncBufReadExt;
    let mut line = Vec::new();
    loop {
        let bytes = tokio::time::timeout(cfg.idle_timeout, reader.fill_buf())
            .await
            .map_err(error)??;
        if bytes.is_empty() {
            return Err(error("Incomplete HTTP/2 adapter response"));
        }
        let size = bytes
            .iter()
            .position(|b| *b == b'\n')
            .map_or(bytes.len(), |n| n + 1);
        if line.len() + size > limit {
            return Err(error("HTTP/2 adapter line limit exceeded"));
        }
        line.extend_from_slice(&bytes[..size]);
        reader.consume(size);
        if line.ends_with(b"\n") {
            return Ok(line);
        }
    }
}
async fn h2_send(send: &mut h2::SendStream<Bytes>, bytes: &[u8], cfg: &Config) -> io::Result<()> {
    let mut rest = Bytes::copy_from_slice(bytes);
    while !rest.is_empty() {
        send.reserve_capacity(rest.len());
        let capacity = tokio::time::timeout(
            cfg.idle_timeout,
            std::future::poll_fn(|cx| send.poll_capacity(cx)),
        )
        .await
        .map_err(error)?
        .ok_or_else(|| error("HTTP/2 response stream closed"))?
        .map_err(error)?;
        if capacity == 0 {
            continue;
        }
        let size = capacity.min(rest.len());
        send.send_data(rest.split_to(size), false).map_err(error)?;
    }
    Ok(())
}
async fn h2_exact(
    reader: &mut tokio::io::BufReader<tokio::io::DuplexStream>,
    send: &mut h2::SendStream<Bytes>,
    mut left: u64,
    cfg: &Config,
) -> io::Result<()> {
    use tokio::io::AsyncReadExt;
    let mut buffer = [0; 16 * 1024];
    while left > 0 {
        let size = left.min(buffer.len() as u64) as usize;
        tokio::time::timeout(cfg.idle_timeout, reader.read_exact(&mut buffer[..size]))
            .await
            .map_err(error)??;
        h2_send(send, &buffer[..size], cfg).await?;
        left -= size as u64;
    }
    Ok(())
}
async fn h2_response(
    source: tokio::io::DuplexStream,
    respond: &mut h2::server::SendResponse<Bytes>,
    method: &str,
    cfg: &Config,
) -> io::Result<()> {
    use tokio::io::AsyncReadExt;
    let mut reader = tokio::io::BufReader::new(source);
    for _ in 0..16 {
        let mut head = Vec::new();
        loop {
            let part =
                adapter_line(&mut reader, cfg.max_header.saturating_sub(head.len()), cfg).await?;
            let end = part == b"\r\n";
            head.extend(part);
            if end {
                break;
            }
        }
        let mut slots = [httparse::EMPTY_HEADER; 128];
        let mut parsed = httparse::Response::new(&mut slots);
        if !parsed.parse(&head).map_err(error)?.is_complete() {
            return Err(error("Incomplete HTTP response head"));
        }
        let status = parsed
            .code
            .ok_or_else(|| error("Response status missing"))?;
        tracing::debug!(
            status,
            head_bytes = head.len(),
            "HTTP/2 adapter parsed upstream response head"
        );
        if status < 200 {
            if status == 101 {
                return Err(error("HTTP/2 upgrades are unsupported"));
            }
            continue;
        }
        let no_body = method == "HEAD" || status == 204 || status == 304;
        let mut response = http::Response::builder().status(status);
        let mut chunked = false;
        let mut length = None;
        for header in parsed.headers.iter() {
            if header.name.eq_ignore_ascii_case("transfer-encoding") {
                chunked = header.value.eq_ignore_ascii_case(b"chunked");
                continue;
            }
            if header.name.eq_ignore_ascii_case("content-length") {
                length = Some(
                    std::str::from_utf8(header.value)
                        .map_err(error)?
                        .parse::<u64>()
                        .map_err(error)?,
                );
            }
            if [
                "connection",
                "keep-alive",
                "proxy-connection",
                "upgrade",
                "trailer",
            ]
            .iter()
            .any(|n| header.name.eq_ignore_ascii_case(n))
            {
                continue;
            }
            response = response.header(header.name, header.value);
        }
        let mut send = respond
            .send_response(response.body(()).map_err(error)?, no_body)
            .map_err(error)?;
        if no_body {
            return Ok(());
        }
        if chunked {
            loop {
                let line = adapter_line(&mut reader, cfg.max_header, cfg).await?;
                let size = u64::from_str_radix(
                    std::str::from_utf8(&line)
                        .map_err(error)?
                        .trim()
                        .split(';')
                        .next()
                        .unwrap_or(""),
                    16,
                )
                .map_err(error)?;
                if size == 0 {
                    let mut trailers = http::HeaderMap::new();
                    let mut size = 0;
                    loop {
                        let line =
                            adapter_line(&mut reader, cfg.max_header.saturating_sub(size), cfg)
                                .await?;
                        size += line.len();
                        if line == b"\r\n" {
                            break;
                        }
                        let line = std::str::from_utf8(&line).map_err(error)?.trim_end();
                        let (name, value) = line
                            .split_once(':')
                            .ok_or_else(|| error("Invalid response trailer"))?;
                        trailers.append(
                            http::header::HeaderName::from_bytes(name.as_bytes()).map_err(error)?,
                            http::HeaderValue::from_str(value.trim()).map_err(error)?,
                        );
                    }
                    if !trailers.is_empty() {
                        send.send_trailers(trailers).map_err(error)?;
                        return Ok(());
                    }
                    break;
                }
                h2_exact(&mut reader, &mut send, size, cfg).await?;
                let mut crlf = [0; 2];
                tokio::time::timeout(cfg.idle_timeout, reader.read_exact(&mut crlf))
                    .await
                    .map_err(error)??;
                if crlf != *b"\r\n" {
                    return Err(error("Invalid chunk terminator"));
                }
            }
        } else if let Some(length) = length {
            h2_exact(&mut reader, &mut send, length, cfg).await?;
        } else {
            let mut buffer = [0; 16 * 1024];
            loop {
                let size = tokio::time::timeout(cfg.idle_timeout, reader.read(&mut buffer))
                    .await
                    .map_err(error)??;
                if size == 0 {
                    break;
                }
                h2_send(&mut send, &buffer[..size], cfg).await?;
            }
        }
        send.send_data(Bytes::new(), true).map_err(error)?;
        return Ok(());
    }
    Err(error("Too many informational responses"))
}
