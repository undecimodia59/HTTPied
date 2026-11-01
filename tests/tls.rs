#![cfg(feature = "tls-spike")]
use httpied::{
    model::Event,
    proxy::{self, Config},
    tls::Intercept,
};
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{mpsc, watch},
};
use tokio_rustls::{
    TlsAcceptor, TlsConnector,
    rustls::{
        self,
        pki_types::{CertificateDer, PrivatePkcs8KeyDer, ServerName},
    },
};

fn roots(cert: Option<CertificateDer<'static>>) -> rustls::RootCertStore {
    let mut roots = rustls::RootCertStore::empty();
    if let Some(cert) = cert {
        roots.add(cert).unwrap();
    }
    roots
}
fn connector(cert: Option<CertificateDer<'static>>) -> TlsConnector {
    let mut cfg = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_root_certificates(roots(cert))
    .with_no_client_auth();
    cfg.alpn_protocols = vec![b"http/1.1".to_vec()];
    TlsConnector::from(Arc::new(cfg))
}

const REQUEST: &[u8] = b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n";
const RESPONSE: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\npong";

async fn scenario(case: &str) {
    tokio::time::timeout(Duration::from_secs(5), scenario_inner(case))
        .await
        .expect("TLS fixture exceeded total test deadline");
}
async fn scenario_inner(case: &str) {
    let backend = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let authority = backend.local_addr().unwrap().to_string();
    let mut params = rcgen::CertificateParams::new(vec![
        if case == "wrong-upstream-name" {
            "wrong.local"
        } else {
            "127.0.0.1"
        }
        .to_owned(),
    ])
    .unwrap();
    if case == "expired-upstream" {
        params.not_before = rcgen::date_time_ymd(2000, 1, 1);
        params.not_after = rcgen::date_time_ymd(2001, 1, 1);
    }
    let key = rcgen::KeyPair::generate().unwrap();
    let cert = params.self_signed(&key).unwrap();
    let server = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(
        vec![cert.der().clone()],
        PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
    )
    .unwrap();
    let upstream_roots = roots(if case == "untrusted-upstream" {
        None
    } else {
        Some(cert.der().clone())
    });
    let identity = Intercept::new(authority.clone(), upstream_roots).unwrap();
    let downstream = connector(if case == "untrusted-client" {
        None
    } else {
        Some(identity.ca_der.clone())
    });
    let cfg = Config {
        listen: "127.0.0.1:0".parse().unwrap(),
        tls: Some(Arc::new(identity)),
        connect_timeout: Duration::from_millis(300),
        ..Config::default()
    };
    let listener = proxy::bind(&cfg).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, mut rx) = mpsc::channel(2);
    let (stop, stop_rx) = watch::channel(false);
    let task = tokio::spawn(proxy::serve(listener, cfg, tx, stop_rx));
    let stall = case == "idle-upstream";
    let fixture = tokio::spawn(async move {
        let (mut socket, _) = backend.accept().await.unwrap();
        if stall {
            let mut bytes = Vec::new();
            let _ = socket.read_to_end(&mut bytes).await;
            return;
        }
        if let Ok(mut tls) = TlsAcceptor::from(Arc::new(server)).accept(socket).await {
            let mut buf = vec![0; REQUEST.len()];
            if tls.read_exact(&mut buf).await.is_ok() {
                assert_eq!(buf, REQUEST);
                tls.write_all(RESPONSE).await.unwrap();
                tls.shutdown().await.unwrap();
            }
        }
    });
    let mut socket = TcpStream::connect(addr).await.unwrap();
    socket
        .write_all(format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        let byte = socket.read_u8().await.unwrap();
        head.push(byte);
    }
    let upstream_failure = [
        "untrusted-upstream",
        "wrong-upstream-name",
        "expired-upstream",
        "idle-upstream",
    ]
    .contains(&case);
    if upstream_failure {
        assert!(head.starts_with(b"HTTP/1.1 502"));
    } else {
        assert!(head.starts_with(b"HTTP/1.1 200"));
        if case == "idle-client" {
            let mut data = Vec::new();
            socket.read_to_end(&mut data).await.unwrap();
        } else if case == "cancel" {
            stop.send(true).unwrap();
        } else {
            let name = ServerName::try_from(if case == "wrong-client-name" {
                "wrong.local"
            } else {
                "127.0.0.1"
            })
            .unwrap();
            match downstream.connect(name, socket).await {
                Ok(mut stream) => {
                    assert_eq!(case, "success");
                    assert_eq!(
                        stream.get_ref().1.alpn_protocol(),
                        Some(b"http/1.1".as_slice())
                    );
                    stream.write_all(REQUEST).await.unwrap();
                    let mut bytes = vec![0; RESPONSE.len()];
                    stream.read_exact(&mut bytes).await.unwrap();
                    assert_eq!(bytes, RESPONSE);
                    stream.shutdown().await.unwrap();
                }
                Err(_) => assert!(["wrong-client-name", "untrusted-client"].contains(&case)),
            }
        }
    }
    if case != "cancel" {
        let event = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .unwrap()
            .unwrap();
        match event {
            Event::InterceptedTls {
                uploaded,
                downloaded,
                ..
            } => {
                assert_eq!(case, "success");
                assert_eq!(uploaded, REQUEST.len() as u64);
                assert_eq!(downloaded, RESPONSE.len() as u64);
            }
            Event::Error(error) => {
                assert_ne!(case, "success", "{error}");
                assert!(error.contains("TLS"), "{error}");
            }
            event => panic!("unexpected {event:?}"),
        }
        stop.send(true).unwrap();
    }
    tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(1), fixture)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn trusted_tls_is_actively_terminated() {
    scenario("success").await;
}
#[tokio::test]
async fn upstream_trust_is_required() {
    scenario("untrusted-upstream").await;
}
#[tokio::test]
async fn upstream_hostname_is_verified() {
    scenario("wrong-upstream-name").await;
}
#[tokio::test]
async fn upstream_expiry_is_verified() {
    scenario("expired-upstream").await;
}
#[tokio::test]
async fn client_must_trust_generated_ca() {
    scenario("untrusted-client").await;
}
#[tokio::test]
async fn generated_leaf_has_correct_san() {
    scenario("wrong-client-name").await;
}
#[tokio::test]
async fn downstream_handshake_deadline() {
    scenario("idle-client").await;
}
#[tokio::test]
async fn upstream_handshake_deadline() {
    scenario("idle-upstream").await;
}
#[tokio::test]
async fn cancellation_during_handshake() {
    scenario("cancel").await;
}
