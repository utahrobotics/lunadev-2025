use iceoryx2::prelude::*;
use iceoryx2::port::publisher::Publisher;
use iceoryx2::port::subscriber::Subscriber;
use iceoryx2::service::ipc;
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;
use std::time::Duration;
use std::thread;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

pub struct IPCSender<T: Serialize + for<'a> Deserialize<'a>> {
    publisher: Publisher<ipc::Service, [u8], ()>,
    node: Node<ipc::Service>,
    ready_listener: iceoryx2::port::listener::Listener<ipc::Service>,
    _phantom: PhantomData<T>,
}

pub struct IPCReceiver<T: Serialize + for<'a> Deserialize<'a>> {
    subscriber: Subscriber<ipc::Service, [u8], ()>,
    node: Node<ipc::Service>,
    _phantom: PhantomData<T>,
}

pub fn channel<T: Serialize + for<'a> Deserialize<'a>>(service_path: &'static str) 
    -> Result<(IPCSender<T>, IPCReceiver<T>), Box<dyn std::error::Error>> {
    
    let sender_node = NodeBuilder::new().create::<ipc::Service>()?;
    let receiver_node = NodeBuilder::new().create::<ipc::Service>()?;
    
    let sender_service = sender_node
        .service_builder(&service_path.try_into()?)
        .publish_subscribe::<[u8]>()
        .open_or_create()?;
    
    let receiver_service = receiver_node
        .service_builder(&service_path.try_into()?)
        .publish_subscribe::<[u8]>()
        .open_or_create()?;
    
    let publisher = sender_service
        .publisher_builder()
        .initial_max_slice_len(1024)
        .allocation_strategy(AllocationStrategy::PowerOfTwo)
        .create()?;
    
    let subscriber = receiver_service.subscriber_builder().create()?;
    
    // Create the ready event listener for the sender
    let ready_event = sender_node.service_builder(&"ready".try_into()?)
        .event()
        .open_or_create()?;
    let ready_listener = ready_event.listener_builder().create()?;
    
    Ok((
        IPCSender {
            publisher,
            node: sender_node,
            ready_listener,
            _phantom: PhantomData,
        },
        IPCReceiver {
            subscriber,
            node: receiver_node,
            _phantom: PhantomData,
        },
    ))
}

impl<T: Serialize + for<'a> Deserialize<'a>> IPCSender<T> {
    pub fn new(service_path: &'static str) -> Result<Self, Box<dyn std::error::Error>> {
        let node = NodeBuilder::new().create::<ipc::Service>()?;
        
        let service = node
            .service_builder(&service_path.try_into()?)
            .publish_subscribe::<[u8]>()
            .open_or_create()?;
        
        let publisher = service
            .publisher_builder()
            .initial_max_slice_len(1024)
            .allocation_strategy(AllocationStrategy::PowerOfTwo)
            .create()?;
        
        // Create the ready event listener once during initialization
        let ready_event = node.service_builder(&"ready".try_into()?)
            .event()
            .open_or_create()?;
        let ready_listener = ready_event.listener_builder().create()?;
        
        Ok(Self {
            publisher,
            node,
            ready_listener,
            _phantom: PhantomData,
        })
    }
    
    pub fn send(&self, data: &T) -> Result<(), Box<dyn std::error::Error>> {
        let serialized = bincode::serialize(data)?;
        let mut sample = self.publisher.loan_slice(serialized.len())?;
        sample.clone_from_slice(&serialized);
        sample.send()?;
        Ok(())
    }

    /// Wait until a heartbeat is received from the receiver
    pub fn wait_until_ready(&self) -> Result<(), Box<dyn std::error::Error>> {
        println!("Waiting for receiver to be ready...");
        
        // Use the pre-created listener instead of creating a new one
        loop {
            match self.ready_listener.try_wait_one() {
                Ok(Some(_event_id)) => {
                    println!("Receiver is ready!");
                    return Ok(());
                }
                Ok(None) => {
                    // No event yet, sleep briefly and try again
                    thread::sleep(Duration::from_millis(10));
                }
                Err(e) => {
                    return Err(format!("Error waiting for ready event: {}", e).into());
                }
            }
        }
    }
}

impl<T: Serialize + for<'a> Deserialize<'a>> IPCReceiver<T> {
    pub fn new(service_path: &'static str) -> Result<Self, Box<dyn std::error::Error>> {
        let node = NodeBuilder::new().create::<ipc::Service>()?;
        
        let service = node
            .service_builder(&service_path.try_into()?)
            .publish_subscribe::<[u8]>()
            .open_or_create()?;
        
        let subscriber = service.subscriber_builder().create()?;
        
        Ok(Self {
            subscriber,
            node,
            _phantom: PhantomData,
        })
    }
    
    pub fn recv(&self) -> Result<T, Box<dyn std::error::Error>> {
        loop {
            if let Some(sample) = self.subscriber.receive()? {
                let bytes = sample.payload();
                let deserialized: T = bincode::deserialize(bytes)?;
                return Ok(deserialized);
            }
        }
    }
    
    pub fn try_recv(&self) -> Result<Option<T>, Box<dyn std::error::Error>> {
        if let Some(sample) = self.subscriber.receive()? {
            let bytes = sample.payload();
            let deserialized: T = bincode::deserialize(bytes)?;
            return Ok(Some(deserialized));
        }
        Ok(None)
    }

    /// Start a heartbeat that sends ready signals at regular intervals
    pub fn start_heartbeat(&self, interval: Duration) -> Result<HeartbeatHandle, Box<dyn std::error::Error>> {
        let event = self.node.service_builder(&"ready".try_into()?)
            .event()
            .open_or_create()?;
        
        let notifier = event.notifier_builder().create()?;
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_flag_clone = stop_flag.clone();
        
        let handle = thread::spawn(move || {
            let event_id = EventId::new(1);
            
            while !stop_flag_clone.load(Ordering::Relaxed) {
                if let Err(e) = notifier.notify_with_custom_event_id(event_id) {
                    eprintln!("Failed to send ready signal: {}", e);
                } else {
                    println!("Sent ready signal");
                }
                
                thread::sleep(interval);
            }
            
            println!("Heartbeat stopped");
        });
        
        Ok(HeartbeatHandle {
            stop_flag,
            handle: Some(handle),
        })
    }
}

/// Handle for controlling the heartbeat thread
pub struct HeartbeatHandle {
    stop_flag: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl HeartbeatHandle {
    /// Stop the heartbeat and wait for the thread to finish
    pub fn stop(mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.stop_flag.store(true, Ordering::Relaxed);
        
        if let Some(handle) = self.handle.take() {
            handle.join().map_err(|_| "Failed to join heartbeat thread")?;
        }
        
        Ok(())
    }
}

impl Drop for HeartbeatHandle {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use std::thread;

    #[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
    struct TestMessage {
        id: u32,
        name: String,
        data: Vec<f32>,
    }

    #[test]
    fn test_send_receive() -> Result<(), Box<dyn std::error::Error>> {
        // Create a channel
        let (sender, receiver) = channel::<TestMessage>("TestChannel")?;
        
        // Create test message
        let test_msg = TestMessage {
            id: 42,
            name: "Test Message".to_string(),
            data: vec![1.0, 2.5, 3.14, 4.2],
        };
        
        // Send the message
        sender.send(&test_msg)?;
        
        // Give some time for message to propagate
        thread::sleep(Duration::from_millis(100));
        
        // Try to receive the message
        let received = receiver.try_recv()?;
        
        // Verify the message was received correctly
        assert!(received.is_some(), "Expected to receive a message");
        assert_eq!(received.unwrap(), test_msg);
        
        Ok(())
    }
    
    #[test]
    fn test_multiple_messages() -> Result<(), Box<dyn std::error::Error>> {
        // Create a channel
        let (sender, receiver) = channel::<i32>("TestChannelInt")?;
        
        // First try_recv should return None as no message has been sent
        let result = receiver.try_recv()?;
        assert!(result.is_none(), "Expected no messages initially");
        
        // Send multiple messages
        for i in 0..5 {
            sender.send(&i)?;
        }
        
        // Allow some time for messages to be processed
        thread::sleep(Duration::from_millis(100));
        
        // Receive all messages
        let mut received = Vec::new();
        while let Some(value) = receiver.try_recv()? {
            received.push(value);
        }
        
        // Verify messages were received
        assert!(!received.is_empty(), "Expected to receive at least one message");
        assert!(received.len() <= 5, "Should not receive more than 5 messages");
        
        // Verify all received values are in the range we sent
        for val in received {
            assert!(val >= 0 && val < 5, "Received unexpected value: {}", val);
        }
        
        Ok(())
    }
}
