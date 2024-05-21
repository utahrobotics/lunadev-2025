use crate::layers::{simulation::duplex, Layer, sequenced::Sequenced};
use bytes::BytesMut;


#[tokio::test]
async fn test_duplex() {
    let (mut a, mut b) = duplex(1024);
    let data = BytesMut::from(&b"hello world"[..]);
    a.send(data.clone()).await.unwrap();
    let recv = b.recv().await.unwrap();
    assert_eq!(data, recv);
}


#[tokio::test]
async fn test_duplex_seq() {
    let (a, b) = duplex(1024);
    let mut a = Sequenced::new(Default::default(), a);
    let mut b = Sequenced::new(Default::default(), b);
    let data = BytesMut::from(&b"hello world"[..]);
    a.send(data.clone()).await.unwrap();
    let recv = b.recv().await.unwrap();
    assert_eq!(data, recv);
}