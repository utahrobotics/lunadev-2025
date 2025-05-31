use iceoryx2::prelude::*;
use iceoryx2::port::publisher::Publisher;
use iceoryx2::port::subscriber::Subscriber;
use iceoryx2::service::ipc;
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;

pub struct IPCSender<T: Serialize + for<'a> Deserialize<'a>> {
    publisher: Publisher<ipc::Service, [u8], ()>,
    _phantom: PhantomData<T>,
}

pub struct IPCReceiver<T: Serialize + for<'a> Deserialize<'a>> {
    subscriber: Subscriber<ipc::Service, [u8], ()>,
    _phantom: PhantomData<T>,
}

pub fn channel<T: Serialize + for<'a> Deserialize<'a>>(service_path: &'static str) 
    -> Result<(IPCSender<T>, IPCReceiver<T>), Box<dyn std::error::Error>> {
    
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
    
    let subscriber = service.subscriber_builder().create()?;
    
    Ok((
        IPCSender {
            publisher,
            _phantom: PhantomData,
        },
        IPCReceiver {
            subscriber,
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
        
        Ok(Self {
            publisher,
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
