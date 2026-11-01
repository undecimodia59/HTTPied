use futures_util::{SinkExt, StreamExt};
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
#[tokio::test]
async fn websocket_messages_are_forwarded_and_captured_in_both_directions() {
    tokio::time::timeout(Duration::from_secs(5),async{
        let origin=TcpListener::bind("127.0.0.1:0").await.unwrap();let target=origin.local_addr().unwrap();
        let fixture=tokio::spawn(async move{
            let (stream,_)=origin.accept().await.unwrap();let mut ws=tokio_tungstenite::accept_async(stream).await.unwrap();
            let message=ws.next().await.unwrap().unwrap();assert_eq!(message.to_text().unwrap(),"hello");ws.send(message).await.unwrap();
            let _=ws.next().await;
        });
        let cfg=Config{listen:"127.0.0.1:0".parse().unwrap(),..Config::default()};let listener=proxy::bind(&cfg).await.unwrap();let address=listener.local_addr().unwrap();
        let (tx,mut rx)=mpsc::channel(16);let (stop,shutdown)=watch::channel(false);let serving=tokio::spawn(proxy::serve(listener,cfg,tx,shutdown));
        let mut client=TcpStream::connect(address).await.unwrap();
        let key=tokio_tungstenite::tungstenite::handshake::client::generate_key();
        client.write_all(format!("GET http://{target}/socket HTTP/1.1\r\nHost: {target}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n").as_bytes()).await.unwrap();
        let mut head=Vec::new();while !head.ends_with(b"\r\n\r\n"){head.push(client.read_u8().await.unwrap());}assert!(head.starts_with(b"HTTP/1.1 101"),"{}",String::from_utf8_lossy(&head));
        let mut ws=tokio_tungstenite::WebSocketStream::from_raw_socket(client,tokio_tungstenite::tungstenite::protocol::Role::Client,None).await;
        ws.send(tokio_tungstenite::tungstenite::Message::Text("hello".into())).await.unwrap();assert_eq!(ws.next().await.unwrap().unwrap().to_text().unwrap(),"hello");
        let mut directions=Vec::new();for _ in 0..3{if let Event::WebSocket{direction,payload,..}=rx.recv().await.unwrap(){assert_eq!(payload,b"hello");directions.push(direction);}}
        assert_eq!(directions,vec!["client → server","server → client"]);
        let _=ws.close(None).await;stop.send(true).unwrap();serving.await.unwrap().unwrap();fixture.await.unwrap();
    }).await.unwrap();
}
