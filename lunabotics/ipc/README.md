# IPC Library

A simple IPC (Inter-Process Communication) library built on top of iceoryx2 for efficient message passing between processes.

## Features

- Type-safe communication using Serde's serialization/deserialization
- Binary encoding using Bincode for efficiency
- Support for any serializable data type
- Simple channel-based API similar to std::sync::mpsc
- Dynamic allocation for varying message sizes

## Usage

### Basic Example

```rust
use ipc::{channel, IPCSender, IPCReceiver};
use serde::{Serialize, Deserialize};

// Define a serializable message type
#[derive(Serialize, Deserialize)]
struct MyMessage {
    id: u32,
    data: Vec<f32>,
}

// Create a channel
let (sender, receiver) = channel::<MyMessage>("my_service").unwrap();

// Send a message
let msg = MyMessage { id: 1, data: vec![1.0, 2.0, 3.0] };
sender.send(&msg).unwrap();

// Receive a message (blocking)
let received = receiver.recv().unwrap();

// Or try to receive without blocking
match receiver.try_recv().unwrap() {
    Some(msg) => println!("Received: {:?}", msg),
    None => println!("No message available"),
}
```

## Running the Examples

This library includes example sender and receiver programs that demonstrate IPC communication.

### Dynamic Allocation Test

The examples demonstrate the dynamic allocation capabilities by sending messages with increasingly larger payloads:

1. Start the receiver in one terminal:
   ```
   cargo run --example receiver
   ```

2. Start the sender in another terminal:
   ```
   cargo run --example sender
   ```

The sender will send 5 messages with exponentially increasing sizes:
- Message #0: 1KB payload
- Message #1: 10KB payload
- Message #2: 100KB payload
- Message #3: 1MB payload
- Message #4: 10MB payload

This tests the dynamic allocation capabilities of the underlying iceoryx2 infrastructure and our serialization layer.

## Implementation Details

- Uses iceoryx2's publish-subscribe mechanism for zero-copy communication
- Messages are serialized using bincode for efficient binary encoding
- Dynamic slices allow for variable-sized messages
- Automatic memory management with intelligent allocation strategies
- Both blocking and non-blocking receive operations are supported 