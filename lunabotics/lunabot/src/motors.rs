use urobotics::{serial, tokio::io::{AsyncWriteExt, WriteHalf}};
use vesc_translator::{CommandType, Message, VescSendable};
use tokio_serial::SerialStream;

//use socketcan::{socket::*, CanFrame, EmbeddedFrame, ExtendedId}; used for CAN comms, not supported yet

/// The Motor trait is the basis for interactions with vesc-translator that use comms.
/// A common, provided implementer for this trait is VescCanMotor.
/// Motors represent a motor. This means that the user can send commands to the motor
/// struct, which will then hopefully control the motor.
pub trait Motor {
    /// Sends an arbitrary command to the motor of type "command",
    /// with the payload data "payload".
    fn send_message(&mut self, command: CommandType, payload: f32);

    // All motor commands should just use send_message with different parameters
    /// Sets the RPM of the attached motor. rpm - [-2^31, 2^31 - 1]
    fn set_rpm(&mut self, rpm: f32) {
        self.send_message(CommandType::SetRpm, rpm);
    }
    /// Sets the duty cycle, or "on level" of the motor. duty_cycle - [-1, 1]
    fn set_duty_cycle(&mut self, duty_cycle: f32) {
        self.send_message(CommandType::SetDutyCycle, duty_cycle);
    }

    // TODO add more commands and requests
}

// Helpers
/// Turns the string of up to four bytes into a unsigned 32 bit integer, assuming big endian order.
fn merge_bytes_small(bytes: Vec<u8>) -> u32 {
    if bytes.len() > 4 {
        // This should really be an error value instead of a panic, but I'm rushing things.
        panic!("merge_bytes_small can only be called on series of bytes smaller than 4.");
    }

    let mut shift_val = 8 * bytes.len();
    let mut out = 0_u32;

    for byte in bytes {
        shift_val -= 8; // Make all future loops closer to the one's place
        out |= (byte as u32) << shift_val; // Insert the next byte
    }

    out
}

pub struct VescUartSerialMotor {
	connection: WriteHalf<SerialStream>
}

impl VescUartSerialMotor {
	pub fn new_from_path(path: &'static str) -> Self {
		let connection = serial::SerialConnection::new(path);
		return VescUartSerialMotor {
			connection: connection.spawn().expect("Serial connection failed")
		}
	}
}

// TODO: Seamlessly reconnect if possible

impl Motor for VescUartSerialMotor {
	fn send_message(&mut self, command: CommandType, payload: f32) {
		let message = Message::new_no_target(command, payload);
		let a =self.connection.write_all(message.to_uart_binary().as_slice());
	}
}

/* Implements CAN motors, not supported yet
/// A common implementation of the Motor trait, which uses the 
/// VESC message generation of the messages package, and sends
/// its messages over serial port CAN bus using socketcan.
pub struct VescCanMotor {
    id: u8,
    soc: CanSocket,
}
impl VescCanMotor {
    /// Creates a new motor using VESC and CAN over serial port.
    /// 
    /// id - The motor's assigned ID.
    fn new(id: u8) -> Self {
        Self {
            id,
            soc: CanSocket::open_addr(&CanAddr::new(0)).expect("CAN socket opening failed."),
        }
    }

    /// Creates a new motor using VESC and CAN over serial port.
    /// 
    /// id - The motor's assigned ID.
    /// interface - The interface to communicate with the motor over.
    pub fn new_with_interface(id: u8, interface: &CanAddr) -> Self {
        Self {
            id,
            soc: CanSocket::open_addr(interface).expect("CAN socket opening failed."),
        }
    }
}
impl Motor for VescCanMotor {
    

    fn send_message(&self, command: CommandType, payload: f32) {
        // Create the message object
        let msg = Message::new(command, self.id, payload);

        let id = ExtendedId::new(merge_bytes_small(msg.to_header_binary())).unwrap();
        // Turn it into socketcan's message object
        let frame: CanFrame = CanFrame::new(id, msg.to_body_binary().as_slice()).unwrap();
        // Send it
        _ = self.soc.write_frame_insist(&frame);
    }
}*/
