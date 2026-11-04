use httpied::{
    campaign::{self, Job, Kind, SavedRun, Step, Update, Workspace},
    proxy::Config,
    state::Variable,
};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::{mpsc, watch},
};
struct Fixture {
    destination: String,
    seen: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn fixture(handler: fn(&str) -> String) -> Fixture {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let destination = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = seen.clone();
    let task = tokio::spawn(async move {
        let mut connections = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                value=listener.accept()=>{let (mut socket,_)=value.unwrap();let seen=record.clone();connections.spawn(async move{
                    let mut request=Vec::new();loop{
                        let mut buf=[0;4096];let n=socket.read(&mut buf).await.unwrap();if n==0{return;}
                        request.extend_from_slice(&buf[..n]);
                        if let Some(end)=request.windows(4).position(|w|w==b"\r\n\r\n"){
                            let head=String::from_utf8_lossy(&request[..end]);let length=head.lines().filter_map(|line|line.split_once(':')).find(|(k,_)|k.eq_ignore_ascii_case("content-length")).map(|(_,v)|v.trim().parse::<usize>().unwrap()).unwrap_or(0);
                            if request.len()>=end+4+length{break;}
                        }
                    }
                    let request=String::from_utf8(request).unwrap();seen.lock().unwrap().push(request.clone());let response=handler(&request);let _=socket.write_all(response.as_bytes()).await;
                });},
                _=connections.join_next(),if !connections.is_empty()=>{}
            }
        }
    });
    Fixture {
        destination,
        seen,
        task,
    }
}
fn response(status: u16, extra: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\n{extra}\r\n{body}",
        body.len()
    )
}
fn step(destination: &str, path: &str) -> Step {
    Step {
        name: path.into(),
        destination: destination.into(),
        request: format!("GET {path} HTTP/1.1\r\n\r\n"),
        ..Default::default()
    }
}
async fn run(
    workspace: Workspace,
    variables: Vec<Variable>,
) -> (
    Vec<(campaign::Record, Option<Box<httpied::model::WireExecution>>)>,
    String,
) {
    let (jobs, job_rx) = mpsc::channel(1);
    let (updates, mut results) = mpsc::channel(2);
    let (stop, stop_rx) = watch::channel(false);
    let (_cancel, cancel) = watch::channel(false);
    let worker = tokio::spawn(campaign::run(job_rx, updates, Config::default(), stop_rx));
    let run = SavedRun {
        status: "Running".into(),
        id: 123,
        name: "fixture".into(),
        workspace,
        variables: variables.clone(),
    };
    jobs.send(Job {
        run,
        variables,
        scope: vec![],
        enforce_scope: false,
        global_rps: 0,
        host_rps: 0,
        cancel,
    })
    .await
    .unwrap();
    let (rows, message) = tokio::time::timeout(Duration::from_secs(10), async {
        let mut rows = Vec::new();
        loop {
            match results.recv().await.unwrap() {
                Update::Result { record, execution } => rows.push((*record, execution)),
                Update::Done { message, .. } => return (rows, message),
            }
        }
    })
    .await
    .unwrap();
    stop.send(true).unwrap();
    worker.await.unwrap();
    (rows, message)
}
#[tokio::test]
async fn synchronized_bursts_capture_every_exact_request_and_release_timing() {
    let fixture =
        fixture(|request| response(200, "", request.split_once("\r\n\r\n").unwrap().1)).await;
    let mut request = step(&fixture.destination, "/race");
    request.request =
        "POST /race HTTP/1.1\r\nX-Exact: keep\r\nContent-Length: 4\r\n\r\nABCD".into();
    let (rows, message) = run(
        Workspace {
            steps: vec![request],
            burst_size: 4,
            connections: 4,
            bursts: 2,
            ..Default::default()
        },
        vec![],
    )
    .await;
    assert_eq!(message, "Completed");
    assert_eq!(rows.len(), 8);
    assert_eq!(fixture.seen.lock().unwrap().len(), 8);
    for burst in 0..2 {
        let mut positions = rows
            .iter()
            .filter(|(r, _)| r.burst == burst)
            .map(|(r, _)| r.position)
            .collect::<Vec<_>>();
        positions.sort();
        assert_eq!(positions, vec![1, 2, 3, 4]);
    }
    for (record, execution) in rows {
        let execution = execution.unwrap();
        assert_eq!(record.status, 200);
        assert!(record.release_us.is_some());
        assert!(execution.outgoing.ends_with(b"ABCD"));
        assert!(
            execution
                .outgoing
                .windows(13)
                .any(|w| w == b"X-Exact: keep")
        );
        assert!(!record.body_hash.is_empty());
    }
}
#[tokio::test]
async fn sequences_extract_tokens_refresh_cookies_and_isolate_each_cycle() {
    let fixture = fixture(|request| {
        if request.starts_with("GET /login ") {
            response(
                200,
                "Set-Cookie: sid=fresh; Path=/; HttpOnly\r\nContent-Type: application/json\r\n",
                r#"{"token":"fresh-token"}"#,
            )
        } else {
            assert!(request.starts_with("GET /use/fresh-token "));
            assert!(request.contains("Cookie: other=keep; sid=fresh"));
            assert!(request.contains("X-CSRF: fresh-token"));
            assert!(!request.contains("sid=stale"));
            response(200, "", "done")
        }
    })
    .await;
    let mut first = step(&fixture.destination, "/login");
    first
        .extractors
        .push(campaign::sequence::Extractor::default());
    let mut second = step(&fixture.destination, "/use/{{token}}");
    second.request =
        "GET /use/{{token}} HTTP/1.1\r\nCookie: sid=stale; other=keep\r\nX-CSRF: {{token}}\r\n\r\n"
            .into();
    let (rows, message) = run(
        Workspace {
            kind: Kind::Sequence,
            steps: vec![first, second],
            cycles: 2,
            ..Default::default()
        },
        vec![],
    )
    .await;
    assert_eq!(message, "Completed");
    assert_eq!(rows.len(), 4);
    assert!(rows.iter().all(|(r, _)| r.error.is_empty()));
    assert_eq!(fixture.seen.lock().unwrap().len(), 4);
}
#[tokio::test]
async fn a_required_extraction_failure_never_executes_the_dependent_step() {
    let fixture = fixture(|_| response(200, "Content-Type: application/json\r\n", "{}")).await;
    let mut first = step(&fixture.destination, "/missing");
    first
        .extractors
        .push(campaign::sequence::Extractor::default());
    let (rows, message) = run(
        Workspace {
            kind: Kind::Sequence,
            steps: vec![first, step(&fixture.destination, "/must-not-run")],
            ..Default::default()
        },
        vec![],
    )
    .await;
    assert_eq!(rows.len(), 1);
    assert!(rows[0].0.error.contains("Required variable"));
    assert!(message.contains("stopped at step 1"));
    assert_eq!(fixture.seen.lock().unwrap().len(), 1);
}
#[tokio::test]
async fn access_control_compares_fresh_baselines_and_strips_original_credentials() {
    let fixture = fixture(|request| {
        if request.starts_with("GET /public ") || request.contains("Authorization: Bearer high") {
            response(200, "", "private payload")
        } else {
            assert!(!request.contains("Cookie: original"));
            assert!(!request.contains("Bearer high"));
            response(403, "", "denied")
        }
    })
    .await;
    let mut protected = step(&fixture.destination, "/protected");
    protected.request="GET /protected HTTP/1.1\r\nAuthorization: Bearer high\r\nCookie: original=1\r\nAuthorization: duplicate\r\n\r\n".into();
    let profiles = vec![campaign::access::Identity {
        name: "User B".into(),
        headers: "Authorization: Bearer low".into(),
    }];
    let (rows, message) = run(
        Workspace {
            kind: Kind::Access,
            steps: vec![protected, step(&fixture.destination, "/public")],
            profiles,
            ..Default::default()
        },
        vec![],
    )
    .await;
    assert_eq!(message, "Completed");
    assert_eq!(rows.len(), 6);
    assert_eq!(rows[1].0.verdict, "Denied by HTTP status");
    assert_eq!(rows[2].0.identity, "Anonymous");
    assert_eq!(
        rows[4].0.verdict,
        "Potential bypass: identical successful body"
    );
    assert_eq!(rows[4].0.baseline, Some(3));
    let requests = fixture.seen.lock().unwrap();
    assert_eq!(requests[1].matches("Authorization:").count(), 1);
    assert_eq!(requests[2].matches("Authorization:").count(), 0);
}
#[tokio::test]
async fn alternate_identity_values_are_expanded_once() {
    let fixture = fixture(|request| {
        assert!(request.contains("Bearer {{literal}}"));
        assert!(!request.contains("must-not-expand"));
        response(200, "", "ok")
    })
    .await;
    let mut original = step(&fixture.destination, "/");
    original.request = "GET / HTTP/1.1\r\nAuthorization: Bearer {{token}}\r\n\r\n".into();
    let variables = vec![
        Variable {
            key: "token".into(),
            value: "{{literal}}".into(),
            secret: true,
            enabled: true,
        },
        Variable {
            key: "literal".into(),
            value: "must-not-expand".into(),
            secret: true,
            enabled: true,
        },
    ];
    let profiles = vec![campaign::access::Identity {
        name: "User B".into(),
        headers: "Authorization: Bearer {{token}}".into(),
    }];
    let (rows, message) = run(
        Workspace {
            kind: Kind::Access,
            steps: vec![original],
            profiles,
            anonymous: false,
            ..Default::default()
        },
        variables,
    )
    .await;
    assert_eq!(message, "Completed");
    assert_eq!(rows.len(), 2);
}
#[cfg(feature = "tls-spike")]
#[tokio::test]
async fn cancellation_during_tls_preparation_releases_sockets_and_completes() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let destination = format!("https://{}", listener.local_addr().unwrap());
    let workspace = Workspace {
        steps: vec![step(&destination, "/race")],
        burst_size: 2,
        connections: 2,
        timeout_secs: 300,
        ..Default::default()
    };
    let (jobs, job_rx) = mpsc::channel(1);
    let (updates, mut results) = mpsc::channel(2);
    let (stop, stop_rx) = watch::channel(false);
    let (cancel, cancel_rx) = watch::channel(false);
    let worker = tokio::spawn(campaign::run(job_rx, updates, Config::default(), stop_rx));
    jobs.send(Job {
        run: SavedRun {
            status: "Running".into(),
            id: 1,
            name: "cancel".into(),
            workspace,
            variables: vec![],
        },
        variables: vec![],
        scope: vec![],
        enforce_scope: false,
        global_rps: 0,
        host_rps: 0,
        cancel: cancel_rx,
    })
    .await
    .unwrap();
    let (mut socket, _) = tokio::time::timeout(Duration::from_secs(3), listener.accept())
        .await
        .unwrap()
        .unwrap();
    let mut hello = [0; 4096];
    assert!(socket.read(&mut hello).await.unwrap() > 0);
    cancel.send(true).unwrap();
    let update = tokio::time::timeout(Duration::from_secs(1), results.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(update,Update::Done{message,..} if message.contains("Cancelled")));
    let closed = tokio::time::timeout(Duration::from_secs(1), socket.read(&mut hello))
        .await
        .unwrap();
    assert!(matches!(closed, Ok(0) | Err(_)));
    stop.send(true).unwrap();
    worker.await.unwrap();
}
