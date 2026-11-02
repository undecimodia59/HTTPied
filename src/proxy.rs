use crate::model::*;
use std::{io, net::SocketAddr, time::Duration};
use tracing::Instrument;
static CONNECTION_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{mpsc, watch},
    task::JoinSet,
    time::timeout,
};

#[derive(Clone, Debug)]
pub struct Config {
    pub resolver: crate::resolver::Resolver,
    pub interception: Option<crate::control::Interception>,
    pub policy: Option<watch::Receiver<std::sync::Arc<crate::policy::Policy>>>,
    pub events: Option<mpsc::Sender<Event>>,
    #[cfg(feature = "tls-spike")]
    pub ca: Option<std::sync::Arc<crate::authority::Authority>>,
    #[cfg(feature = "tls-spike")]
    pub tls: Option<std::sync::Arc<crate::tls::Intercept>>,
    pub listen: SocketAddr,
    pub max_connections: usize,
    pub max_header: usize,
    pub max_body: usize,
    pub max_capture: usize,
    pub tunnel_idle_timeout: Duration,
    pub connect_timeout: Duration,
    pub idle_timeout: Duration,
    pub total_timeout: Duration,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            resolver: Default::default(),
            interception: None,
            policy: None,
            events: None,
            #[cfg(feature = "tls-spike")]
            ca: None,
            #[cfg(feature = "tls-spike")]
            tls: None,
            listen: "127.0.0.1:8080".parse().unwrap(),
            max_connections: 32,
            max_header: 32 * 1024,
            max_body: 1024 * 1024,
            max_capture: 1024 * 1024,
            tunnel_idle_timeout: Duration::from_secs(300),
            connect_timeout: Duration::from_secs(5),
            idle_timeout: Duration::from_secs(10),
            total_timeout: Duration::from_secs(120),
        }
    }
}
fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}
pub(crate) fn phase_error(phase: &str, target: &str, error: io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("{phase} [{target}]: {error}"))
}

pub async fn bind(config: &Config) -> io::Result<TcpListener> {
    if !config.listen.ip().is_loopback() || config.max_connections == 0 {
        return Err(invalid(
            "Listener requires a loopback address and a nonzero connection limit",
        ));
    }
    TcpListener::bind(config.listen).await
}

/// The caller owns a bounded event receiver. A stalled receiver applies backpressure.
/// Shutdown aborts and joins all active connections, including stalled event senders.
pub async fn serve(
    listener: TcpListener,
    config: Config,
    events: mpsc::Sender<Event>,
    mut stop: watch::Receiver<bool>,
) -> io::Result<()> {
    let mut config = config;
    tracing::info!(listen=%listener.local_addr()?,max_connections=config.max_connections,connect_timeout_ms=config.connect_timeout.as_millis(),idle_timeout_ms=config.idle_timeout.as_millis(),total_timeout_ms=config.total_timeout.as_millis(),"proxy service starting");
    config.events = Some(events.clone());
    let mut tasks = JoinSet::new();
    let result = loop {
        if *stop.borrow() {
            break Ok(());
        }
        tokio::select! {
            biased;
            _ = stop.changed() => break Ok(()),
            result = tasks.join_next(), if !tasks.is_empty() => {
                if let Some(Err(e)) = result { break Err(io::Error::other(e)); }
            }
            accepted = listener.accept(), if tasks.len() < config.max_connections => {
                let (mut socket, peer) = match accepted { Ok(v) => v, Err(e) => break Err(e) };
                let cfg = config.clone();
                let tx = events.clone();
                let connection_id=CONNECTION_ID.fetch_add(1,std::sync::atomic::Ordering::Relaxed);
                let span=tracing::info_span!("proxy_connection",connection_id,%peer);
                tasks.spawn(async move {
                    tracing::debug!("client TCP connection accepted");
                    let event=match exchange(&mut socket,&cfg).await {
                        Ok(event)=>event,
                        Err(error) if error.kind()==io::ErrorKind::NotConnected=>{tracing::debug!(%error,"client connection ended without a standalone capture");return},
                        Err(error)=>{let message=format!("Connection {connection_id} (client {peer}): {error}");tracing::warn!(%message,error_kind=?error.kind(),error_debug=?error,"proxy exchange failed");Event::Error(message)}
                    };
                    drop(socket);
                    let _ = tx.send(event).await;
                }.instrument(span));
            }
        }
    };
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
    tracing::info!(result=?result,"proxy service stopped");
    result
}

