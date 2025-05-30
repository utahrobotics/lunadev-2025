use std::time::Duration;
use iceoryx2::prelude::*;
use rerun_ipc_common::{Level, RerunMessage};

const CYCLE_TIME: Duration = Duration::from_millis(100);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting Rerun IPC subscriber server");

    let rec = rerun::RecordingStreamBuilder::new("rerun_ipc_subscriber")
        .spawn()?;

    let node = NodeBuilder::new().create::<ipc::Service>()?;


    let service = node.service_builder(&"rerun/messages".try_into()?)
        .publish_subscribe::<RerunMessage<1024>>()
        .open_or_create()?;

    let subscriber = service.subscriber_builder().create()?;

    println!("Subscriber ready - waiting for rerun messages...");

    // Main loop to continuously receive and process messages
    while node.wait(CYCLE_TIME).is_ok() {
        while let Some(sample) = subscriber.receive()? {
            let rerun_msg = &*sample;
            
            match rerun_msg {
                RerunMessage::Points3D(log_path, points) => {
                    let path_str = String::from_utf8_lossy(log_path.as_bytes());
                    println!("Received Points3D with {} positions for path: {}", points.positions.len(), path_str);
                    
                    // Convert to rerun format
                    let positions: Vec<[f32; 3]> = points.positions.iter()
                        .map(|p| [p.x, p.y, p.z])
                        .collect();
                    
                    let colors: Vec<[u8; 4]> = points.colors.iter()
                        .map(|c| [c.r, c.g, c.b, c.a])
                        .collect();
                    
                    let radii: Vec<f32> = points.radii.iter().cloned().collect();
                    
                    rec.log(
                        path_str.as_ref(),
                        &rerun::Points3D::new(positions)
                            .with_colors(colors)
                            .with_radii(radii),
                    )?;
                }
                
                RerunMessage::Boxes3D(log_path, boxes) => {
                    let path_str = String::from_utf8_lossy(log_path.as_bytes());
                    println!("Received Boxes3D with {} boxes for path: {}", boxes.centers.len(), path_str);
                    
                    let centers: Vec<[f32; 3]> = boxes.centers.iter()
                        .map(|p| [p.x, p.y, p.z])
                        .collect();
                    
                    let half_sizes: Vec<[f32; 3]> = boxes.half_sizes.iter()
                        .map(|p| [p.x, p.y, p.z])
                        .collect();
                    
                    let rotations: Vec<rerun::Quaternion> = boxes.quaternions.iter()
                        .map(|q| rerun::Quaternion::from_xyzw([q.x, q.y, q.z, q.w]))
                        .collect();
                    
                    let colors: Vec<[u8; 4]> = boxes.colors.iter()
                        .map(|c| [c.r, c.g, c.b, c.a])
                        .collect();
                    
                    rec.log(
                        path_str.as_ref(),
                        &rerun::Boxes3D::from_centers_and_half_sizes(centers, half_sizes)
                            .with_quaternions(rotations)
                            .with_colors(colors),
                    )?;
                }
                
                RerunMessage::TextLog(log_path, text_log) => {
                    let path_str = String::from_utf8_lossy(log_path.as_bytes());
                    let level_str = match text_log.level {
                        Level::Info => "INFO",
                        Level::Warning => "WARN", 
                        Level::Error => "ERROR",
                    };
                    
                    let text_content = String::from_utf8_lossy(text_log.text.as_bytes()).to_string();
                    println!("Received TextLog [{}]: {} for path: {}", level_str, text_content, path_str);
                    
                    let rerun_level = match text_log.level {
                        Level::Info => rerun::TextLogLevel::INFO,
                        Level::Warning => rerun::TextLogLevel::WARN,
                        Level::Error => rerun::TextLogLevel::ERROR,
                    };
                    
                    rec.log(
                        path_str.as_ref(),
                        &rerun::TextLog::new(text_content).with_level(rerun_level),
                    )?;
                }
                
                RerunMessage::Transform3D(log_path, transform) => {
                    let path_str = String::from_utf8_lossy(log_path.as_bytes());
                    println!("Received Transform3D for path: {}", path_str);
                    
                    let translation = [transform.position.x, transform.position.y, transform.position.z];
                    let rotation = rerun::Quaternion::from_xyzw([transform.rotation.x, transform.rotation.y, transform.rotation.z, transform.rotation.w]);
                    let scale = transform.scale;
                    
                    rec.log(
                        path_str.as_ref(),
                        &rerun::Transform3D::from_translation_rotation_scale(
                            translation,
                            rotation,
                            scale,
                        ),
                    )?;
                }
                
                RerunMessage::Pinhole(log_path, pinhole) => {
                    let path_str = String::from_utf8_lossy(log_path.as_bytes());
                    println!("Received Pinhole camera for path: {}", path_str);
                    
                    rec.log(
                        path_str.as_ref(),
                        &rerun::Pinhole::from_focal_length_and_resolution(
                            pinhole.focal_length,
                            pinhole.resolution,
                        ),
                    )?;
                }
                
                RerunMessage::DepthImage(log_path, depth_image) => {
                    let path_str = String::from_utf8_lossy(log_path.as_bytes());
                    println!("Received DepthImage with {} pixels for path: {}", depth_image.bytes.len(), path_str);
                    
                    let image_data: Vec<u16> = depth_image.bytes.iter().cloned().collect();
                    let [width, height] = depth_image.image_format.resolution;
                    
                    // Convert u16 data to bytes
                    let mut byte_data = Vec::with_capacity(image_data.len() * 2);
                    for pixel in image_data {
                        byte_data.extend_from_slice(&pixel.to_le_bytes());
                    }
                    
                    // Convert our ChannelDatatype to rerun's ChannelDatatype
                    let rerun_datatype = match depth_image.image_format.channel_datatype {
                        rerun_ipc_common::ChannelDatatype::U16 => rerun::ChannelDatatype::U16,
                        rerun_ipc_common::ChannelDatatype::U8 => rerun::ChannelDatatype::U8,
                        rerun_ipc_common::ChannelDatatype::F32 => rerun::ChannelDatatype::F32,
                        // Add other mappings as needed
                        _ => rerun::ChannelDatatype::U16, // default fallback
                    };
                    
                    rec.log(
                        path_str.as_ref(),
                        &rerun::DepthImage::new(
                            byte_data,
                            rerun::ImageFormat::depth([width, height], rerun_datatype),
                        )
                        .with_meter(depth_image.meter)
                        .with_depth_range(depth_image.depth_range),
                    )?;
                }

                RerunMessage::Arrows3D(log_path, arrows) => {
                    let path_str = String::from_utf8_lossy(log_path.as_bytes());
                    println!("Received Arrows3D with {} vectors for path: {}", arrows.vectors.len(), path_str);
                    
                    let vectors: Vec<[f32; 3]> = arrows.vectors.iter()
                        .map(|v| [v.x, v.y, v.z])
                        .collect();
                    
                    let origins: Vec<[f32; 3]> = arrows.origins.iter()
                        .map(|o| [o.x, o.y, o.z])
                        .collect();
                    
                    let colors: Vec<[u8; 4]> = arrows.colors.iter()
                        .map(|c| [c.r, c.g, c.b, c.a])
                        .collect();
                    
                    let mut arrows_3d = rerun::Arrows3D::from_vectors(vectors);
                    if !origins.is_empty() {
                        arrows_3d = arrows_3d.with_origins(origins);
                    }
                    if !colors.is_empty() {
                        arrows_3d = arrows_3d.with_colors(colors);
                    }
                    
                    rec.log(path_str.as_ref(), &arrows_3d)?;
                }

                RerunMessage::ViewCoordinates(log_path, view_coords) => {
                    let path_str = String::from_utf8_lossy(log_path.as_bytes());
                    println!("Received ViewCoordinates for path: {}", path_str);
                    
                    // Convert coordinates to rerun ViewCoordinates
                    // For now, assume right-hand Y-up coordinates
                    rec.log(
                        path_str.as_ref(),
                        &rerun::ViewCoordinates::RIGHT_HAND_Y_UP,
                    )?;
                }

                RerunMessage::Asset3D(log_path, asset) => {
                    let path_str = String::from_utf8_lossy(log_path.as_bytes());
                    let media_type = String::from_utf8_lossy(asset.media_type.as_bytes());
                    println!("Received Asset3D ({}) for path: {}", media_type, path_str);
                    
                    let asset_data = asset.data.as_bytes().to_vec();
                    
                    rec.log(
                        path_str.as_ref(),
                        &rerun::Asset3D::new(asset_data),
                    )?;
                }
            }
        }
    }

    println!("Subscriber shutting down");
    Ok(())
}
