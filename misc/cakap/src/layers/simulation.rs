use bytes::BytesMut;
use rand::Rng;
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};

use super::Layer;


#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CorruptionDirection {
    Send,
    Recv,
    Both,
}


pub struct Corruptor<T> {
    pub corruption_direction: CorruptionDirection,
    pub max_corruption_rate: f32,
    pub forward: T,
}


impl<T> Layer for Corruptor<T> where T: Layer<SendItem = BytesMut, RecvItem = BytesMut> {
    type SendError = T::SendError;
    type RecvError = T::RecvError;

    type SendItem = BytesMut;
    type RecvItem = BytesMut;

    async fn send(&mut self, mut data: Self::SendItem) -> Result<(), Self::SendError> {
        if self.corruption_direction == CorruptionDirection::Send || self.corruption_direction == CorruptionDirection::Both {
            let mut rng = rand::thread_rng();
            let bytes_to_corrupt = (data.len() as f32 * self.max_corruption_rate * rng.gen::<f32>()).round() as usize;
            for _ in 0..bytes_to_corrupt {
                let index = rng.gen_range(0..data.len());
                data[index] = rng.gen();
            }
        }
        self.forward.send(data).await
    }

    async fn recv(&mut self) -> Result<Self::RecvItem, Self::RecvError> {
        let mut data = self.forward.recv().await?;
        
        if self.corruption_direction == CorruptionDirection::Recv || self.corruption_direction == CorruptionDirection::Both {
            let mut rng = rand::thread_rng();
            let bytes_to_corrupt = (data.len() as f32 * self.max_corruption_rate * rng.gen::<f32>()).round() as usize;
            for _ in 0..bytes_to_corrupt {
                let index = rng.gen_range(0..data.len());
                data[index] = rng.gen();
            }
        }

        Ok(data)
    }
}


pub struct DuplexTransport {
    inner: DuplexStream
}


impl Layer for DuplexTransport {
    type SendError = std::io::Error;
    type RecvError = std::io::Error;

    type SendItem = BytesMut;
    type RecvItem = BytesMut;

    async fn send(&mut self, data: Self::SendItem) -> Result<(), Self::SendError> {
        self.inner.write_all(&data.len().to_ne_bytes()).await?;
        self.inner.write_all(&data).await
    }

    async fn recv(&mut self) -> Result<Self::RecvItem, Self::RecvError> {
        let mut len = 0usize.to_ne_bytes();
        self.inner.read_exact(&mut len).await?;
        let len = usize::from_ne_bytes(len);
        let mut buffer = BytesMut::with_capacity(len);
        buffer.resize(len, 0);
        self.inner.read_exact(&mut buffer).await?;
        Ok(buffer)
    }
}

pub fn duplex(max_buf_usize: usize) -> (DuplexTransport, DuplexTransport) {
    let (a, b) = tokio::io::duplex(max_buf_usize);
    (DuplexTransport { inner: a }, DuplexTransport { inner: b })
}