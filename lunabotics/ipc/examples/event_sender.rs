use ipc::IPCSender;
use serde::{Serialize, Deserialize};
use std::{thread, time::Duration};

#[derive(Debug, Serialize, Deserialize)]
struct ExampleMessage {
    id: u32,
    data: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Initializing sender...");
    
    // Create a sender
    let sender = IPCSender::<ExampleMessage>::new("ExampleChannel")?;
    
    println!("Waiting for receiver to be ready...");
    println!("(Start the event_receiver example in another terminal if you haven't already)");
    
    // Wait until receiver is ready - this will block until heartbeat is detected
    sender.wait_until_ready()?;
    
    println!("Receiver ready signal detected! Starting to send messages...");
    
    // Send example messages with a nice progression
    for i in 0..15 {
        let message = ExampleMessage {
            id: i,
            data: format!("Hello from sender! Message #{} at {}", i, 
                         chrono::Utc::now().format("%H:%M:%S")),
        };
        
        println!("Sending message #{}: {}", i, message.data);
        
        match sender.send(&message) {
            Ok(()) => {
                println!("  Message #{} sent successfully", i);
            }
            Err(e) => {
                println!("  Failed to send message #{}: {}", i, e);
            }
        }
        
        // Wait between messages - start fast, then slow down
        let delay = if i < 5 { 
            Duration::from_millis(500) 
        } else { 
            Duration::from_secs(1) 
        };
        thread::sleep(delay);
    }
    
    println!("All messages sent! Sender finished.");
    println!("(The receiver should continue running and show all received messages)");
    Ok(())
} 