async fn read_some(
    socket: &mut (impl tokio::io::AsyncRead + Unpin + ?Sized),
    bytes: &mut Vec<u8>,
    limit: usize,
    cfg: &Config,
) -> io::Result<usize> {
    if bytes.len() >= limit {
        return Err(invalid("message size limit exceeded"));
    }
    let mut buf = [0; 4096];
    let cap = buf.len().min(limit - bytes.len());
    let n = timeout(cfg.idle_timeout, socket.read(&mut buf[..cap])).await??;
    bytes.extend_from_slice(&buf[..n]);
    Ok(n)
}

fn framing(headers: &[httparse::Header<'_>]) -> io::Result<Option<usize>> {
    let mut length = None;
    for header in headers {
        if header.name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(invalid("Transfer-Encoding unsupported in M0"));
        }
        if header.name.eq_ignore_ascii_case("content-length") {
            if length.is_some() {
                return Err(invalid("duplicate Content-Length rejected"));
            }
            let text = std::str::from_utf8(header.value).map_err(|_| invalid("invalid length"))?;
            if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
                return Err(invalid("invalid length"));
            }
            length = Some(text.parse().map_err(|_| invalid("invalid length"))?);
        }
    }
    Ok(length)
}

async fn exchange(client: &mut TcpStream, cfg: &Config) -> io::Result<Event> {
    // No output has been sent on validation/connect failures, so an explicit error is safe.
    let prepared = match timeout(cfg.total_timeout, prepare(client, cfg)).await {
        Ok(result) => result,
        Err(_) => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "request header/body timeout",
        )),
    };
    match prepared {
        Ok(mut prepared) => {
            intercept_request(client, cfg, &mut prepared).await?;
            forward(client, cfg, prepared).await
        }
        Err(e) => {
            if e.kind() == io::ErrorKind::NotConnected {
                return Err(e);
            }
            let status = match e.kind() {
                io::ErrorKind::InvalidData => "400 Bad Request",
                io::ErrorKind::TimedOut => "408 Request Timeout",
                _ => "502 Bad Gateway",
            };
            let reply =
                format!("HTTP/1.1 {status}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n");
            let _ = timeout(cfg.idle_timeout, client.write_all(reply.as_bytes())).await;
            Err(e)
        }
    }
}
async fn intercept_request(
    client: &mut dyn crate::Transport,
    cfg: &Config,
    prepared: &mut Prepared,
) -> io::Result<()> {
    let policy = cfg
        .policy
        .as_ref()
        .map(|p| p.borrow().clone())
        .unwrap_or_default();
    if !prepared.tunnel && !policy.rules.is_empty() {
        let (bytes, changes) = policy
            .rewrite(&prepared.parsed.target, &prepared.outgoing, false)
            .map_err(io::Error::other)?;
        if changes > 0 {
            let uri: http::Uri = prepared
                .parsed
                .target
                .parse()
                .map_err(|_| invalid("Invalid rewrite target"))?;
            let first = bytes
                .windows(2)
                .position(|w| w == b"\r\n")
                .ok_or_else(|| invalid("Rewritten request has no head"))?;
            let line = std::str::from_utf8(&bytes[..first]).map_err(io::Error::other)?;
            let mut parts = line.split_whitespace();
            let method = parts
                .next()
                .ok_or_else(|| invalid("Rewritten method missing"))?;
            let path = parts
                .next()
                .ok_or_else(|| invalid("Rewritten path missing"))?;
            let target = format!(
                "{}://{}{}",
                uri.scheme_str().unwrap_or("http"),
                prepared.authority,
                path
            );
            let mut wire = format!("{method} {target} HTTP/1.1\r\n").into_bytes();
            wire.extend_from_slice(&bytes[first + 2..]);
            // Header/body edits must explicitly supply a correct Content-Length.
            let edited = prepare(&mut std::io::Cursor::new(wire), cfg).await?;
            if edited.authority != prepared.authority || edited.tunnel {
                return Err(invalid("Rewrite cannot change destination"));
            }
            prepared.outgoing = edited.outgoing;
            prepared.parsed = edited.parsed;
            prepared.edited = true;
        }
    }
    if !prepared.tunnel
        && policy.request(&prepared.parsed.target, &prepared.parsed.method)
        && let Some(control) = &cfg.interception
    {
        let held_start = std::time::Instant::now();
        let decision = control
            .pause_stage(
                prepared.authority.to_string(),
                format!(
                    "{}://{}",
                    if prepared.parsed.target.starts_with("https:") {
                        "https"
                    } else {
                        "http"
                    },
                    prepared.authority
                ),
                {
                    let uri: http::Uri = prepared
                        .parsed
                        .target
                        .parse()
                        .map_err(|_| invalid("Invalid target"))?;
                    let first = prepared
                        .outgoing
                        .windows(2)
                        .position(|w| w == b"\r\n")
                        .ok_or_else(|| invalid("Invalid serialized request"))?;
                    let mut wire = format!(
                        "{} {}://{}{} HTTP/1.1\r\n",
                        prepared.parsed.method,
                        uri.scheme_str().unwrap_or("http"),
                        prepared.authority,
                        uri.path_and_query().map_or("/", |p| p.as_str())
                    )
                    .into_bytes();
                    wire.extend_from_slice(&prepared.outgoing[first + 2..]);
                    wire
                },
                false,
            )
            .await;
        prepared.held_ms = held_start.elapsed().as_secs_f64() * 1000.0;
        match decision {
            Ok(crate::control::Decision::Forward) => {}
            Ok(crate::control::Decision::Drop) => {
                client.write_all(b"HTTP/1.1 403 Dropped by operator\r\nConnection: close\r\nContent-Length: 0\r\n\r\n").await?;
                return Err(invalid("request dropped by operator"));
            }
            Ok(crate::control::Decision::Edit(bytes)) => {
                let edited = match prepare(&mut std::io::Cursor::new(bytes), cfg).await {
                    Ok(edited) => edited,
                    Err(error) => {
                        client.write_all(b"HTTP/1.1 400 Invalid edited request\r\nConnection: close\r\nContent-Length: 0\r\n\r\n").await?;
                        return Err(error);
                    }
                };
                if edited.authority != prepared.authority || edited.tunnel {
                    client.write_all(b"HTTP/1.1 400 Invalid edited destination\r\nConnection: close\r\nContent-Length: 0\r\n\r\n").await?;
                    return Err(invalid(
                        "edited request must retain original authority and HTTP transport",
                    ));
                }
                prepared.outgoing = edited.outgoing;
                prepared.parsed = edited.parsed;
                prepared.edited = true;
            }
            Err(message) => {
                client.write_all(b"HTTP/1.1 408 Intercept expired\r\nConnection: close\r\nContent-Length: 0\r\n\r\n").await?;
                return Err(invalid(message));
            }
        }
    }
    Ok(())
}
struct Prepared {
    bytes: Vec<u8>,
    parsed: ParsedMessage,
    outgoing: Vec<u8>,
    authority: http::uri::Authority,
    edited: bool,
    held_ms: f64,
    started: std::time::Instant,
    tunnel: bool,
}
async fn prepare(
    client: &mut (impl tokio::io::AsyncRead + Unpin + ?Sized),
    cfg: &Config,
) -> io::Result<Prepared> {
    prepare_context(client, cfg, None).await
}
async fn prepare_context(
    client: &mut (impl tokio::io::AsyncRead + Unpin + ?Sized),
    cfg: &Config,
    origin: Option<&str>,
) -> io::Result<Prepared> {
    let mut bytes = Vec::new();
    let end = loop {
        if let Some(index) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            break index + 4;
        }
        match read_some(client, &mut bytes, cfg.max_header, cfg).await {
            Ok(0) if bytes.is_empty() => {
                return Err(io::Error::new(
                    io::ErrorKind::NotConnected,
                    "unused browser preconnection",
                ));
            }
            Ok(0) => return Err(invalid("client closed a partial request head")),
            Err(error)
                if bytes.is_empty()
                    && matches!(
                        error.kind(),
                        io::ErrorKind::TimedOut
                            | io::ErrorKind::ConnectionReset
                            | io::ErrorKind::BrokenPipe
                    ) =>
            {
                return Err(io::Error::new(
                    io::ErrorKind::NotConnected,
                    "unused browser preconnection",
                ));
            }
            Err(error) => return Err(error),
            _ => {}
        }
    };
    let mut slots = [httparse::EMPTY_HEADER; 128];
    let mut request = httparse::Request::new(&mut slots);
    if !request
        .parse(&bytes[..end])
        .map_err(|_| invalid("invalid HTTP head"))?
        .is_complete()
        || request.version != Some(1)
    {
        return Err(invalid("HTTP/1.1 required"));
    }
    let method = request.method.unwrap().to_owned();
    let observed_target = request.path.unwrap();
    let target = if let Some(origin) = origin {
        if !observed_target.starts_with('/') {
            return Err(invalid("TLS request requires origin-form target"));
        }
        format!("https://{origin}{observed_target}")
    } else {
        observed_target.to_owned()
    };
    let length = framing(request.headers)?.unwrap_or(0);
    if length > cfg.max_body {
        return Err(invalid("request body limit exceeded"));
    }
    let tunnel = method == "CONNECT";
    let uri: http::Uri = target.parse().map_err(|_| invalid("invalid URI"))?;
    let authority = if tunnel {
        let a: http::uri::Authority = target
            .parse()
            .map_err(|_| invalid("CONNECT requires authority"))?;
        if a.port_u16().is_none() || target.contains('@') {
            return Err(invalid("CONNECT requires explicit port"));
        }
        a
    } else {
        if !matches!(uri.scheme_str(), Some("http" | "https")) {
            return Err(invalid("absolute http/https URI required"));
        }
        uri.authority()
            .ok_or_else(|| invalid("missing authority"))?
            .clone()
    };
    if authority.as_str().contains('@') {
        return Err(invalid("userinfo unsupported"));
    }
    let websocket = request.headers.iter().any(|h| {
        h.name.eq_ignore_ascii_case("upgrade") && h.value.eq_ignore_ascii_case(b"websocket")
    });
    let mut host_count = 0;
    for h in request.headers.iter() {
        if h.name.eq_ignore_ascii_case("host") {
            host_count += 1;
            let default_port = if uri.scheme_str() == Some("https") {
                443
            } else {
                80
            };
            let host = std::str::from_utf8(h.value)
                .ok()
                .and_then(|s| s.parse::<http::uri::Authority>().ok());
            if !host.is_some_and(|h| {
                h.host().eq_ignore_ascii_case(authority.host())
                    && h.port_u16().unwrap_or(default_port)
                        == authority.port_u16().unwrap_or(default_port)
            }) {
                return Err(invalid("Host must match target authority"));
            }
        }
        if ["expect", "trailer"]
            .iter()
            .any(|n| h.name.eq_ignore_ascii_case(n))
        {
            return Err(invalid("Expect/upgrade/trailers unsupported"));
        }
        if h.name.eq_ignore_ascii_case("connection")
            && !websocket
            && ![b"close".as_slice(), b"keep-alive".as_slice()]
                .iter()
                .any(|v| h.value.eq_ignore_ascii_case(v))
        {
            return Err(invalid("unsupported Connection tokens"));
        }
    }
    if !websocket
        && request
            .headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case("upgrade"))
    {
        return Err(invalid("Unsupported protocol upgrade"));
    }
    if host_count != 1 || (tunnel && length != 0) {
        return Err(invalid("one Host required; CONNECT body forbidden"));
    }
    let parsed = ParsedMessage {
        method: method.clone(),
        target: target.clone(),
        headers: request
            .headers
            .iter()
            .map(|h| (h.name.to_owned(), h.value.to_vec()))
            .collect(),
    };
    let path = uri.path_and_query().map_or("/", |p| p.as_str());
    let mut outgoing = format!("{method} {path} HTTP/1.1\r\n").into_bytes();
    // Keep each retained original header line byte-for-byte, including spacing and casing.
    let head = &bytes[..end];
    let first = head.windows(2).position(|w| w == b"\r\n").unwrap() + 2;
    for line in head[first..end - 2].split_inclusive(|b| *b == b'\n') {
        let name = line.split(|b| *b == b':').next().unwrap_or_default();
        if [
            b"connection".as_slice(),
            b"proxy-connection",
            b"proxy-authorization",
            b"keep-alive",
        ]
        .iter()
        .any(|n| name.eq_ignore_ascii_case(n))
        {
            continue;
        }
        outgoing.extend_from_slice(line);
    }
    outgoing.extend_from_slice(b"Connection: close\r\n\r\n");
    while bytes.len() < end + length {
        if read_some(client, &mut bytes, end + length, cfg).await? == 0 {
            return Err(invalid("incomplete body"));
        }
    }
    if !tunnel && bytes.len() != end + length {
        return Err(invalid("pipelining not supported"));
    }
    if !tunnel {
        outgoing.extend_from_slice(&bytes[end..]);
    }
    if tunnel {
        outgoing = bytes[end..].to_vec();
    }
    Ok(Prepared {
        bytes,
        parsed,
        outgoing,
        authority,
        edited: false,
        held_ms: 0.0,
        started: std::time::Instant::now(),
        tunnel,
    })
}

