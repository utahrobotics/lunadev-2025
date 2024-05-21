use crate::layers::{
    ecc::ECC,
    sequenced::Sequenced,
    simulation::{duplex, Corruptor, Direction, RngVariant},
    Layer,
};
use bytes::BytesMut;
use rand::{rngs::SmallRng, SeedableRng};

const BIG_DATA: &[u8] = &[0; 9212];

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

#[tokio::test]
async fn test_duplex_ecc() {
    let (a, b) = duplex(1024);
    let a = Corruptor {
        forward: a,
        direction: Direction::Send,
        min_corruption_rate: 0.12,
        max_corruption_rate: 0.12,
        rng: RngVariant::Seeded(SmallRng::seed_from_u64(4557)),
    };
    let mut a = ECC::new(0.4, a);
    let mut b = ECC::new(0.4, b);
    let data = BytesMut::from(BIG_DATA);
    let (_, recv) = tokio::join!(
        async {
            a.send(data.clone()).await.unwrap();
        },
        async { b.recv().await.unwrap() },
    );
    assert_eq!(data, recv);
}
