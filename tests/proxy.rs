use httpied::{
    model::Event,
    proxy::{self, Config},
};
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{mpsc, watch},
};

async fn start(
    mut cfg: Config,
) -> (
    std::net::SocketAddr,
    watch::Sender<bool>,
    mpsc::Receiver<Event>,
    tokio::task::JoinHandle<std::io::Result<()>>,
) {
    cfg.listen = "127.0.0.1:0".parse().unwrap();
    let listener = proxy::bind(&cfg).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel(1);
    let (stop, stop_rx) = watch::channel(false);
    (
        addr,
        stop,
        rx,
        tokio::spawn(proxy::serve(listener, cfg, tx, stop_rx)),
    )
}
async fn request(addr: std::net::SocketAddr, bytes: &[u8]) -> Vec<u8> {
    let mut socket = TcpStream::connect(addr).await.unwrap();
    socket.write_all(bytes).await.unwrap();
    let mut response = Vec::new();
    tokio::time::timeout(Duration::from_secs(3), socket.read_to_end(&mut response))
        .await
        .unwrap()
        .unwrap();
    response
}

#[tokio::test]
async fn post_preserves_raw_headers_and_query_and_body() {
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let host = upstream.local_addr().unwrap();
    let fixture = tokio::spawn(async move {
        let (mut socket, _) = upstream.accept().await.unwrap();
        let mut bytes = Vec::new();
        loop {
            let mut buf = [0; 1024];
            let n = socket.read(&mut buf).await.unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&buf[..n]);
            if bytes.ends_with(b"hello") {
                break;
            }
        }
        // Fragmented response and a backend that does not close: length owns framing.
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n")
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
        socket.write_all(b"ok").await.unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
        bytes
    });
    let (addr, stop, mut events, task) = start(Config::default()).await;
    let input = format!(
        "POST http://{host}/a%2fb?q=%20 HTTP/1.1\r\nHost: {host}\r\nx-Mix:  one\r\nX-Mix:\ttwo\r\nContent-Length: 5\r\nProxy-Authorization: secret\r\n\r\nhello"
    );
    let result = request(addr, input.as_bytes()).await;
    assert!(result.ends_with(b"ok"));
    let wire = fixture.await.unwrap();
    assert!(wire.starts_with(b"POST /a%2fb?q=%20 HTTP/1.1\r\n"));
    assert!(
        wire.windows(b"x-Mix:  one\r\nX-Mix:\ttwo\r\n".len())
            .any(|w| w == b"x-Mix:  one\r\nX-Mix:\ttwo\r\n")
    );
    assert!(!String::from_utf8_lossy(&wire).contains("secret"));
    match events.recv().await.unwrap() {
        Event::Exchange(e) => {
            assert_eq!(e.original.0, input.as_bytes());
            assert_eq!(e.outgoing, wire);
            assert_eq!(e.response, result);
        }
        e => panic!("unexpected {e:?}"),
    }
    stop.send(true).unwrap();
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn rejects_ambiguous_unsupported_and_oversize_inputs() {
    let cfg = Config {
        max_body: 8,
        ..Config::default()
    };
    let (addr, stop, mut events, task) = start(cfg).await;
    for headers in [
        "Content-Length: 1\r\nContent-Length: 1",
        "Transfer-Encoding: chunked",
        "Content-Length: 9",
        "Expect: 100-continue",
        "Connection: x-secret",
        "Content-Length: +1",
    ] {
        let bytes =
            format!("POST http://127.0.0.1:1/ HTTP/1.1\r\nHost: 127.0.0.1:1\r\n{headers}\r\n\r\n");
        assert!(
            request(addr, bytes.as_bytes())
                .await
                .starts_with(b"HTTP/1.1 400")
        );
        assert!(matches!(events.recv().await, Some(Event::Error(_))));
    }
    stop.send(true).unwrap();
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn tunnel_forwards_prefetched_bytes_and_half_closes() {
    let fixture = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let host = fixture.local_addr().unwrap();
    let backend = tokio::spawn(async move {
        let (mut s, _) = fixture.accept().await.unwrap();
        let mut bytes = Vec::new();
        s.read_to_end(&mut bytes).await.unwrap();
        assert_eq!(bytes, b"opaque");
        s.write_all(b"reply").await.unwrap();
    });
    let (addr, stop, mut events, task) = start(Config::default()).await;
    let mut socket = TcpStream::connect(addr).await.unwrap();
    socket
        .write_all(format!("CONNECT {host} HTTP/1.1\r\nHost: {host}\r\n\r\nopaque").as_bytes())
        .await
        .unwrap();
    socket.shutdown().await.unwrap();
    let mut bytes = Vec::new();
    socket.read_to_end(&mut bytes).await.unwrap();
    assert!(bytes.starts_with(b"HTTP/1.1 200 Connection Established\r\n\r\n"));
    assert!(bytes.ends_with(b"reply"));
    backend.await.unwrap();
    assert!(matches!(
        events.recv().await,
        Some(Event::Tunnel {
            uploaded: 6,
            downloaded: 5,
            ..
        })
    ));
    stop.send(true).unwrap();
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn cancellation_joins_active_and_backpressured_tasks() {
    let cfg = Config {
        max_connections: 2,
        idle_timeout: Duration::from_millis(50),
        ..Config::default()
    };
    let (addr, stop, _events, task) = start(cfg).await;
    let mut sockets = Vec::new();
    for _ in 0..4 {
        sockets.push(TcpStream::connect(addr).await.unwrap());
    }
    tokio::time::sleep(Duration::from_millis(120)).await;
    stop.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn response_chunking_is_forwarded() {
    let fixture = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let host = fixture.local_addr().unwrap();
    let backend = tokio::spawn(async move {
        let (mut s, _) = fixture.accept().await.unwrap();
        let mut buf = [0; 4096];
        assert!(s.read(&mut buf).await.unwrap() > 0);
        s.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n")
            .await
            .unwrap();
    });
    let (addr, stop, mut events, task) = start(Config::default()).await;
    let bytes = format!("GET http://{host}/ HTTP/1.1\r\nHost: {host}\r\n\r\n");
    assert!(
        request(addr, bytes.as_bytes())
            .await
            .starts_with(b"HTTP/1.1 200")
    );
    assert!(matches!(events.recv().await, Some(Event::Exchange(_))));
    backend.await.unwrap();
    stop.send(true).unwrap();
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn refuses_non_loopback_listener() {
    let cfg = Config {
        listen: "0.0.0.0:8080".parse().unwrap(),
        ..Config::default()
    };
    assert!(proxy::bind(&cfg).await.is_err());
}

#[tokio::test]
async fn partial_get_and_head_with_advertised_body_length() {
    for method in ["GET", "HEAD"] {
        let fixture = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let host = fixture.local_addr().unwrap();
        let backend = tokio::spawn(async move {
            let (mut socket, _) = fixture.accept().await.unwrap();
            let mut bytes = Vec::new();
            while !bytes.ends_with(b"\r\n\r\n") {
                let mut buf = [0; 64];
                let n = socket.read(&mut buf).await.unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&buf[..n]);
            }
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 99\r\n\r\n")
                .await
                .unwrap();
        });
        let (addr, stop, mut events, task) = start(Config::default()).await;
        let mut socket = TcpStream::connect(addr).await.unwrap();
        socket
            .write_all(format!("{method} http://{host}/ HTTP/1.1\r\n").as_bytes())
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
        socket
            .write_all(format!("Host: {host}\r\n\r\n").as_bytes())
            .await
            .unwrap();
        let mut result = Vec::new();
        socket.read_to_end(&mut result).await.unwrap();
        if method == "HEAD" {
            assert!(result.starts_with(b"HTTP/1.1 200"));
            assert!(matches!(events.recv().await, Some(Event::Exchange(_))));
        } else {
            assert!(result.starts_with(b"HTTP/1.1 200")); // Headers streamed before the origin truncated its body.
            assert!(matches!(events.recv().await, Some(Event::Error(_))));
        }
        backend.await.unwrap();
        stop.send(true).unwrap();
        task.await.unwrap().unwrap();
    }
}

#[tokio::test]
async fn idle_and_total_deadlines_are_observable() {
    for total in [false, true] {
        let cfg = Config {
            idle_timeout: Duration::from_millis(if total { 1000 } else { 20 }),
            total_timeout: Duration::from_millis(if total { 20 } else { 1000 }),
            ..Config::default()
        };
        let (addr, stop, mut events, task) = start(cfg).await;
        let mut socket = TcpStream::connect(addr).await.unwrap();
        socket.write_all(b"GET http://localhost/").await.unwrap();
        let mut bytes = Vec::new();
        tokio::time::timeout(Duration::from_secs(1), socket.read_to_end(&mut bytes))
            .await
            .unwrap()
            .unwrap();
        let Some(Event::Error(error)) = events.recv().await else {
            panic!("missing timeout error");
        };
        assert!(
            error.contains("timeout") || error.contains("deadline") || error.contains("timed out"),
            "{error}"
        );
        if !total {
            assert!(bytes.starts_with(b"HTTP/1.1 408"));
        }
        stop.send(true).unwrap();
        task.await.unwrap().unwrap();
    }
}

#[tokio::test]
async fn large_response_streams_with_bounded_truncated_capture() {
    let fixture = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let host = fixture.local_addr().unwrap();
    let backend = tokio::spawn(async move {
        let (mut socket, _) = fixture.accept().await.unwrap();
        let mut buf = [0; 4096];
        assert!(socket.read(&mut buf).await.unwrap() > 0);
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 10485760\r\n\r\n")
            .await
            .unwrap();
        for _ in 0..640 {
            socket.write_all(&[b'x'; 16384]).await.unwrap();
        }
    });
    let cfg = Config {
        max_capture: 1024,
        ..Config::default()
    };
    let (addr, stop, mut events, task) = start(cfg).await;
    let bytes = format!("GET http://{host}/ HTTP/1.1\r\nHost: {host}\r\n\r\n");
    let response = request(addr, bytes.as_bytes()).await;
    assert!(response.starts_with(b"HTTP/1.1 200"));
    assert!(response.len() > 10485760);
    let Some(Event::Exchange(capture)) = events.recv().await else {
        panic!("missing capture")
    };
    assert!(capture.response_truncated);
    assert_eq!(capture.response.len(), 1024);
    assert_eq!(capture.response_total_bytes, response.len() as u64);
    backend.await.unwrap();
    stop.send(true).unwrap();
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn clean_preconnections_do_not_become_errors() {
    let (addr, stop, mut events, task) = start(Config::default()).await;
    for _ in 0..5 {
        drop(TcpStream::connect(addr).await.unwrap());
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(events.try_recv().is_err());
    stop.send(true).unwrap();
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn partial_request_head_is_still_reported() {
    let (addr, stop, mut events, task) = start(Config::default()).await;
    let mut socket = TcpStream::connect(addr).await.unwrap();
    socket.write_all(b"GET /part").await.unwrap();
    socket.shutdown().await.unwrap();
    let mut response = Vec::new();
    socket.read_to_end(&mut response).await.unwrap();
    assert!(response.starts_with(b"HTTP/1.1 400"));
    let Some(Event::Error(error)) = events.recv().await else {
        panic!("missing error")
    };
    assert!(error.contains("partial request"));
    stop.send(true).unwrap();
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn refused_origin_error_identifies_the_target() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let host = listener.local_addr().unwrap();
    drop(listener);
    let (addr, stop, mut events, task) = start(Config::default()).await;
    let response = request(
        addr,
        format!("GET http://{host}/ HTTP/1.1\r\nHost: {host}\r\n\r\n").as_bytes(),
    )
    .await;
    assert!(response.starts_with(b"HTTP/1.1 502"));
    let Some(Event::Error(error)) = events.recv().await else {
        panic!("missing error")
    };
    assert!(error.contains(&host.to_string()));
    assert!(error.contains("server is running"));
    stop.send(true).unwrap();
    task.await.unwrap().unwrap();
}

async fn origin_response(wire: Vec<u8>, capture_limit: usize) -> (Vec<u8>, Event) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let host = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut s, _) = listener.accept().await.unwrap();
        let mut buf = [0; 4096];
        assert!(s.read(&mut buf).await.unwrap() > 0);
        let _ = s.write_all(&wire).await;
    });
    let (addr, stop, mut events, task) = start(Config {
        max_capture: capture_limit,
        ..Config::default()
    })
    .await;
    let response = request(
        addr,
        format!("GET http://{host}/ HTTP/1.1\r\nHost: {host}\r\n\r\n").as_bytes(),
    )
    .await;
    let event = events.recv().await.unwrap();
    server.await.unwrap();
    stop.send(true).unwrap();
    task.await.unwrap().unwrap();
    (response, event)
}
#[tokio::test]
async fn close_delimited_response_and_chunk_extensions_and_trailers_are_forwarded() {
    for wire in [b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nclose-body".to_vec(),b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4;name=value\r\ntest\r\n0\r\nX-Checksum: local\r\n\r\n".to_vec()]{
        let (response,event)=origin_response(wire.clone(),64).await;assert_eq!(response,wire);
        let Event::Exchange(capture)=event else{panic!("expected capture")};assert_eq!(capture.response_total_bytes,wire.len() as u64);assert_eq!(capture.response,wire[..wire.len().min(64)]);assert_eq!(capture.response_truncated,wire.len()>64);
    }
}
#[tokio::test]
async fn ambiguous_response_framing_is_rejected_before_forwarding() {
    let (response, event) = origin_response(
        b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n"
            .to_vec(),
        4096,
    )
    .await;
    assert!(response.starts_with(b"HTTP/1.1 502"));
    assert!(matches!(event,Event::Error(message) if message.contains("ambiguous")));
}
#[tokio::test]
async fn malformed_chunk_does_not_inject_a_second_http_response() {
    let (response, event) = origin_response(
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\ninvalid\r\n".to_vec(),
        4096,
    )
    .await;
    assert!(response.starts_with(b"HTTP/1.1 200"));
    assert!(!response.windows(12).any(|bytes| bytes == b"HTTP/1.1 502"));
    assert!(matches!(event,Event::Error(message) if message.contains("chunk")));
}
#[tokio::test]
async fn response_headers_and_body_start_arrive_before_origin_finishes() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let host = listener.local_addr().unwrap();
    let (release, wait) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = [0; 1024];
        assert!(socket.read(&mut buf).await.unwrap() > 0);
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\nabc")
            .await
            .unwrap();
        wait.await.unwrap();
        socket.write_all(b"def").await.unwrap();
    });
    let (addr, stop, mut events, task) = start(Config::default()).await;
    let mut socket = TcpStream::connect(addr).await.unwrap();
    socket
        .write_all(format!("GET http://{host}/ HTTP/1.1\r\nHost: {host}\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut first = vec![0; b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\nabc".len()];
    tokio::time::timeout(Duration::from_secs(1), socket.read_exact(&mut first))
        .await
        .unwrap()
        .unwrap();
    assert!(first.ends_with(b"abc"));
    release.send(()).unwrap();
    let mut rest = Vec::new();
    socket.read_to_end(&mut rest).await.unwrap();
    assert_eq!(rest, b"def");
    assert!(matches!(events.recv().await, Some(Event::Exchange(_))));
    server.await.unwrap();
    stop.send(true).unwrap();
    task.await.unwrap().unwrap();
}
#[tokio::test]
async fn active_connect_tunnel_outlives_http_exchange_budget() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let host = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        for _ in 0..5 {
            let byte = socket.read_u8().await.unwrap();
            socket.write_u8(byte).await.unwrap();
        }
    });
    let (addr, stop, mut events, task) = start(Config {
        total_timeout: Duration::from_millis(50),
        tunnel_idle_timeout: Duration::from_secs(1),
        ..Config::default()
    })
    .await;
    let mut socket = TcpStream::connect(addr).await.unwrap();
    socket
        .write_all(format!("CONNECT {host} HTTP/1.1\r\nHost: {host}\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut head = vec![0; b"HTTP/1.1 200 Connection Established\r\n\r\n".len()];
    socket.read_exact(&mut head).await.unwrap();
    for index in 0..5 {
        tokio::time::sleep(Duration::from_millis(30)).await;
        socket.write_u8(index).await.unwrap();
        assert_eq!(socket.read_u8().await.unwrap(), index);
    }
    socket.shutdown().await.unwrap();
    assert!(matches!(events.recv().await, Some(Event::Tunnel { .. })));
    server.await.unwrap();
    stop.send(true).unwrap();
    task.await.unwrap().unwrap();
}
