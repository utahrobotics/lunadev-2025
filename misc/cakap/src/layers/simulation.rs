use std::{collections::VecDeque, convert::Infallible, sync::Arc};

use bytes::BytesMut;
use crossbeam::queue::{ArrayQueue, SegQueue};
use rand::{rngs::SmallRng, Rng};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};

use super::Layer;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Send,
    Recv,
    Both,
}

pub enum RngVariant {
    ThreadRng,
    Seeded(SmallRng),
}

pub struct Corruptor<T> {
    pub direction: Direction,
    pub rng: RngVariant,
    pub min_corruption_rate: f32,
    pub max_corruption_rate: f32,
    pub forward: T,
}

impl<T> Layer for Corruptor<T>
where
    T: Layer<SendItem = BytesMut, RecvItem = BytesMut>,
{
    type SendError = T::SendError;
    type RecvError = T::RecvError;

    type SendItem = BytesMut;
    type RecvItem = BytesMut;

    async fn send(&mut self, mut data: Self::SendItem) -> Result<(), Self::SendError> {
        if self.direction == Direction::Send || self.direction == Direction::Both {
            match &mut self.rng {
                RngVariant::ThreadRng => {
                    let mut rng = rand::thread_rng();
                    let bytes_to_corrupt = (data.len() as f32
                        * rng.gen_range(self.min_corruption_rate..=self.max_corruption_rate))
                    .round() as usize;
                    for _ in 0..bytes_to_corrupt {
                        let index = rng.gen_range(0..data.len());
                        data[index] = rng.gen();
                    }
                }
                RngVariant::Seeded(rng) => {
                    let bytes_to_corrupt = (data.len() as f32
                        * rng.gen_range(self.min_corruption_rate..=self.max_corruption_rate))
                    .round() as usize;
                    for _ in 0..bytes_to_corrupt {
                        let index = rng.gen_range(0..data.len());
                        data[index] = rng.gen();
                    }
                }
            }
        }
        self.forward.send(data).await
    }

    async fn recv(&mut self) -> Result<Self::RecvItem, Self::RecvError> {
        let mut data = self.forward.recv().await?;

        if self.direction == Direction::Recv || self.direction == Direction::Both {
            match &mut self.rng {
                RngVariant::ThreadRng => {
                    let mut rng = rand::thread_rng();
                    let bytes_to_corrupt = (data.len() as f32
                        * rng.gen_range(self.min_corruption_rate..=self.max_corruption_rate))
                    .round() as usize;
                    for _ in 0..bytes_to_corrupt {
                        let index = rng.gen_range(0..data.len());
                        data[index] = rng.gen();
                    }
                }
                RngVariant::Seeded(rng) => {
                    let bytes_to_corrupt = (data.len() as f32
                        * rng.gen_range(self.min_corruption_rate..=self.max_corruption_rate))
                    .round() as usize;
                    for _ in 0..bytes_to_corrupt {
                        let index = rng.gen_range(0..data.len());
                        data[index] = rng.gen();
                    }
                }
            }
        }

        Ok(data)
    }

    #[inline(always)]
    fn get_max_packet_size(&self) -> usize {
        self.forward.get_max_packet_size()
    }
}

pub struct Dropper<T> {
    pub direction: Direction,
    pub rng: RngVariant,
    pub drop_rate: f32,
    pub forward: T,
}

impl<T> Layer for Dropper<T>
where
    T: Layer,
{
    type SendError = T::SendError;
    type RecvError = T::RecvError;

    type SendItem = T::SendItem;
    type RecvItem = T::RecvItem;

    async fn send(&mut self, data: Self::SendItem) -> Result<(), Self::SendError> {
        if self.direction == Direction::Send || self.direction == Direction::Both {
            match &mut self.rng {
                RngVariant::ThreadRng => {
                    let mut rng = rand::thread_rng();
                    if rng.gen::<f32>() < self.drop_rate {
                        return Ok(());
                    }
                }
                RngVariant::Seeded(rng) => {
                    if rng.gen::<f32>() < self.drop_rate {
                        return Ok(());
                    }
                }
            }
        }
        self.forward.send(data).await
    }

    async fn recv(&mut self) -> Result<Self::RecvItem, Self::RecvError> {
        loop {
            let data = self.forward.recv().await?;

            if self.direction == Direction::Recv || self.direction == Direction::Both {
                match &mut self.rng {
                    RngVariant::ThreadRng => {
                        let mut rng = rand::thread_rng();
                        if rng.gen::<f32>() < self.drop_rate {
                            continue;
                        }
                    }
                    RngVariant::Seeded(rng) => {
                        if rng.gen::<f32>() < self.drop_rate {
                            continue;
                        }
                    }
                }
            }

            break Ok(data);
        }
    }

    #[inline(always)]
    fn get_max_packet_size(&self) -> usize {
        self.forward.get_max_packet_size()
    }
}

