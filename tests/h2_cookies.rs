use httpied::{model::Event, proxy::Config};
use std::{collections::BTreeMap, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::mpsc,
};

#[tokio::test]
async fn split_http2_cookies_reach_strict_http1_origin_as_one_field() {
    tokio::time::timeout(Duration::from_secs(5),async {
        for values in [vec![],vec!["session=one"],vec!["session=one","theme=dark"],vec!["id=first; token=a%3Bb","id=second","quoted=\"a,b\""]] {
            let listener=TcpListener::bind("127.0.0.1:0").await.unwrap();
            let authority=listener.local_addr().unwrap().to_string();
            let upstream=TcpStream::connect(&authority).await.unwrap();
            let fixture=tokio::spawn(async move {
                let (mut stream,_)=listener.accept().await.unwrap();
                let mut head=Vec::new();
                while !head.ends_with(b"\r\n\r\n") {head.push(stream.read_u8().await.unwrap());}
                let count=head.split(|b|*b==b'\n').filter(|line|line.get(..7).is_some_and(|prefix|prefix.eq_ignore_ascii_case(b"cookie:"))).count();
                if count>1 {
                    let body=b"{\"error\":\"Duplicate header names are not allowed\"}";
                    stream.write_all(format!("HTTP/1.1 400 Bad Request\r\nContent-Length: {}\r\n\r\n",body.len()).as_bytes()).await.unwrap();stream.write_all(body).await.unwrap();
                }else{
                    stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nSet-Cookie: first=1; Path=/\r\nSet-Cookie: second=2; Path=/\r\n\r\nok").await.unwrap();
                }
                stream.shutdown().await.unwrap();head
            });
            let (client,server)=tokio::io::duplex(64*1024);let (events,mut captured)=mpsc::channel(1);
            let cfg=Config{events:Some(events),..Default::default()};
            let serving=tokio::spawn(httpied::protocols::h2(server,authority.clone(),cfg,Some(Box::new(upstream))));
            let (mut sender,driver)=h2::client::handshake(client).await.unwrap();let driver=tokio::spawn(driver);
            let mut request=http::Request::builder().uri(format!("https://{authority}/cookies"));
            for value in &values {request=request.header("cookie",*value);}
            let request=request.header("x-repeat","first").header("x-repeat","second").body(()).unwrap();
            let (response,_)=sender.send_request(request,true).unwrap();let response=response.await.unwrap();
            assert_eq!(response.status(),200,"split HTTP/2 cookies must be joined before strict HTTP/1 forwarding");
            assert_eq!(response.headers().get_all("set-cookie").iter().count(),2);
            let mut body=response.into_body();let mut bytes=Vec::new();
            while let Some(chunk)=body.data().await {let chunk=chunk.unwrap();bytes.extend_from_slice(&chunk);body.flow_control().release_capacity(chunk.len()).unwrap();}
            assert_eq!(bytes,b"ok");
            let wire=fixture.await.unwrap();let mut fields:BTreeMap<String,Vec<String>>=BTreeMap::new();
            for line in std::str::from_utf8(&wire).unwrap().split("\r\n").skip(1) {
                if let Some((name,value))=line.split_once(':') {fields.entry(name.to_ascii_lowercase()).or_default().push(value.trim_start().into());}
            }
            if values.is_empty(){assert!(!fields.contains_key("cookie"));}else{assert_eq!(fields["cookie"],vec![values.join("; ")]);}
            assert_eq!(fields["host"],vec![authority]);assert_eq!(fields["x-repeat"],vec!["first","second"]);
            let Event::Exchange(capture)=captured.recv().await.unwrap() else {panic!("expected capture")};
            assert_eq!(capture.outgoing,wire);assert!(capture.original.0.is_empty());
            assert_eq!(capture.transformations.iter().any(|note|note.contains("Cookie fields joined")),values.len()>1);
            drop(sender);driver.abort();serving.abort();let _=serving.await;
        }
    }).await.unwrap();
}
