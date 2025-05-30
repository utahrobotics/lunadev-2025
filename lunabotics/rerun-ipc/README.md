# rerun-ipc

An iceoryx2 subscriber that receives serialized rerun messages and logs them to a rerun viewer.

## Purpose

This application acts as a bridge between lightweight publisher applications and rerun visualization. Publishers only need to import the `rerun-ipc-common` crate and send messages via iceoryx2, while this subscriber handles all rerun integration.

## Architecture

```
Publisher Apps          rerun-ipc               Rerun Viewer
     |                      |                        |
     |---> iceoryx2 ------> |                        |
     |    (zero-copy)       |---> rerun logging ---> |
     |                      |                        |
```

## Features

- Zero-copy message reception via iceoryx2
- Automatic conversion from wrapper types to rerun types
- User-specified log paths for organized data visualization
- Support for all major rerun archetypes
- Enhanced depth image support with meter and depth range

## Running

Start the subscriber:

```bash
cargo run
```

The subscriber will:
1. Initialize a rerun recording stream
2. Open an iceoryx2 service on `"rerun/messages"`
3. Wait for incoming messages
4. Convert and log received data to rerun

## Publisher Integration

Publishers should use the `rerun-ipc-common` crate:

```rust
use iceoryx2::prelude::*;
use rerun_types_wrapper::RerunMessage;

// Create iceoryx2 publisher
let node = NodeBuilder::new().create::<ipc::Service>()?;
let service = node.service_builder(&"rerun/messages".try_into()?)
    .publish_subscribe::<RerunMessage<1024>>()
    .open_or_create()?;
let publisher = service.publisher_builder().create()?;

// Send messages with custom log paths
let message = RerunMessage::points_3d("sensors/lidar", points_data)?;
let sample = publisher.loan_uninit()?;
let sample = sample.write_payload(message);
sample.send()?;
```

## Supported Message Types

- **Points3D** - 3D point clouds with colors and radii
- **Boxes3D** - 3D bounding boxes with quaternion rotations
- **TextLog** - Text logging with severity levels (Info, Warning, Error)
- **Transform3D** - 3D transformations with position, rotation, and scale
- **Pinhole** - Camera parameters with focal length and resolution
- **DepthImage** - Depth images with meter conversion and depth range

## Dependencies

- `iceoryx2` - Zero-copy IPC framework
- `rerun` - Visualization and logging framework
- `rerun-ipc-common` - Lightweight type definitions

## Service Configuration

- **Service Name**: `"rerun/messages"`
- **Message Type**: `RerunMessage<1024>`
- **Pattern**: Publish-Subscribe
- **Cycle Time**: 100ms polling interval 