pub struct Skip<T> {
    pub direction: Direction,
    pub skip_rate: f32,
    send_skipped: usize,
    send_total: usize,
    recv_skipped: usize,
    recv_total: usize,
    pub forward: T,
}

impl<T> Skip<T> {
    #[inline(always)]
    pub fn new(direction: Direction, skip_rate: f32, forward: T) -> Self {
        Self {
            direction,
            skip_rate,
            send_skipped: 0,
            send_total: 0,
            recv_skipped: 0,
            recv_total: 0,
            forward,
        }
    }
}

impl<T> Layer for Skip<T>
where
    T: Layer,
{
    type SendError = T::SendError;
    type RecvError = T::RecvError;

    type SendItem = T::SendItem;
    type RecvItem = T::RecvItem;

    async fn send(&mut self, data: Self::SendItem) -> Result<(), Self::SendError> {
        self.send_total += 1;
        if self.direction == Direction::Send || self.direction == Direction::Both {
            if self.send_skipped as f32 / (self.send_total as f32) < self.skip_rate {
                self.send_skipped += 1;
                return Ok(());
            }
        }
        self.forward.send(data).await
    }

    async fn recv(&mut self) -> Result<Self::RecvItem, Self::RecvError> {
        loop {
            self.recv_total += 1;
            let data = self.forward.recv().await?;

            if self.direction == Direction::Recv || self.direction == Direction::Both {
                if self.recv_skipped as f32 / (self.recv_total as f32) < self.skip_rate {
                    self.recv_skipped += 1;
                    continue;
                }
            }

            break Ok(data);
        }
    }

    #[inline(always)]
    fn get_max_packet_size(&self) -> usize {
        self.forward.get_max_packet_size()
    }
}

// pub struct Delay<T> {
//     pub direction: Direction,
//     pub min_delay_msecs: usize,
//     pub max_delay_msecs: usize,
//     pub rng: RngVariant,
//     pub forward: T,
// }

// impl<T> Layer for Delay<T>
// where
//     T: Layer,
// {
//     type SendError = T::SendError;
//     type RecvError = T::RecvError;

//     type SendItem = T::SendItem;
//     type RecvItem = T::RecvItem;

//     async fn send(&mut self, data: Self::SendItem) -> Result<(), Self::SendError> {
//         if self.direction == Direction::Send || self.direction == Direction::Both {
//             let delay_msecs = match &mut self.rng {
//                 RngVariant::ThreadRng => {
//                     rand::thread_rng().gen_range(self.min_delay_msecs..=self.max_delay_msecs)
//                 }
//                 RngVariant::Seeded(rng) => {
//                     rng.gen_range(self.min_delay_msecs..=self.max_delay_msecs)
//                 }
//             };
//             tokio::time::sleep(std::time::Duration::from_millis(delay_msecs as u64)).await;
//         }
//         self.forward.send(data).await
//     }

//     async fn recv(&mut self) -> Result<Self::RecvItem, Self::RecvError> {
//         if self.direction == Direction::Recv || self.direction == Direction::Both {
//             let delay_msecs = match &mut self.rng {
//                 RngVariant::ThreadRng => {
//                     rand::thread_rng().gen_range(self.min_delay_msecs..=self.max_delay_msecs)
//                 }
//                 RngVariant::Seeded(rng) => {
//                     rng.gen_range(self.min_delay_msecs..=self.max_delay_msecs)
//                 }
//             };
//             tokio::time::sleep(std::time::Duration::from_millis(delay_msecs as u64)).await;
//         }
//         self.forward.recv().await
//     }

//     #[inline(always)]
//     fn get_max_packet_size(&self) -> usize {
//         self.forward.get_max_packet_size()
//     }
// }

pub struct DuplexTransport {
    inner: DuplexStream,
    max_buf_usize: usize,
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

    #[inline(always)]
    fn get_max_packet_size(&self) -> usize {
        self.max_buf_usize
    }
}

impl DuplexTransport {
    pub fn drain_drop(mut self) {
        tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            loop {
                if self.inner.read(&mut buf).await.is_err() {
                    return;
                }
            }
        });
    }
}

