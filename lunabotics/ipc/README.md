# IPC Library

A simple IPC (Inter-Process Communication) library built on top of iceoryx2 for efficient message passing between processes.

## Features

- Type-safe communication using Serde's serialization/deserialization
- Binary encoding using Bincode for efficiency
- Support for any serializable data type
- Simple channel-based API similar to std::sync::mpsc
- Dynamic allocation for varying message sizes
- **Event-based coordination with ready signals and heartbeats**

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

### Event-Based Coordination

The IPC library now supports event-based coordination to synchronize sender and receiver processes:

#### Receiver with Heartbeat

```rust
use ipc::IPCReceiver;
use std::time::Duration;

// Create a receiver
let receiver = IPCReceiver::<MyMessage>::new("my_service")?;

// Start a heartbeat that signals readiness every 500ms
let heartbeat_handle = receiver.start_heartbeat(Duration::from_millis(500))?;

// Your message processing loop here...
while running {
    if let Some(msg) = receiver.try_recv()? {
        println!("Received: {:?}", msg);
    }
    std::thread::sleep(Duration::from_millis(10));
}

// Stop the heartbeat when done
heartbeat_handle.stop()?;
```

#### Sender with Ready Wait

```rust
use ipc::IPCSender;

// Create a sender
let sender = IPCSender::<MyMessage>::new("my_service")?;

// Wait until the receiver signals it's ready
sender.wait_until_ready()?;

// Now send messages knowing the receiver is ready
for i in 0..10 {
    let msg = MyMessage { id: i, data: vec![1.0, 2.0, 3.0] };
    sender.send(&msg)?;
}
```

## Running the Examples

This library includes several example programs that demonstrate IPC communication.

### Basic Dynamic Allocation Test

The basic examples demonstrate the dynamic allocation capabilities by sending messages with increasingly larger payloads:

1. Start the receiver in one terminal:
   ```
   cargo run --example receiver
   ```

2. Start the sender in another terminal:
   ```
   cargo run --example sender
   ```

### Event-Based Coordination Examples

The event examples demonstrate the ready signal and heartbeat functionality with full sender-receiver coordination:

1. **Start the event receiver first** in one terminal:
   ```
   cargo run --example event_receiver
   ```
   
   The receiver will:
   - Initialize and start sending heartbeat signals every 500ms
   - Display when it's ready to receive messages
   - Wait indefinitely for incoming messages
   - Show each received message with a counter

2. **Start the event sender** in another terminal:
   ```
   cargo run --example event_sender
   ```
   
   The sender will:
   - Initialize and wait for the receiver's ready signal
   - Display a waiting message with instructions
   - **Automatically start sending once the receiver is detected**
   - Send 15 messages with timestamps
   - Show progress for each sent message
   - Exit after all messages are sent

**Expected Workflow:**
- Receiver starts and begins heartbeat
- Sender waits for ready signal
- Sender detects receiver and starts sending
- Receiver displays each incoming message
- Sender completes and exits
- Receiver continues running (Ctrl+C to stop)

This demonstrates perfect coordination where the sender will never start sending until it knows the receiver is ready to process messages.

## Implementation Details

- Uses iceoryx2's publish-subscribe mechanism for zero-copy communication
- Events are implemented using iceoryx2's event services for coordination
- Ready signals are sent via heartbeat on the "/ready" event channel
- **Event listeners are created once during initialization for efficient reuse**
- All operations are thread-safe and can be used in multi-threaded environments

## Event Functionality

### Methods

- **`IPCSender::wait_until_ready()`**: Blocks until a receiver signals readiness via heartbeat
- **`IPCReceiver::start_heartbeat(interval)`**: Starts a background thread that sends ready signals at the specified interval
- **`HeartbeatHandle::stop()`**: Stops the heartbeat thread gracefully

### Use Cases

- **Process Startup Coordination**: Ensure receivers are ready before senders start
- **Service Discovery**: Detect when services become available
- **Graceful Shutdown**: Coordinate process termination
- **Load Balancing**: Wait for services to be ready before sending requests 