async fn forward(client: &mut TcpStream, cfg: &Config, mut p: Prepared) -> io::Result<Event> {
    tracing::debug!(authority=%p.authority,method=%p.parsed.method,target=%p.parsed.target,tunnel=p.tunnel,"request prepared for forwarding");
    p.started = std::time::Instant::now();
    let mut upstream = match connect(&p, cfg).await {
        Ok(upstream) => upstream,
        Err(error) => {
            client
                .write_all(
                    b"HTTP/1.1 502 Bad Gateway\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
                )
                .await?;
            return Err(error);
        }
    };
    if p.tunnel {
        #[cfg(feature = "tls-spike")]
        if let Some(ca) = &cfg.ca {
            let host = p.authority.host().trim_matches(['[', ']']).to_owned();
            let mut origin = match ca.connect(&host, upstream, cfg.connect_timeout).await {
                Ok(origin) => origin,
                Err(error) => {
                    let _=timeout(cfg.idle_timeout,client.write_all(b"HTTP/1.1 502 Upstream TLS failed\r\nConnection: close\r\nContent-Length: 0\r\n\r\n")).await;
                    return Err(io::Error::new(
                        error.kind(),
                        format!("Upstream TLS {}: {error}", p.authority),
                    ));
                }
            };
            let acceptor = ca.acceptor(host).await?;
            tracing::debug!(authority=%p.authority,"sending CONNECT success; awaiting downstream TLS handshake");
            client
                .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                .await?;
            let prefixed = crate::tls::Prefixed {
                prefix: std::io::Cursor::new(p.outgoing),
                socket: client,
            };
            let started = std::time::Instant::now();
            let result = timeout(cfg.connect_timeout, acceptor.accept(prefixed)).await;
            match &result {
                Ok(Ok(stream)) => {
                    tracing::debug!(authority=%p.authority,elapsed_ms=started.elapsed().as_millis(),alpn=?stream.get_ref().1.alpn_protocol().map(String::from_utf8_lossy),"downstream TLS handshake completed")
                }
                Ok(Err(error)) => {
                    tracing::warn!(authority=%p.authority,elapsed_ms=started.elapsed().as_millis(),error_kind=?error.kind(),%error,error_debug=?error,"downstream TLS handshake failed")
                }
                Err(error) => {
                    tracing::warn!(authority=%p.authority,elapsed_ms=started.elapsed().as_millis(),%error,"downstream TLS handshake timed out")
                }
            }
            let mut downstream = result
                .map_err(|error| {
                    phase_error(
                        "Client TLS handshake timeout",
                        p.authority.as_str(),
                        error.into(),
                    )
                })?
                .map_err(|error| {
                    phase_error("Client TLS handshake", p.authority.as_str(), error)
                })?;
            if downstream.get_ref().1.alpn_protocol() == Some(b"h2".as_slice()) {
                crate::protocols::h2(
                    downstream,
                    p.authority.to_string(),
                    cfg.clone(),
                    Some(Box::new(origin)),
                )
                .await
                .map_err(|error| {
                    phase_error("Client HTTP/2 connection", p.authority.as_str(), error)
                })?;
                return Err(io::Error::new(
                    io::ErrorKind::NotConnected,
                    "HTTP/2 connection closed",
                ));
            }
            let mut prepared = prepare_context(&mut downstream, cfg, Some(p.authority.as_str()))
                .await
                .map_err(|error| {
                    phase_error("Client HTTPS request read", p.authority.as_str(), error)
                })?;
            intercept_request(&mut downstream, cfg, &mut prepared).await?;
            prepared.started = std::time::Instant::now();
            if is_websocket(&prepared) {
                return crate::protocols::websocket(
                    &mut downstream,
                    Box::new(origin),
                    prepared.bytes,
                    prepared.parsed,
                    cfg,
                )
                .await;
            }
            let execution = http_exchange(
                prepared,
                &mut origin,
                cfg,
                Some(&mut downstream),
                &mut false,
            )
            .await?;
            let _ = downstream.shutdown().await;
            return Ok(Event::Exchange(execution));
        }
        #[cfg(feature = "tls-spike")]
        if let Some(tls) = &cfg.tls
            && tls.authority == p.parsed.target
        {
            let (uploaded, downloaded) = tls
                .bridge(
                    client,
                    upstream,
                    p.outgoing,
                    cfg.connect_timeout,
                    cfg.tunnel_idle_timeout,
                )
                .await?;
            return Ok(Event::InterceptedTls {
                authority: p.parsed.target,
                uploaded,
                downloaded,
            });
        }
        client
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await?;
        upstream.write_all(&p.outgoing).await?;
        // Tunnel lifetime is bounded by inactivity; active traffic is not an HTTP exchange.
        let (up, down) =
            crate::tunnel::copy(client, &mut upstream, cfg.tunnel_idle_timeout).await?;
        return Ok(Event::Tunnel {
            authority: p.parsed.target,
            uploaded: up + p.outgoing.len() as u64,
            downloaded: down,
        });
    }
    let mut transport: Box<dyn crate::Transport> = if p.parsed.target.starts_with("https:") {
        #[cfg(feature = "tls-spike")]
        {
            let host = p.authority.host().trim_matches(['[', ']']).to_owned();
            Box::new(
                crate::authority::Authority::connect_public(&host, upstream, cfg.connect_timeout)
                    .await?,
            )
        }
        #[cfg(not(feature = "tls-spike"))]
        {
            return Err(invalid("HTTPS feature disabled"));
        }
    } else {
        Box::new(upstream)
    };
    if is_websocket(&p) {
        return crate::protocols::websocket(client, transport, p.bytes, p.parsed, cfg).await;
    }
    let mut started = false;
    match http_exchange(p, transport.as_mut(), cfg, Some(client), &mut started).await {
        Ok(execution) => Ok(Event::Exchange(execution)),
        Err(error) => {
            if started {
                return Err(error);
            }
            client
                .write_all(
                    b"HTTP/1.1 502 Bad Gateway\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
                )
                .await?;
            Err(error)
        }
    }
}
async fn connect(p: &Prepared, cfg: &Config) -> io::Result<TcpStream> {
    let host = p.authority.host().trim_matches(['[', ']']);
    let port = p
        .authority
        .port_u16()
        .unwrap_or(if p.parsed.target.starts_with("https:") {
            443
        } else {
            80
        });
    let started = std::time::Instant::now();
    let deadline = tokio::time::Instant::now() + cfg.connect_timeout;
    tracing::debug!(%host,port,budget_ms=cfg.connect_timeout.as_millis(),"DNS/TCP connect started");
    let addresses = match tokio::time::timeout_at(
        deadline,
        cfg.resolver.resolve(host, port, cfg.connect_timeout),
    )
    .await
    {
        Ok(Ok(addresses)) => addresses,
        result => {
            let error = match result {
                Ok(Err(error)) => error,
                Err(_) => io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!(
                        "DNS lookup for {host} exceeded {:?}; no TCP connection was attempted",
                        cfg.connect_timeout
                    ),
                ),
                Ok(Ok(_)) => unreachable!(),
            };
            tracing::warn!(authority=%p.authority,elapsed_ms=started.elapsed().as_millis(),error_kind=?error.kind(),%error,"DNS resolution failed before TCP connect");
            return Err(error);
        }
    };
    let dial = async {
        let mut last = None;
        for (index, address) in addresses.iter().copied().enumerate() {
            let attempt = std::time::Instant::now();
            tracing::debug!(%address,index,"TCP address attempt started");
            match TcpStream::connect(address).await {
                Ok(stream) => {
                    tracing::debug!(%address,index,elapsed_ms=attempt.elapsed().as_millis(),local=?stream.local_addr(),peer=?stream.peer_addr(),"TCP address attempt connected");
                    return Ok(stream);
                }
                Err(error) => {
                    tracing::debug!(%address,index,elapsed_ms=attempt.elapsed().as_millis(),error_kind=?error.kind(),%error,"TCP address attempt failed");
                    last = Some(error);
                }
            }
        }
        Err(last.unwrap_or_else(|| {
            io::Error::new(io::ErrorKind::AddrNotAvailable, "DNS returned no addresses")
        }))
    };
    match tokio::time::timeout_at(deadline, dial).await {
        Ok(Ok(stream)) => Ok(stream),
        Ok(Err(error)) => {
            cfg.resolver.invalidate(host, port);
            tracing::warn!(authority=%p.authority,elapsed_ms=started.elapsed().as_millis(),error_kind=?error.kind(),%error,"TCP connect failed after DNS resolution");
            Err(io::Error::new(
                error.kind(),
                format!(
                    "Cannot connect to {}: {}{}",
                    p.authority,
                    error,
                    if error.kind() == io::ErrorKind::ConnectionRefused {
                        ". Check the target host/port and whether its server is running."
                    } else {
                        ""
                    }
                ),
            ))
        }
        Err(_) => {
            cfg.resolver.invalidate(host, port);
            tracing::warn!(authority=%p.authority,elapsed_ms=started.elapsed().as_millis(),"TCP connect timed out after DNS resolution");
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!(
                    "TCP connection to {} exceeded {:?} including DNS",
                    p.authority, cfg.connect_timeout
                ),
            ))
        }
    }
}

