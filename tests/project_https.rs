#![cfg(feature = "tls-spike")]
use httpied::{
    authority::Authority,
    model::Event,
    proxy::{self, Config},
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
        pki_types::{PrivatePkcs8KeyDer, ServerName},
    },
};

#[tokio::test]
async fn general_https_captures_decrypted_http_and_replays_with_verified_tls() {
    #[cfg(feature = "debug-logging")]
    let _diagnostics =
        std::env::var_os("HTTPIED_TEST_DEBUG_LOGS").map(|_| httpied::diagnostics::start().unwrap());
    tokio::time::timeout(Duration::from_secs(8),async{
        let backend=TcpListener::bind("127.0.0.1:0").await.unwrap();let address=backend.local_addr().unwrap();let authority=format!("localhost:{}",address.port());
        let params=rcgen::CertificateParams::new(vec!["127.0.0.1".into(),"localhost".into()]).unwrap();let key=rcgen::KeyPair::generate().unwrap();let cert=params.self_signed(&key).unwrap();
        let provider=Arc::new(rustls::crypto::ring::default_provider());
        let server=rustls::ServerConfig::builder_with_provider(provider.clone()).with_safe_default_protocol_versions().unwrap().with_no_client_auth().with_single_cert(vec![cert.der().clone()],PrivatePkcs8KeyDer::from(key.serialize_der()).into()).unwrap();
        let mut roots=rustls::RootCertStore::empty();roots.add(cert.der().clone()).unwrap();
        let mut params=rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();params.is_ca=rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        let key=rcgen::KeyPair::generate().unwrap();let ca_cert=params.self_signed(&key).unwrap();
        let ca=Arc::new(Authority::with_roots(rcgen::Issuer::new(params,key),ca_cert.der().clone(),roots).unwrap());
        let mut roots=rustls::RootCertStore::empty();roots.add(ca_cert.der().clone()).unwrap();
        let client=rustls::ClientConfig::builder_with_provider(provider).with_safe_default_protocol_versions().unwrap().with_root_certificates(roots).with_no_client_auth();
        let cfg=Config{listen:"127.0.0.1:0".parse().unwrap(),ca:Some(ca),..Config::default()};
        let listener=proxy::bind(&cfg).await.unwrap();let proxy_address=listener.local_addr().unwrap();
        let (tx,mut rx)=mpsc::channel(8);let (stop,shutdown)=watch::channel(false);let task=tokio::spawn(proxy::serve(listener,cfg.clone(),tx,shutdown));
        let fixture=tokio::spawn(async move{for index in 0..11 {
            let (stream,_)=backend.accept().await.unwrap();let mut stream=TlsAcceptor::from(Arc::new(server.clone())).accept(stream).await.unwrap();
            let mut head=Vec::new();while !head.ends_with(b"\r\n\r\n"){head.push(stream.read_u8().await.unwrap());}
            assert!(head.starts_with(b"GET /hello HTTP/1.1\r\n"));
            if index>=2 {
                let mut headers=[httparse::EMPTY_HEADER;128];let mut request=httparse::Request::new(&mut headers);request.parse(&head).unwrap();
                let cookies=request.headers.iter().filter(|header|header.name.eq_ignore_ascii_case("cookie")).collect::<Vec<_>>();
                assert_eq!(cookies.len(),1,"verified TLS origin must receive one Cookie field");
                assert_eq!(cookies[0].value,b"session=fixture; theme=dark");
            }
            if index==2 {
                stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",3*1024*1024).as_bytes()).await.unwrap();
                for _ in 0..3 {stream.write_all(&vec![b'x';1024*1024]).await.unwrap();}
            }else if index==1 {
                let mut encoder=flate2::write::GzEncoder::new(Vec::new(),flate2::Compression::default());
                std::io::Write::write_all(&mut encoder,b"window.fixture = 'verified HTTPS JavaScript';").unwrap();
                let compressed=encoder.finish().unwrap();
                stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/javascript\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\n\r\n",compressed.len()).as_bytes()).await.unwrap();
                stream.write_all(&compressed).await.unwrap();
            }else if index>=3{stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok").await.unwrap();}else{stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\n\r\n{\"ok\":true}").await.unwrap();}
            stream.shutdown().await.unwrap();
        }});
        let mut stream=TcpStream::connect(proxy_address).await.unwrap();stream.write_all(format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n\r\n").as_bytes()).await.unwrap();
        let mut head=Vec::new();while !head.ends_with(b"\r\n\r\n"){head.push(stream.read_u8().await.unwrap());}assert!(head.starts_with(b"HTTP/1.1 200"));
        let mut stream=TlsConnector::from(Arc::new(client.clone())).connect(ServerName::try_from("localhost").unwrap(),stream).await.unwrap();stream.write_all(format!("GET /hello HTTP/1.1\r\nHost: {authority}\r\n\r\n").as_bytes()).await.unwrap();let mut response=Vec::new();stream.read_to_end(&mut response).await.unwrap();assert!(response.ends_with(b"{\"ok\":true}"));
        let Event::Exchange(capture)=rx.recv().await.unwrap() else{panic!("expected HTTP capture")};assert!(capture.parsed.target.starts_with("https://"));assert_eq!(capture.response_mime,"application/json");assert!(capture.first_byte_ms<=capture.elapsed_ms);
        let request=httpied::replay::executed_request(&capture).unwrap();let result=proxy::execute(request,&cfg).await.unwrap();assert_eq!(result.response_status,200);
        assert!(httpied::exchange::needs_decoding(&result.response));
        assert!(httpied::exchange::decoded_message(&result.response).unwrap().contains("verified HTTPS JavaScript"));
        let split=result.response.windows(4).position(|w|w==b"\r\n\r\n").unwrap()+4;assert_eq!(&result.response[split..split+2],&[0x1f,0x8b]);
        let mut stream=TcpStream::connect(proxy_address).await.unwrap();stream.write_all(format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n\r\n").as_bytes()).await.unwrap();
        let mut head=Vec::new();while !head.ends_with(b"\r\n\r\n"){head.push(stream.read_u8().await.unwrap());}assert!(head.starts_with(b"HTTP/1.1 200"));
        let mut h2_client=client;h2_client.alpn_protocols=vec![b"h2".to_vec()];
        let stream=TlsConnector::from(Arc::new(h2_client)).connect(ServerName::try_from("localhost").unwrap(),stream).await.unwrap();
        assert_eq!(stream.get_ref().1.alpn_protocol(),Some(b"h2".as_slice()));
        let (mut sender,connection)=h2::client::handshake(stream).await.unwrap();let driver=tokio::spawn(connection);
        let request=http::Request::builder().uri(format!("https://{authority}/hello")).header("cookie","session=fixture").header("cookie","theme=dark").body(()).unwrap();
        let (response,_)=sender.send_request(request,true).unwrap();let response=response.await.unwrap();assert_eq!(response.status(),200);
        let mut body=response.into_body();let mut data=Vec::new();while let Some(chunk)=body.data().await{let chunk=chunk.unwrap();data.extend_from_slice(&chunk);body.flow_control().release_capacity(chunk.len()).unwrap();}
        assert_eq!(data,vec![b'x';3*1024*1024]);
        let Event::Exchange(capture)=rx.recv().await.unwrap() else{panic!("expected HTTP/2 capture")};assert!(capture.protocol.starts_with("HTTP/2"));assert!(capture.original.0.is_empty());assert!(capture.response_truncated);assert!(capture.response.len()<=1024*1024);
        assert!(capture.transformations.iter().any(|note|note.contains("Cookie fields joined")));
        let mut responses=Vec::new();
        for _ in 0..8 {
            let request=http::Request::builder().uri(format!("https://{authority}/hello")).header("cookie","session=fixture").header("cookie","theme=dark").body(()).unwrap();
            let (response,_)=sender.send_request(request,true).unwrap();responses.push(response);
        }
        for response in responses {
            let response=response.await.unwrap();assert_eq!(response.status(),200);
            let mut body=response.into_body();let mut data=Vec::new();
            while let Some(chunk)=body.data().await {let chunk=chunk.unwrap();data.extend_from_slice(&chunk);body.flow_control().release_capacity(chunk.len()).unwrap();}
            assert_eq!(data,b"ok");
        }
        for _ in 0..8 {assert!(matches!(rx.recv().await,Some(Event::Exchange(_))));}
        drop(sender);driver.abort();
        stop.send(true).unwrap();task.await.unwrap().unwrap();fixture.await.unwrap();
    }).await.unwrap();
}

#[tokio::test]
async fn upstream_tls_eof_identifies_handshake_and_host() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let fixture = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut hello = [0; 4096];
            assert!(stream.read(&mut hello).await.unwrap() > 0);
            stream.shutdown().await.unwrap();
        });
        let stream = TcpStream::connect(address).await.unwrap();
        let error = Authority::connect_public("localhost", stream, Duration::from_secs(2))
            .await
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("Upstream TLS handshake [localhost]"),
            "{error}"
        );
        assert!(
            matches!(
                error.kind(),
                std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::ConnectionReset
            ),
            "{error:?}"
        );
        fixture.await.unwrap();
    })
    .await
    .unwrap();
}
