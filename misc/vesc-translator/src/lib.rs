#![no_std]

use core::iter::once;

use crc16::{State, XMODEM};
use heapless::Vec;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SetDutyCycle(pub f32);

impl Payload for SetDutyCycle {
    const LEN: usize = 5;

    fn append_to(&self, buffer: &mut impl Extend<u8>) {
        buffer.extend(once(5u8.to_be()));
        buffer.extend(scale_and_pack(self.0, 100_000_f32).to_be_bytes());
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Alive;

impl Payload for Alive {
    const LEN: usize = 1;

    fn append_to(&self, buffer: &mut impl Extend<u8>) {
        buffer.extend(once(30u8.to_be()));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanForwarded<T> {
    pub can_id: u8,
    pub payload: T
}

impl<T: Payload> Payload for CanForwarded<T> {
    const LEN: usize = 2 + T::LEN;

    fn append_to(&self, buffer: &mut impl Extend<u8>) {
        buffer.extend([34u8.to_be(), self.can_id.to_be()]);
        self.payload.append_to(buffer);
    }
}

impl<T: Getter> Getter for CanForwarded<T> {
    type Input = T::Input;
    type Response = T::Response;

    fn parse_response(buffer: &Self::Input) -> Result<Self::Response, CorruptedResponse> {
        T::parse_response(buffer)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GetValues;

impl Payload for GetValues {
    const LEN: usize = 1;

    fn append_to(&self, buffer: &mut impl Extend<u8>) {
        buffer.extend(once(4u8.to_be()));
    }
}

/// The response size is 62 bytes + 2 bytes for packet type and len + 1 byte for packet type + 3 bytes for crc and end byte.
impl Getter for GetValues {
    type Input = [u8; 68];
    type Response = GetValuesResponse;

    fn parse_response(buffer: &Self::Input) -> Result<Self::Response, CorruptedResponse> {
        let payload = extract_payload::<62>(buffer)?;
        
        Ok(GetValuesResponse {
            temp_fet: half_unscale_and_unpack([payload[0], payload[1]], 10.0),
            temp_motor: half_unscale_and_unpack([payload[2], payload[3]], 10.0),
            avg_motor_current: unscale_and_unpack([payload[4], payload[5], payload[6], payload[7]], 100.0),
            avg_input_current: unscale_and_unpack([payload[8], payload[9], payload[10], payload[11]], 100.0),
            avg_id: unscale_and_unpack([payload[12], payload[13], payload[14], payload[15]], 100.0),
            avg_iq: unscale_and_unpack([payload[16], payload[17], payload[18], payload[19]], 100.0),
            duty_cycle_now: half_unscale_and_unpack([payload[20], payload[21]], 1000.0),
            rpm: i32::from_be_bytes([payload[22], payload[23], payload[24], payload[25]]),
            v_in: half_unscale_and_unpack([payload[26], payload[27]], 10.0),
            amp_hours: unscale_and_unpack([payload[28], payload[29], payload[30], payload[31]], 10000.0),
            amp_hours_charged: unscale_and_unpack([payload[32], payload[33], payload[34], payload[35]], 10000.0),
            watt_hours: unscale_and_unpack([payload[36], payload[37], payload[38], payload[39]], 10000.0),
            watt_hours_charged: unscale_and_unpack([payload[40], payload[41], payload[42], payload[43]], 10000.0),
            tachometer: i32::from_be_bytes([payload[44], payload[45], payload[46], payload[47]]),
            tachometer_abs: i32::from_be_bytes([payload[48], payload[49], payload[50], payload[51]]),
            mc_fault_code: u8::from_be(payload[52]),
            pid_pos_now: unscale_and_unpack([payload[53], payload[54], payload[55], payload[56]], 1000000.0),
            app_controller_id: u8::from_be(payload[57]),
            time_ms: u32::from_be_bytes([payload[58], payload[59], payload[60], payload[61]]),
        })
    }
}

fn extract_payload<const N: usize>(buffer: &[u8]) -> Result<[u8; N], CorruptedResponse> {
    let header_size;
    let length;
    if N > 255 {
        if buffer[0] != 3 || buffer.len() < 3 {
            return Err(CorruptedResponse);
        }
        length = u16::from_be_bytes([buffer[1], buffer[2]]) as usize;
        header_size = 3;
    } else {
        if buffer[0] != 2 || buffer.len() < 2 {
            return Err(CorruptedResponse);
        }
        length = u8::from_be(buffer[1]) as usize;
        header_size = 2;
    }
    if length != N {
        return Err(CorruptedResponse);
    }
    if buffer.len() != N + header_size + 2 {
        return Err(CorruptedResponse);
    }
    if buffer.last() != Some(&3) {
        return Err(CorruptedResponse);
    }

    let payload_slice = &buffer[header_size..header_size + N];
    let received_checksum = u16::from_be_bytes([buffer[N + header_size], buffer[N + header_size + 1]]);
    let calculated_checksum = State::<XMODEM>::calculate(payload_slice);

    if received_checksum != calculated_checksum {
        return Err(CorruptedResponse);
    }

    let mut payload = [0u8; N];
    payload.copy_from_slice(payload_slice);
    Ok(payload)
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct GetValuesResponse {
    // ('temp_fet', 'h', 10),
    // ('temp_motor', 'h', 10),
    // ('avg_motor_current', 'i', 100),
    // ('avg_input_current', 'i', 100),
    // ('avg_id', 'i', 100),
    // ('avg_iq', 'i', 100),
    // ('duty_cycle_now', 'h', 1000),
    // ('rpm', 'i', 1),
    // ('v_in', 'h', 10),
    // ('amp_hours', 'i', 10000),
    // ('amp_hours_charged', 'i', 10000),
    // ('watt_hours', 'i', 10000),
    // ('watt_hours_charged', 'i', 10000),
    // ('tachometer', 'i', 1),
    // ('tachometer_abs', 'i', 1),
    // ('mc_fault_code', 'c', 0),
    // ('pid_pos_now', 'i', 1000000),
    // ('app_controller_id', 'c', 0),
    // ('time_ms', 'i', 1),

    pub temp_fet: f32,
    pub temp_motor: f32,
    pub avg_motor_current: f32,
    pub avg_input_current: f32,
    pub avg_id: f32,
    pub avg_iq: f32,
    pub duty_cycle_now: f32,
    pub rpm: i32,
    pub v_in: f32,
    pub amp_hours: f32,
    pub amp_hours_charged: f32,
    pub watt_hours: f32,
    pub watt_hours_charged: f32,
    pub tachometer: i32,
    pub tachometer_abs: i32,
    pub mc_fault_code: u8,
    pub pid_pos_now: f32,
    pub app_controller_id: u8,
    pub time_ms: u32,
}

pub trait Payload {
    const LEN: usize;

    fn append_to(&self, buffer: &mut impl Extend<u8>);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CorruptedResponse;

pub trait Getter {
    type Input;
    type Response;

    fn parse_response(buffer: &Self::Input) -> Result<Self::Response, CorruptedResponse>;
}

#[derive(Default)]
pub struct VescPacker {
    buffer: Vec<u8, 68>,
}

impl VescPacker {
    pub fn pack<'a, P: Payload>(&'a mut self, payload: &P) -> &'a [u8] {
        self.buffer.clear();
        if P::LEN > 255 {
            self.buffer.push(3u8.to_be()).unwrap();
            self.buffer.extend((P::LEN as u16).to_be_bytes());
        } else {
            self.buffer.push(2u8.to_be()).unwrap();
            self.buffer.push((P::LEN as u8).to_be()).unwrap();
        }
        let payload_start_index = self.buffer.len();
        payload.append_to(&mut self.buffer);
        self.buffer.extend(State::<XMODEM>::calculate(&self.buffer[payload_start_index..]).to_be_bytes());
        self.buffer.push(3u8.to_be()).unwrap();
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
    u32::from_ne_bytes(((data * scale) as i32).to_ne_bytes())
}

fn unscale_and_unpack(data: [u8; 4], scale: f32) -> f32 {
    i32::from_ne_bytes(data) as f32 / scale
}

fn half_unscale_and_unpack(data: [u8; 2], scale: f32) -> f32 {
    i16::from_ne_bytes(data) as f32 / scale
}