/// Bypasses manual interception. Destination must already be explicit in an absolute request.
pub async fn execute(bytes: Vec<u8>, cfg: &Config) -> io::Result<WireExecution> {
    execute_mode(bytes, cfg, false).await
}
pub(crate) async fn execute_mode(
    bytes: Vec<u8>,
    cfg: &Config,
    manual: bool,
) -> io::Result<WireExecution> {
    execute_on(bytes, cfg, manual, None).await
}
pub(crate) async fn execute_on(
    bytes: Vec<u8>,
    cfg: &Config,
    manual: bool,
    upstream: Option<Box<dyn crate::Transport>>,
) -> io::Result<WireExecution> {
    execute_stream(bytes, cfg, manual, upstream, None).await
}
pub(crate) async fn execute_stream(
    bytes: Vec<u8>,
    cfg: &Config,
    manual: bool,
    upstream: Option<Box<dyn crate::Transport>>,
    sink: Option<&mut dyn crate::Transport>,
) -> io::Result<WireExecution> {
    let mut cfg = cfg.clone();
    if !manual {
        cfg.interception = None;
    }
    let cfg = &cfg;
    timeout(cfg.total_timeout, async {
        let mut p = prepare(&mut std::io::Cursor::new(bytes), cfg).await?;
        if p.tunnel {
            return Err(invalid("Replay does not send CONNECT"));
        }
        if manual {
            intercept_request(&mut std::io::Cursor::new(Vec::new()), cfg, &mut p).await?;
        }
        if let Some(mut upstream) = upstream {
            return http_exchange(p, upstream.as_mut(), cfg, sink, &mut false).await;
        }
        let mut upstream = connect(&p, cfg).await?;
        if p.parsed.target.starts_with("https:") {
            #[cfg(feature = "tls-spike")]
            {
                let host = p.authority.host().trim_matches(['[', ']']).to_owned();
                let mut tls = if let Some(ca) = &cfg.ca {
                    ca.connect(&host, upstream, cfg.connect_timeout).await?
                } else {
                    crate::authority::Authority::connect_public(
                        &host,
                        upstream,
                        cfg.connect_timeout,
                    )
                    .await?
                };
                return http_exchange(p, &mut tls, cfg, sink, &mut false).await;
            }
            #[cfg(not(feature = "tls-spike"))]
            return Err(invalid("HTTPS requires the tls-spike feature"));
        }
        http_exchange(p, &mut upstream, cfg, sink, &mut false).await
    })
    .await?
}
async fn http_exchange(
    p: Prepared,
    upstream: &mut dyn crate::Transport,
    cfg: &Config,
    sink: Option<&mut dyn crate::Transport>,
    started: &mut bool,
) -> io::Result<WireExecution> {
    tracing::debug!(authority=%p.authority,method=%p.parsed.method,target=%p.parsed.target,request_bytes=p.outgoing.len(),"upstream HTTP request write started");
    timeout(cfg.idle_timeout, upstream.write_all(&p.outgoing))
        .await
        .map_err(|error| {
            phase_error(
                "HTTP request write timeout",
                p.authority.as_str(),
                error.into(),
            )
        })?
        .map_err(|error| phase_error("HTTP request write", p.authority.as_str(), error))?;
    tracing::debug!(authority=%p.authority,"upstream HTTP request sent; awaiting response");
    let response_offset_ms = p.started.elapsed().as_secs_f64() * 1000.0;
    let capture = timeout(
        cfg.total_timeout,
        crate::response::read(
            upstream,
            sink,
            &p.parsed.method,
            &p.parsed.target,
            cfg,
            started,
        ),
    )
    .await
    .map_err(|_| {
        io::Error::new(
            io::ErrorKind::TimedOut,
            format!(
                "{}: HTTP response exceeded {:?}; adjust --http-timeout-secs",
                p.parsed.target, cfg.total_timeout
            ),
        )
    })?
    .map_err(|error| phase_error("HTTP response transfer", &p.parsed.target, error))?;
    let mime = response_mime(&capture.bytes);
    tracing::debug!(authority=%p.authority,status=capture.status,total_bytes=capture.total,captured_bytes=capture.bytes.len(),truncated=capture.truncated,elapsed_ms=p.started.elapsed().as_millis(),"upstream HTTP response completed");
    let mut execution = WireExecution {
        original: OriginalCapture(p.bytes),
        parsed: p.parsed,
        outgoing: p.outgoing,
        response: capture.bytes,
        response_outgoing: capture.sent,
        protocol: "HTTP/1.1".into(),
        response_total_bytes: capture.total,
        response_status: capture.status,
        response_truncated: capture.truncated,
        elapsed_ms: (p.started.elapsed().as_secs_f64() * 1000.0 - capture.held_ms).max(0.0),
        first_byte_ms: response_offset_ms + capture.first_byte_ms,
        held_ms: p.held_ms + capture.held_ms,
        response_mime: mime,
        run: None,
        transformations: vec![
            "absolute-form to origin-form target".into(),
            "remove proxy/hop connection headers; force Connection: close".into(),
            if p.edited {
                "request changed by operator or rewrite; original capture retained"
            } else {
                "no operator edits"
            }
            .into(),
        ],
    };
    execution.transformations.extend(capture.notes);
    Ok(execution)
}

fn response_mime(mut bytes: &[u8]) -> String {
    for _ in 0..16 {
        let mut headers = [httparse::EMPTY_HEADER; 128];
        let mut response = httparse::Response::new(&mut headers);
        let Ok(httparse::Status::Complete(end)) = response.parse(bytes) else {
            return String::new();
        };
        if response.code.is_some_and(|code| code < 200 && code != 101) {
            bytes = &bytes[end..];
            continue;
        }
        return response
            .headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case("content-type"))
            .map(|h| {
                String::from_utf8_lossy(h.value)
                    .split(';')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_owned()
            })
            .unwrap_or_default();
    }
    String::new()
}

fn is_websocket(p: &Prepared) -> bool {
    p.parsed.headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("upgrade") && value.eq_ignore_ascii_case(b"websocket")
    })
}
