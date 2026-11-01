use httpied::{
    control::{Decision, Interception},
    model::Event,
    proxy::{self, Config},
};
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{mpsc, watch},
};

async fn scenario(mode: &str) {
    let backend = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let host = backend.local_addr().unwrap();
    let (control, mut pending) = Interception::channel(
        1,
        Duration::from_millis(if mode == "timeout" { 50 } else { 1000 }),
    );
    control.set_enabled(true);
    let config = Config {
        listen: "127.0.0.1:0".parse().unwrap(),
        interception: Some(control.clone()),
        ..Config::default()
    };
    let listener = proxy::bind(&config).await.unwrap();
    let proxy_addr = listener.local_addr().unwrap();
    let (events, mut rx) = mpsc::channel(4);
    let (stop, stop_rx) = watch::channel(false);
    let serving = tokio::spawn(proxy::serve(listener, config.clone(), events, stop_rx));
    let original = format!("GET http://{host}/before HTTP/1.1\r\nHost: {host}\r\n\r\n");
    let bytes = original.clone();
    let client = tokio::spawn(async move {
        let mut s = TcpStream::connect(proxy_addr).await.unwrap();
        s.write_all(bytes.as_bytes()).await.unwrap();
        let mut bytes = Vec::new();
        s.read_to_end(&mut bytes).await.unwrap();
        bytes
    });
    let held = tokio::time::timeout(Duration::from_secs(2), pending.recv())
        .await
        .unwrap()
        .unwrap();
    // The backend must not even be contacted while the request is held.
    assert!(
        tokio::time::timeout(Duration::from_millis(20), backend.accept())
            .await
            .is_err()
    );
    match mode {
        "edit" => {
            let edited = format!("POST /after?q=%20 HTTP/1.1\r\nHost: {host}\r\n\r\nchanged");
            let wire = httpied::replay::build(&format!("http://{host}"), &edited, true).unwrap();
            held.decision.send(Decision::Edit(wire)).unwrap();
        }
        "forward" => held.decision.send(Decision::Forward).unwrap(),
        "off" => control.set_enabled(false),
        "drop" => held.decision.send(Decision::Drop).unwrap(),
        "timeout" => {}
        "destination" => {
            let wire = b"GET http://127.0.0.1:1/ HTTP/1.1\r\nHost: 127.0.0.1:1\r\n\r\n".to_vec();
            held.decision.send(Decision::Edit(wire)).unwrap();
        }
        "shutdown" => {
            stop.send(true).unwrap();
        }
        _ => panic!(),
    }
    if ["edit", "forward", "off"].contains(&mode) {
        let (mut socket, _) = tokio::time::timeout(Duration::from_secs(2), backend.accept())
            .await
            .unwrap()
            .unwrap();
        let mut request = Vec::new();
        loop {
            let mut buf = [0; 1024];
            let n = socket.read(&mut buf).await.unwrap();
            assert!(n > 0);
            request.extend_from_slice(&buf[..n]);
            if (mode == "edit" && request.ends_with(b"changed"))
                || (mode != "edit" && request.ends_with(b"\r\n\r\n"))
            {
                break;
            }
        }
        assert!(request.starts_with(if mode == "edit" {
            b"POST /after?q=%20"
        } else {
            b"GET /before"
        }));
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
            .await
            .unwrap();
        assert!(client.await.unwrap().ends_with(b"ok"));
        let Event::Exchange(execution) = rx.recv().await.unwrap() else {
            panic!("missing capture")
        };
        assert_eq!(execution.original.0, original.as_bytes());
        assert_eq!(execution.outgoing, request);
        if mode == "edit" {
            let restored = httpied::replay::executed_request(&execution).unwrap();
            assert!(
                String::from_utf8(restored)
                    .unwrap()
                    .contains("/after?q=%20")
            );
        }
    } else {
        let response = client.await.unwrap();
        if mode == "drop" {
            assert!(response.starts_with(b"HTTP/1.1 403"));
        }
        if mode == "timeout" {
            assert!(response.starts_with(b"HTTP/1.1 408"));
        }
        if mode == "destination" {
            assert!(response.starts_with(b"HTTP/1.1 400"));
        }
        if mode != "shutdown" {
            assert!(matches!(rx.recv().await, Some(Event::Error(_))));
        }
    }
    let _ = stop.send(true);
    tokio::time::timeout(Duration::from_secs(1), serving)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}
