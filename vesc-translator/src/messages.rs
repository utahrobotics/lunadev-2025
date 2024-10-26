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

/// Represents something that can be sent by CAN or UART to VESC motor controllers.
pub trait VescSendable {
    /// Add the bytes representing a VESC packet header (typically in CAN control) to a given Vec
    fn extend_header_binary(&self, out: &mut Vec<u8>);
    /// Add the bytes representing a VESC packet body to a given Vec
    fn extend_body_binary(&self, out: &mut Vec<u8>);
    /// Add the bytes representing a full UART command to a given Vec
    fn extend_uart_binary(&self, out: &mut Vec<u8>);

    /// Create a new vec with bytes representing a VESC packet header (typically in CAN control).
    fn to_header_binary(&self) -> Vec<u8> {
        let mut out = vec![];
        self.extend_header_binary(&mut out);
        out
    }

    /// Create a new vec with bytes representing a VESC packet body.
    fn to_body_binary(&self) -> Vec<u8> {
        let mut out = vec![];
        self.extend_body_binary(&mut out);
        out
    }

    /// Create a new vec with bytes representing a full UART command.
    fn to_uart_binary(&self) -> Vec<u8> {
        let mut out = vec![];
        self.extend_uart_binary(&mut out);
        out
    }
}

/// The main implementation of VescSendable, represents a message
/// in VESC, with a command ID, payload data, and an optional target ID
/// for control over CAN.
#[derive(Clone, Copy)]
pub struct Message {
    command: CommandType,
    target: Option<u8>,
    payload: f32,
}
impl Message {
    /// Creates a new Message. If sent, it will command the motor controller
    /// with id "target" to execute the command "command", according to whatever
    /// payload data value is in "payload".
    pub fn new(command: CommandType, target: u8, payload: f32) -> Self {
        return Self {
            command,
            target: Option::Some(target),
            payload,
        };
    }

    /// Creates a new Message. If sent, it will command whatever motor controller
    /// it is sent to to execute the command "command", according to whatever
    /// payload data value is in "payload". This kind of message cannot be used over CAN.
    pub fn new_no_target(command: CommandType, payload: f32) -> Self {
        return Self {
            command,
            target: Option::None,
            payload,
        };
    }
}
impl VescSendable for Message {
    fn extend_header_binary(&self, out: &mut Vec<u8>) {
        // If no target id was specified, no header can be made, and this is invalid, so panic
        let target_val = self.target.clone().expect("A message without a target id cannot generate a header.");
        // target is stored in the lower byte, the rest of the space is used for the command
        out.extend(((target_val as u32) | ((self.command as u32) << 8)).to_be_bytes());
    }

    // It's unclear whether this should give a 32 bit or 64 bit output.
    // Note that the payload is stored in the exact format it was specified,
    // despite being used in a converted form everywhere. This could be changed,
    // but I haven't done so so that future methods can access the exact form.
    fn extend_body_binary(&self, out: &mut Vec<u8>) {
        out.extend(self.command.pack_payload_data(self.payload).to_be_bytes());
    }

    fn extend_uart_binary(&self, out: &mut Vec<u8>) {
        out.extend(&(self.command as u32 + 0x020505_u32).to_be_bytes()[1..]);
        self.extend_body_binary(out);
        out.extend(CRC_GENERATOR.checksum(out).to_be_bytes());
        out.push(3_u8);
    }
}

/// This represents one of the VESC commands that can be sent.
/// Currently only a few are implemented. To add more, add an item
/// to the enum, and a case in the match statement in pack_payload_data.
/// The numeric value of CommandTypes is the command id in VESC.
#[derive(Clone, Copy)]
#[repr(u8)]
pub enum CommandType {
    SetDutyCycle = 0x0,
    SetRpm = 0x3,
}
impl CommandType {
    /// Converts data to be transmitted into the form expected by VESC, applying a scaling.
    fn pack_payload_data(self, payload: f32) -> u32 {
        match self {
            CommandType::SetDutyCycle => scale_and_pack(payload, 100_000_f32),
            CommandType::SetRpm => scale_and_pack(payload, 1_f32),
        }
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