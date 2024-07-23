use std::net::{Ipv4Addr, SocketAddrV4};

use bytes::BytesMut;
use cakap::Connection;

#[tokio::main]
async fn main() {
    let handle = tokio::spawn(async {
        let conn = Connection::connect(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 54200), 0, 2048)
            .await
            .unwrap();
        let stream = conn
            .open_unordered_unreliable_stream::<[u8]>(0)
            .await
            .unwrap();
        stream.send(b"hello").await.unwrap();
        let mut bytes = BytesMut::new();
        stream.recv(&mut bytes).await.unwrap();
        println!("client: {}", std::str::from_utf8(&bytes).unwrap());
    });
    let mut conn = Connection::bind(54200, 2048).await.unwrap();
    let stream = conn.accept_stream::<[u8]>(0).await.unwrap();
    let mut bytes = BytesMut::new();
    stream.recv(&mut bytes).await.unwrap();
    println!("server: {}", std::str::from_utf8(&bytes).unwrap());
    stream.send(&bytes).await.unwrap();
    handle.await.unwrap();
}