macro_rules! case {
    ($name:ident,$mode:literal) => {
        #[tokio::test]
        async fn $name() {
            tokio::time::timeout(Duration::from_secs(5), scenario($mode))
                .await
                .unwrap();
        }
    };
}
case!(intercept_forwards_original, "forward");
case!(intercept_edits_wire_but_preserves_original, "edit");
case!(intercept_off_releases_held, "off");
case!(intercept_drop_does_not_contact_upstream, "drop");
case!(intercept_hold_timeout_is_visible, "timeout");
case!(intercept_edit_cannot_change_destination, "destination");
case!(shutdown_cancels_held_request, "shutdown");

#[tokio::test]
async fn replay_executes_without_entering_intercept_queue() {
    let backend = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let host = backend.local_addr().unwrap();
    let (control, mut pending) = Interception::channel(1, Duration::from_secs(1));
    control.set_enabled(true);
    let cfg = Config {
        interception: Some(control),
        ..Config::default()
    };
    let fixture = tokio::spawn(async move {
        let (mut socket, _) = backend.accept().await.unwrap();
        let mut buf = [0; 1024];
        assert!(socket.read(&mut buf).await.unwrap() > 0);
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\ndone")
            .await
            .unwrap();
    });
    let request = httpied::replay::build(
        &format!("http://{host}"),
        &format!("GET /replay HTTP/1.1\nHost: {host}\n\n"),
        true,
    )
    .unwrap();
    let result = proxy::execute(request, &cfg).await.unwrap();
    assert!(result.response.ends_with(b"done"));
    assert!(pending.try_recv().is_err());
    fixture.await.unwrap();
}

#[tokio::test]
async fn cancelled_job_completes_without_connecting() {
    let (tx, rx) = mpsc::channel(1);
    let (results, mut result_rx) = mpsc::channel(1);
    let (stop, stop_rx) = watch::channel(false);
    let (cancel, cancel_rx) = watch::channel(false);
    cancel.send_replace(true);
    let worker = tokio::spawn(httpied::jobs::run(rx, results, Config::default(), stop_rx));
    tx.send(httpied::jobs::Job {
        pause: None,
        options: Default::default(),
        id: 7,
        requests: vec![b"invalid".to_vec()].into(),
        delay: Duration::ZERO,
        cancel: cancel_rx,
    })
    .await
    .unwrap();
    let result = tokio::time::timeout(Duration::from_secs(1), result_rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.id, 7);
    assert!(result.done);
    assert_eq!(result.result.unwrap_err(), "Cancelled");
    stop.send(true).unwrap();
    worker.await.unwrap();
}

#[tokio::test]
async fn cancelling_an_active_replay_closes_its_upstream_socket() {
    let backend = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let host = backend.local_addr().unwrap();
    let (tx, rx) = mpsc::channel(1);
    let (results, mut result_rx) = mpsc::channel(1);
    let (stop, stop_rx) = watch::channel(false);
    let (cancel, cancel_rx) = watch::channel(false);
    let worker = tokio::spawn(httpied::jobs::run(rx, results, Config::default(), stop_rx));
    let request = httpied::replay::build(
        &format!("http://{host}"),
        &format!("GET / HTTP/1.1\nHost: {host}\n\n"),
        true,
    )
    .unwrap();
    tx.send(httpied::jobs::Job {
        pause: None,
        options: Default::default(),
        id: 1,
        requests: vec![request].into(),
        delay: Duration::ZERO,
        cancel: cancel_rx,
    })
    .await
    .unwrap();
    let (mut socket, _) = tokio::time::timeout(Duration::from_secs(1), backend.accept())
        .await
        .unwrap()
        .unwrap();
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") {
        let mut buf = [0; 1024];
        let n = socket.read(&mut buf).await.unwrap();
        assert!(n > 0);
        bytes.extend_from_slice(&buf[..n]);
    }
    cancel.send_replace(true);
    let result = tokio::time::timeout(Duration::from_secs(1), result_rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(result.done);
    assert_eq!(result.result.unwrap_err(), "Cancelled");
    let mut byte = [0];
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), socket.read(&mut byte))
            .await
            .unwrap()
            .unwrap(),
        0
    );
    stop.send(true).unwrap();
    worker.await.unwrap();
}
