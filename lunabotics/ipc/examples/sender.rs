use ipc::IPCSender;
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
    // Create a sender
    let sender = IPCSender::<DynamicMessage>::new("DynamicChannel")?;
    
    println!("Sender initialized. Sending messages with increasing sizes...");
    
    // Send messages with exponentially increasing payload sizes
    for i in 0..5 {
        // Create a payload that grows exponentially (1KB, 10KB, 100KB, 1MB, 10MB)
        let size = 1024 * 10_u32.pow(i);
        let payload = vec![i as u8; size as usize];
        
        let message = DynamicMessage {
            id: i,
            text: format!("Message #{} with {} bytes", i, size),
            payload,
        };
        
        println!("Sending: Message #{} with {} bytes", i, size);
        sender.send(&message)?;
        
        // Wait a bit between messages
        thread::sleep(Duration::from_secs(1));
    }
    
    println!("All messages sent. Keep this process running to maintain the service.");
    println!("Press Ctrl+C to exit.");
    
    // Keep the program running
    loop {
        thread::sleep(Duration::from_secs(1));
    }
} 