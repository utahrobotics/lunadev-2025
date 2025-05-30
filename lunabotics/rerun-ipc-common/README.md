# rerun-ipc-common

A lightweight wrapper for rerun data types designed for zero-copy inter-process communication via iceoryx2.

## Purpose

This crate provides serializable versions of common rerun archetypes that can be sent over iceoryx2 IPC without requiring the full rerun dependency in publisher applications.

## Supported Types

### Archetypes
- `Points3D<N>` - 3D point clouds with colors and radii
- `Boxes3D<N>` - 3D bounding boxes with quaternion rotations
- `TextLog<N>` - Text logging with severity levels
- `Transform3D` - 3D transformations (position, rotation, scale)
- `Pinhole` - Camera parameters
- `DepthImage<N>` - Depth images with meter and depth range support

### Base Types
- `Position3D` - 3D coordinates
- `Quaternion` - Quaternion rotation
- `Color` - RGBA color
- `Level` - Log levels (Info, Warning, Error)
- `ImageFormat` - Image metadata
- `ChannelDatatype` - Data type specifications

### Message Envelope
- `RerunMessage<N>` - Unified message type containing log path and archetype data

## Usage

### Creating Messages

```rust
use rerun_types_wrapper::{RerunMessage, archetypes::*};
use iceoryx2_bb_container::vec::FixedSizeVec;

// Create a points message
let points = Points3D {
    positions: FixedSizeVec::new(),
    colors: FixedSizeVec::new(),
    radii: FixedSizeVec::new(),
};

// Create message with log path
let message = RerunMessage::points_3d("world/sensors/lidar", points)?;
```

### Builder Pattern Support

```rust
// Depth image with custom parameters
let depth_image = DepthImage::new(bytes, format)
    .with_meter(0.001)  // 1mm per unit
    .with_depth_range([0.0, 5.0]);  // 0-5 meter range

let message = RerunMessage::depth_image("cameras/front/depth", depth_image)?;
```

## Dependencies

- `iceoryx2` - Zero-copy IPC framework
- `iceoryx2-bb-container` - Fixed-size containers
- `serde` - Serialization framework

## Integration

This crate is designed to work with the `rerun-ipc` subscriber that handles the conversion to actual rerun logging calls. 
