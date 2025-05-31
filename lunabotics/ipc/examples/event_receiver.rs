use ipc::IPCReceiver;
use serde::{Serialize, Deserialize};
use std::{thread, time::Duration};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

#[derive(Debug, Serialize, Deserialize)]
struct ExampleMessage {
    id: u32,
    data: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Initializing receiver...");
    
    // Create a receiver
    let receiver = IPCReceiver::<ExampleMessage>::new("ExampleChannel")?;
    
    println!("Starting heartbeat to signal readiness...");
    
    // Start heartbeat to signal readiness every 500ms
    let heartbeat_handle = receiver.start_heartbeat(Duration::from_millis(500))?;
    
    println!("Receiver is ready! Heartbeat active. Waiting for messages...");
    println!("(The sender should now detect the ready signal and start sending)");
    println!("Press Ctrl+C to stop gracefully...");
    
    // Set up signal handling for graceful shutdown
    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();
    
    ctrlc::set_handler(move || {
        println!("\nReceived Ctrl+C, shutting down gracefully...");
        running_clone.store(false, Ordering::Relaxed);
    })?;
    
    // Keep track of received messages
    let mut message_count = 0;
    let mut last_message_time = std::time::Instant::now();
    
    // Receive messages until interrupted
    while running.load(Ordering::Relaxed) {
        match receiver.try_recv()? {
            Some(message) => {
                message_count += 1;
                last_message_time = std::time::Instant::now();
                println!("Received message #{}: {:?}", message_count, message);
            }
            None => {
                // No message available, sleep briefly
                thread::sleep(Duration::from_millis(100));
                
                // Show periodic status if no messages received recently
                if message_count > 0 && last_message_time.elapsed() > Duration::from_secs(3) {
                    println!("Waiting for more messages... (received {} so far)", message_count);
                    last_message_time = std::time::Instant::now(); // Reset to avoid spam
                }
            }
        }
    }
    
    println!("Stopping heartbeat...");
    heartbeat_handle.stop()?;
    println!("Receiver stopped cleanly.");
    
    Ok(())
} 