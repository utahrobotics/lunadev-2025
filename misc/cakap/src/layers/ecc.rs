use std::hash::BuildHasherDefault;

use bytes::BytesMut;
use fxhash::FxHashMap;
use parking_lot::{RwLock, RwLockWriteGuard};
use reed_solomon::{Decoder, Encoder};

use super::{reliable::{HasReliableGuard, ReliableToken}, Layer};

static REED_SOLOMON_ENCODERS: RwLock<FxHashMap<usize, Encoder>> =
    RwLock::new(FxHashMap::with_hasher(BuildHasherDefault::new()));

static REED_SOLOMON_DECODERS: RwLock<FxHashMap<usize, Decoder>> =
    RwLock::new(FxHashMap::with_hasher(BuildHasherDefault::new()));

pub struct ECC<T> {
    pub forward: T,
    ecc_frac: f32,
    ecc_len: usize,
}

impl<T> ECC<T> {
    pub fn new(ecc_frac: f32, forward: T) -> Self {
        ECC {
            forward,
            ecc_frac,
            ecc_len: (ecc_frac * 255.0).round() as usize,
        }
    }

    pub fn map<V>(self, new: V) -> ECC<V> {
        ECC {
            forward: new,
            ecc_frac: self.ecc_frac,
            ecc_len: self.ecc_len,
        }
    }
}

#[derive(Debug)]
pub enum ECCRecvError<E> {
    TooCorrupted,
    ForwardError(E),
}

impl<E> From<E> for ECCRecvError<E> {
    fn from(e: E) -> Self {
        ECCRecvError::ForwardError(e)
    }
}

impl<T> Layer for ECC<T>
where
    T: Layer<SendItem = BytesMut, RecvItem = BytesMut>,
{
    type SendError = T::SendError;
    type RecvError = ECCRecvError<T::RecvError>;

    type SendItem = BytesMut;
    type RecvItem = BytesMut;

    async fn send(&mut self, data: Self::SendItem) -> Result<(), Self::SendError> {
        let mut reader;
        let mut encoder = 'get: {
            reader = REED_SOLOMON_ENCODERS.read();
            if let Some(encoder) = reader.get(&self.ecc_len) {
                break 'get encoder;
            }
            drop(reader);
            let encoder = Encoder::new(self.ecc_len);
            REED_SOLOMON_ENCODERS.write().insert(self.ecc_len, encoder);
            reader = REED_SOLOMON_ENCODERS.read();
            break 'get reader.get(&self.ecc_len).unwrap();
        };
        let max_payload = 255 - self.ecc_len;
        let mut out =
            BytesMut::with_capacity((data.len() as f32 * (1.0 + self.ecc_frac)).ceil() as usize);
        let mut iter = data.chunks_exact(max_payload);
        while let Some(fragment) = iter.next() {
            let ecc = encoder.encode(fragment);
            out.extend_from_slice(&ecc);
        }

        if !iter.remainder().is_empty() {
            let ecc_len = (iter.remainder().len() as f32 * self.ecc_frac).ceil() as usize;
            encoder = 'get: {
                if let Some(encoder) = reader.get(&ecc_len) {
                    break 'get encoder;
                }
                drop(reader);
                let encoder = Encoder::new(ecc_len);
                REED_SOLOMON_ENCODERS.write().insert(ecc_len, encoder);
                reader = REED_SOLOMON_ENCODERS.read();
                break 'get reader.get(&ecc_len).unwrap();
            };
            let ecc = encoder.encode(iter.remainder());
            out.extend_from_slice(&ecc);
        }

        self.forward.send(out).await
    }

    async fn recv(&mut self) -> Result<Self::RecvItem, Self::RecvError> {
        let data = self.forward.recv().await?;
        let mut reader;
        let mut decoder = 'get: {
            reader = REED_SOLOMON_DECODERS.read();
            if let Some(decoder) = reader.get(&self.ecc_len) {
                break 'get decoder;
            }
            drop(reader);
            let decoder = Decoder::new(self.ecc_len);
            let mut writer = REED_SOLOMON_DECODERS.write();
            writer.insert(self.ecc_len, decoder);
            reader = RwLockWriteGuard::downgrade(writer);
            reader.get(&self.ecc_len).unwrap()
        };

        let mut out =
            BytesMut::with_capacity((data.len() as f32 / (1.0 + self.ecc_frac)).ceil() as usize);
        let mut iter = data.chunks_exact(255);
        while let Some(fragment) = iter.next() {
            let corrected = decoder
                .correct(&fragment, None)
                .map_err(|_| ECCRecvError::TooCorrupted)?;
            out.extend_from_slice(corrected.data());
        }

        if !iter.remainder().is_empty() {
            let ecc_len = (iter.remainder().len() as f32 * (1.0 - 1.0 / (1.0 + self.ecc_frac)))
                .ceil() as usize;
            decoder = 'get: {
                if let Some(decoder) = reader.get(&ecc_len) {
                    break 'get decoder;
                }
                drop(reader);
                let decoder = Decoder::new(ecc_len);
                let mut writer = REED_SOLOMON_DECODERS.write();
                writer.insert(ecc_len, decoder);
                reader = RwLockWriteGuard::downgrade(writer);
                reader.get(&ecc_len).unwrap()
            };
            let corrected = decoder
                .correct(iter.remainder(), None)
                .map_err(|_| ECCRecvError::TooCorrupted)?;
            out.extend_from_slice(corrected.data());
        }

        Ok(out)
    }
}


impl<T: HasReliableGuard> HasReliableGuard for ECC<T> {
    #[inline(always)]
    async fn reliable_guard_send(
        &mut self,
        data: BytesMut,
        token: ReliableToken,
    ) {
        self.forward.reliable_guard_send(data, token).await
    }
}
