use httpied::{
    control::{Decision, Interception},
    model::Event,
    policy::Policy,
    proxy::{self, Config},
    state::RewriteRule,
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{mpsc, watch},
};

#[tokio::test]
async fn emergency_stop_aborts_sockets_while_result_delivery_is_full() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target = origin.local_addr().unwrap();
        let (closed, closure) = tokio::sync::oneshot::channel();
        let fixture = tokio::spawn(async move {
            let mut tasks = tokio::task::JoinSet::new();
            let mut closed = Some(closed);
            for _ in 0..3 {
                let (mut stream, _) = origin.accept().await.unwrap();
                let mut head = Vec::new();
                while !head.ends_with(b"\r\n\r\n") {
                    head.push(stream.read_u8().await.unwrap());
                }
                if head.starts_with(b"GET /1 ") {
                    let closed = closed.take().unwrap();
                    tasks.spawn(async move {
                        let mut tail = Vec::new();
                        stream.read_to_end(&mut tail).await.unwrap();
                        let _ = closed.send(());
                    });
                } else {
                    stream
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                        .await
                        .unwrap();
                }
            }
            while tasks.join_next().await.is_some() {}
        });
        let (commands, rx) = mpsc::channel(1);
        let (results, mut responses) = mpsc::channel(1);
        let (stop, shutdown) = watch::channel(false);
        let jobs = tokio::spawn(httpied::jobs::run(rx, results, Config::default(), shutdown));
        let (cancel, cancelled) = watch::channel(false);
        commands
            .send(httpied::jobs::Job {
                id: 9,
                requests: httpied::jobs::Requests::Payload {
                    destination: format!("http://{target}"),
                    template: "GET /{{payload}} HTTP/1.1\r\n\r\n".into(),
                    repair: true,
                    values: httpied::payload::Payloads::numbers("0", "9", "1").unwrap(),
                },
                delay: Duration::ZERO,
                cancel: cancelled,
                pause: None,
                options: httpied::jobs::Options {
                    concurrency: 2,
                    ..Default::default()
                },
            })
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        cancel.send_replace(true);
        tokio::time::timeout(Duration::from_secs(1), closure)
            .await
            .unwrap()
            .unwrap();
        let mut seen = Vec::new();
        loop {
            let result = responses.recv().await.unwrap();
            let done = result.done;
            if !result.terminal {
                seen.push((result.index, result.payloads, result.result.is_err()));
            }
            if done {
                break;
            }
        }
        assert_eq!(seen.len(), 3);
        assert!(
            seen.iter()
                .any(|(index, payloads, error)| *index == 1 && payloads == &["1"] && *error)
        );
        stop.send(true).unwrap();
        jobs.await.unwrap();
        fixture.await.unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn conditional_request_and_response_edits_preserve_observed_bytes() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let host = origin.local_addr().unwrap();
        let fixture = tokio::spawn(async move {
            let (mut stream, _) = origin.accept().await.unwrap();
            let mut head = Vec::new();
            while !head.ends_with(b"\r\n\r\n") {
                head.push(stream.read_u8().await.unwrap());
            }
            assert!(head.starts_with(b"GET /after HTTP/1.1"));
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 4\r\n\r\npong",
                )
                .await
                .unwrap();
        });
        let (control, mut pending) = Interception::channel(2, Duration::from_secs(2));
        control.set_enabled(true);
        let policy = Policy {
            responses: true,
            status: "2".into(),
            method: "GET".into(),
            rules: vec![RewriteRule {
                enabled: true,
                response: false,
                host: "127.0.0.1".into(),
                find: "/before".into(),
                replace: "/after".into(),
            }],
            ..Default::default()
        };
        let (_, policy) = watch::channel(Arc::new(policy));
        let cfg = Config {
            listen: "127.0.0.1:0".parse().unwrap(),
            interception: Some(control),
            policy: Some(policy),
            ..Config::default()
        };
        let listener = proxy::bind(&cfg).await.unwrap();
        let address = listener.local_addr().unwrap();
        let (events, mut rx) = mpsc::channel(4);
        let (stop, shutdown) = watch::channel(false);
        let serving = tokio::spawn(proxy::serve(listener, cfg, events, shutdown));
        let client = tokio::spawn(async move {
            let mut client = TcpStream::connect(address).await.unwrap();
            client
                .write_all(
                    format!("GET http://{host}/before HTTP/1.1\r\nHost: {host}\r\n\r\n").as_bytes(),
                )
                .await
                .unwrap();
            let mut bytes = Vec::new();
            client.read_to_end(&mut bytes).await.unwrap();
            bytes
        });
        let request = pending.recv().await.unwrap();
        assert!(!request.response);
        assert!(String::from_utf8_lossy(&request.request).contains("/after"));
        request.decision.send(Decision::Forward).unwrap();
        let response = pending.recv().await.unwrap();
        assert!(response.response);
        assert!(response.request.ends_with(b"pong"));
        response
            .decision
            .send(Decision::Edit(
                b"HTTP/1.1 201 Changed\r\nContent-Length: 999\r\n\r\nedited".to_vec(),
            ))
            .unwrap();
        let sent = client.await.unwrap();
        assert!(sent.starts_with(b"HTTP/1.1 201"));
        assert!(sent.ends_with(b"edited"));
        assert!(String::from_utf8_lossy(&sent).contains("Content-Length: 6"));
        let Event::Exchange(e) = rx.recv().await.unwrap() else {
            panic!("expected exchange")
        };
        assert!(String::from_utf8_lossy(&e.original.0).contains("/before"));
        assert!(e.response.ends_with(b"pong"));
        assert!(e.response_outgoing.ends_with(b"edited"));
        stop.send(true).unwrap();
        serving.await.unwrap().unwrap();
        fixture.await.unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn intruder_pause_and_concurrency_are_bounded_and_results_keep_payloads() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let host = origin.local_addr().unwrap();
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let count = Arc::new(AtomicUsize::new(0));
        let (a, p, c) = (active.clone(), peak.clone(), count.clone());
        let fixture = tokio::spawn(async move {
            let mut tasks = tokio::task::JoinSet::new();
            for _ in 0..6 {
                let (mut stream, _) = origin.accept().await.unwrap();
                let (a, p, c) = (a.clone(), p.clone(), c.clone());
                tasks.spawn(async move {
                    let n = a.fetch_add(1, Ordering::SeqCst) + 1;
                    p.fetch_max(n, Ordering::SeqCst);
                    c.fetch_add(1, Ordering::SeqCst);
                    let mut head = Vec::new();
                    while !head.ends_with(b"\r\n\r\n") {
                        head.push(stream.read_u8().await.unwrap());
                    }
                    tokio::time::sleep(Duration::from_millis(60)).await;
                    stream
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                        .await
                        .unwrap();
                    a.fetch_sub(1, Ordering::SeqCst);
                });
            }
            while tasks.join_next().await.is_some() {}
        });
        let (tx, commands) = mpsc::channel(1);
        let (results, mut rx) = mpsc::channel(2);
        let (stop, shutdown) = watch::channel(false);
        let jobs = tokio::spawn(httpied::jobs::run(
            commands,
            results,
            Config::default(),
            shutdown,
        ));
        let (_cancel, cancel) = watch::channel(false);
        let (pause, paused) = watch::channel(true);
        tx.send(httpied::jobs::Job {
            id: 71,
            requests: httpied::jobs::Requests::Payload {
                destination: format!("http://{host}"),
                template: "GET /{{payload}} HTTP/1.1\r\n\r\n".into(),
                repair: true,
                values: httpied::payload::Payloads::numbers("0", "5", "1").unwrap(),
            },
            delay: Duration::ZERO,
            cancel,
            pause: Some(paused),
            options: httpied::jobs::Options {
                concurrency: 2,
                ..Default::default()
            },
        })
        .await
        .unwrap();
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(count.load(Ordering::SeqCst), 0);
        pause.send_replace(false);
        let first = rx.recv().await.unwrap();
        assert_eq!(first.payloads.len(), 1);
        assert!(first.result.unwrap().run.is_some());
        pause.send_replace(true);
        tokio::time::sleep(Duration::from_millis(30)).await;
        let paused_count = count.load(Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(120)).await;
        assert_eq!(count.load(Ordering::SeqCst), paused_count);
        assert!(paused_count <= 4);
        pause.send_replace(false);
        let mut done = false;
        for _ in 1..6 {
            let result = rx.recv().await.unwrap();
            assert!(result.result.is_ok());
            done = result.done;
        }
        assert!(done);
        assert!(peak.load(Ordering::SeqCst) <= 2);
        assert_eq!(count.load(Ordering::SeqCst), 6);
        stop.send(true).unwrap();
        jobs.await.unwrap();
        fixture.await.unwrap();
    })
    .await
    .unwrap();
}
