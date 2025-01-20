use crc::Crc;

/// The CRC used by UART communication. 
/// Reverse engineered using CRC RevEng https://sourceforge.net/projects/reveng/
const CRC_GENERATOR: Crc<u16> = Crc::<u16>::new(&crc::Algorithm {
    width: 16,
    poly: 0x1021,
    init: 0x0205,
    refin: false,
    refout: false,
    xorout: 0x0000,
    check: 0xbf1a,
    residue: 0x0000
});

#[derive(Clone, Copy, Debug)]
pub struct SetDutyCycle(pub f32);

impl Payload for SetDutyCycle {
    fn len(&self) -> usize {
        5
    }
    fn append_to(&self, buffer: &mut Vec<u8>) {
        buffer.push(5);
        buffer.extend_from_slice(&scale_and_pack(self.0, 100_000_f32).to_be_bytes());
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Alive;

impl Payload for Alive {
    fn len(&self) -> usize {
        1
    }
    fn append_to(&self, buffer: &mut Vec<u8>) {
        buffer.push(30);
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CanForwarded<T> {
    pub can_id: u8,
    pub payload: T
}

impl<T: Payload> Payload for CanForwarded<T> {
    fn len(&self) -> usize {
        2 + self.payload.len()
    }
    fn append_to(&self, buffer: &mut Vec<u8>) {
        buffer.push(34);
        buffer.push(self.can_id);
        self.payload.append_to(buffer);
    }
}

pub trait Payload {
    fn len(&self) -> usize;
    fn append_to(&self, buffer: &mut Vec<u8>);
}

#[derive(Default)]
pub struct VescPacker {
    buffer: Vec<u8>,
}

impl VescPacker {
    pub fn pack<'a>(&'a mut self, payload: &impl Payload) -> &'a [u8] {
        self.buffer.clear();
        if payload.len() > 255 {
            self.buffer.reserve(3 + payload.len() + 3);
            self.buffer.push(3);
            self.buffer.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        } else {
            self.buffer.reserve(2 + payload.len() + 3);
            self.buffer.push(2);
            self.buffer.push(payload.len() as u8);
        }
        payload.append_to(&mut self.buffer);
        self.buffer.extend_from_slice(&CRC_GENERATOR.checksum(&self.buffer).to_be_bytes());
        self.buffer.push(3);
        &self.buffer
    }
}

// Helpers
/// Scales something by an number, and then converts it to a u32, first truncating
/// but keeping sign, then converting by byte to a u32.
/// 
/// data - The data being packed.
/// scale - The scale factor being used.
fn scale_and_pack(data: f32, scale: f32) -> u32 {
    (data * scale) as i32 as u32
}