pub fn duplex(max_buf_usize: usize) -> (DuplexTransport, DuplexTransport) {
    let (a, b) = tokio::io::duplex(max_buf_usize.saturating_add(std::mem::size_of::<usize>()));
    (
        DuplexTransport {
            inner: a,
            max_buf_usize,
        },
        DuplexTransport {
            inner: b,
            max_buf_usize,
        },
    )
}

pub struct NoMorePackets;

impl<T> Layer for Vec<T> {
    type SendError = Infallible;
    type RecvError = NoMorePackets;

    type SendItem = T;
    type RecvItem = T;

    #[inline(always)]
    async fn send(&mut self, data: Self::SendItem) -> Result<(), Self::SendError> {
        self.push(data);
        Ok(())
    }

    #[inline(always)]
    async fn recv(&mut self) -> Result<Self::RecvItem, Self::RecvError> {
        self.pop().ok_or(NoMorePackets)
    }

    #[inline(always)]
    fn get_max_packet_size(&self) -> usize {
        usize::MAX
    }
}

impl<T> Layer for VecDeque<T> {
    type SendError = Infallible;
    type RecvError = NoMorePackets;

    type SendItem = T;
    type RecvItem = T;

    #[inline(always)]
    async fn send(&mut self, data: Self::SendItem) -> Result<(), Self::SendError> {
        self.push_back(data);
        Ok(())
    }

    #[inline(always)]
    async fn recv(&mut self) -> Result<Self::RecvItem, Self::RecvError> {
        self.pop_front().ok_or(NoMorePackets)
    }

    #[inline(always)]
    fn get_max_packet_size(&self) -> usize {
        usize::MAX
    }
}

impl<T> Layer for SegQueue<T> {
    type SendError = Infallible;
    type RecvError = NoMorePackets;

    type SendItem = T;
    type RecvItem = T;

    #[inline(always)]
    async fn send(&mut self, data: Self::SendItem) -> Result<(), Self::SendError> {
        self.push(data);
        Ok(())
    }

    #[inline(always)]
    async fn recv(&mut self) -> Result<Self::RecvItem, Self::RecvError> {
        self.pop().ok_or(NoMorePackets)
    }

    #[inline(always)]
    fn get_max_packet_size(&self) -> usize {
        usize::MAX
    }
}

pub struct NoSpace<T>(pub T);

impl<T> Layer for ArrayQueue<T> {
    type SendError = NoSpace<T>;
    type RecvError = NoMorePackets;

    type SendItem = T;
    type RecvItem = T;

    #[inline(always)]
    async fn send(&mut self, data: Self::SendItem) -> Result<(), Self::SendError> {
        self.push(data).map_err(NoSpace)
    }

    #[inline(always)]
    async fn recv(&mut self) -> Result<Self::RecvItem, Self::RecvError> {
        self.pop().ok_or(NoMorePackets)
    }

    #[inline(always)]
    fn get_max_packet_size(&self) -> usize {
        usize::MAX
    }
}

impl<T> Layer for Arc<SegQueue<T>> {
    type SendError = Infallible;
    type RecvError = NoMorePackets;

    type SendItem = T;
    type RecvItem = T;

    #[inline(always)]
    async fn send(&mut self, data: Self::SendItem) -> Result<(), Self::SendError> {
        self.push(data);
        Ok(())
    }

    #[inline(always)]
    async fn recv(&mut self) -> Result<Self::RecvItem, Self::RecvError> {
        self.pop().ok_or(NoMorePackets)
    }

    #[inline(always)]
    fn get_max_packet_size(&self) -> usize {
        usize::MAX
    }
}

impl<T> Layer for Arc<ArrayQueue<T>> {
    type SendError = NoSpace<T>;
    type RecvError = NoMorePackets;

    type SendItem = T;
    type RecvItem = T;

    #[inline(always)]
    async fn send(&mut self, data: Self::SendItem) -> Result<(), Self::SendError> {
        self.push(data).map_err(NoSpace)
    }

    #[inline(always)]
    async fn recv(&mut self) -> Result<Self::RecvItem, Self::RecvError> {
        self.pop().ok_or(NoMorePackets)
    }

    #[inline(always)]
    fn get_max_packet_size(&self) -> usize {
        usize::MAX
    }
}

pub fn segqueue_transport<T>() -> (Arc<SegQueue<T>>, Arc<SegQueue<T>>) {
    let a = Arc::new(SegQueue::new());
    let b = a.clone();
    (a, b)
}

pub fn arrayqueue_transport<T>(capacity: usize) -> (Arc<ArrayQueue<T>>, Arc<ArrayQueue<T>>) {
    let a = Arc::new(ArrayQueue::new(capacity));
    let b = a.clone();
    (a, b)
}
