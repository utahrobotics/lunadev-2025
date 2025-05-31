use ipc::IPCReceiver;
use serde::{Serialize, Deserialize};
use std::{thread, time::Duration};

#[derive(Debug, Serialize, Deserialize)]
struct DynamicMessage {
    id: u32,
    text: String,
    // Large vector with varying size to test dynamic allocation
    payload: Vec<u8>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a receiver
    let receiver = IPCReceiver::<DynamicMessage>::new("DynamicChannel")?;
    
    println!("Receiver initialized. Waiting for messages...");
    println!("Press Ctrl+C to exit.");
    
    // Loop to receive messages
    loop {
        // Try to receive a message (non-blocking)
        match receiver.try_recv()? {
            Some(message) => {
                println!("Received: Message #{} with {} bytes of payload", 
                    message.id, message.payload.len());
                
                // Verify first byte of payload matches the id
                if !message.payload.is_empty() && message.payload[0] == message.id as u8 {
                    println!("  Payload verification: OK");
                } else if message.payload.is_empty() {
                    println!("  Payload is empty");
                } else {
                    println!("  Payload verification: FAILED");
                }
            },
            None => {
                // No message received, wait a bit
                thread::sleep(Duration::from_millis(500));
            }
        }
    }
} 