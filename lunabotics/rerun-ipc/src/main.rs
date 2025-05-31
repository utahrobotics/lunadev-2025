use std::time::Duration;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use ipc::IPCReceiver;
use rerun_ipc_common::{Level, RerunMessage};

const CYCLE_TIME: Duration = Duration::from_millis(100);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting Rerun IPC subscriber server");

    let rec = rerun::RecordingStreamBuilder::new("rerun_ipc_subscriber")
        .spawn()?;

    let receiver = IPCReceiver::<(RerunMessage, bool)>::new("rerun/messages")?;
    let heartbeat = receiver.start_heartbeat(Duration::from_millis(100)).expect("Failed to start heartbeat");
    println!("Subscriber ready - waiting for rerun messages...");
    println!("Press Ctrl+C to stop gracefully...");

    // Set up signal handling for graceful shutdown
    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();
    
    ctrlc::set_handler(move || {
        println!("\nReceived Ctrl+C, shutting down rerun-ipc gracefully...");
        running_clone.store(false, Ordering::Relaxed);
    })?;

    let mut total_messages_received = 0;
    
    // Main loop to continuously receive and process messages until interrupted
    while running.load(Ordering::Relaxed) {
        match receiver.try_recv()? {
            Some((rerun_msg, is_static)) => {
                total_messages_received += 1;
                
                let msg_type = match &rerun_msg {
                    RerunMessage::Points3D(path, _) => format!("Points3D to {}", path),
                    RerunMessage::Boxes3D(path, _) => format!("Boxes3D to {}", path),
                    RerunMessage::TextLog(path, _) => format!("TextLog to {}", path),
                    RerunMessage::Transform3D(path, _) => format!("Transform3D to {}", path),
                    RerunMessage::Pinhole(path, _) => format!("Pinhole to {}", path),
                    RerunMessage::DepthImage(path, _) => format!("DepthImage to {}", path),
                    RerunMessage::Arrows3D(path, _) => format!("Arrows3D to {}", path),
                    RerunMessage::ViewCoordinates(path, _) => format!("ViewCoordinates to {}", path),
                    RerunMessage::Asset3D(path, _) => format!("Asset3D to {}", path),
                };
                if !msg_type.contains("Transform3D") { 
                    println!("🔧 DEBUG: Received message #{}: {} (static: {})", 
                    total_messages_received, msg_type, is_static);
                }

                
                match rerun_msg {
                    RerunMessage::Points3D(log_path, points) => {
                        println!("Received Points3D with {} positions for path: {}", points.positions.len(), log_path);
                        
                        // Convert to rerun format
                        let positions: Vec<[f32; 3]> = points.positions.iter()
                            .map(|p| [p.x, p.y, p.z])
                            .collect();
                        
                        let colors: Vec<[u8; 4]> = points.colors.iter()
                            .map(|c| [c.r, c.g, c.b, c.a])
                            .collect();
                        
                        let radii: Vec<f32> = points.radii.iter().cloned().collect();
                        
                        if is_static {
                            rec.log_static(
                                log_path.as_str(),
                                &rerun::Points3D::new(positions)
                                    .with_colors(colors)
                                    .with_radii(radii),
                            )?;
                        } else {
                            rec.log(
                                log_path.as_str(),
                                &rerun::Points3D::new(positions)
                                    .with_colors(colors)
                                    .with_radii(radii),
                            )?;
                        }
                    }
                    
                    RerunMessage::Boxes3D(log_path, boxes) => {
                        println!("Received Boxes3D with {} boxes for path: {}", boxes.centers.len(), log_path);
                        
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
                        if is_static {
                            rec.log_static(
                                log_path.as_str(),
                                &rerun::Boxes3D::from_centers_and_half_sizes(centers, half_sizes)
                                    .with_quaternions(rotations)
                                    .with_colors(colors),
                            )?;
                        } else {
                            rec.log(
                                log_path.as_str(),
                                &rerun::Boxes3D::from_centers_and_half_sizes(centers, half_sizes)
                                    .with_quaternions(rotations)
                                    .with_colors(colors),
                            )?;
                        }
                    }
                    
                    RerunMessage::TextLog(log_path, text_log) => {
                        println!("Received TextLog for path: {}", log_path);
                        let level_str = match text_log.level {
                            Level::Info => "INFO",
                            Level::Warning => "WARN", 
                            Level::Error => "ERROR",
                        };
                        
                        println!("Received TextLog [{}]: {} for path: {}", level_str, text_log.text, log_path);
                        
                        let rerun_level = match text_log.level {
                            Level::Info => rerun::TextLogLevel::INFO,
                            Level::Warning => rerun::TextLogLevel::WARN,
                            Level::Error => rerun::TextLogLevel::ERROR,
                        };
                        if is_static {
                            rec.log_static(
                                log_path.as_str(),
                                &rerun::TextLog::new(text_log.text).with_level(rerun_level),
                            )?;
                        } else {
                            rec.log(
                                log_path.as_str(),
                                &rerun::TextLog::new(text_log.text).with_level(rerun_level),
                            )?;
                        }

                    }
                    
                    RerunMessage::Transform3D(log_path, transform) => {
                        println!("Received Transform3D for path: {}", log_path);
                        
                        let translation = [transform.translation.x, transform.translation.y, transform.translation.z];
                        let rotation = rerun::Quaternion::from_xyzw([transform.rotation.x, transform.rotation.y, transform.rotation.z, transform.rotation.w]);
                        let scale = transform.scale;
                        
                        if is_static {
                            rec.log(
                                log_path.as_str(),
                                &rerun::Transform3D::from_translation_rotation_scale(
                                    translation,
                                    rotation,
                                    scale,
                                ),
                            )?;
                        } else {

                            rec.log(
                                log_path.as_str(),
                                &rerun::Transform3D::from_translation_rotation_scale(
                                    translation,
                                    rotation,
                                    scale,
                                ),
                            )?;
                        }
                    }
                    
                    RerunMessage::Pinhole(log_path, pinhole) => {
                        println!("Received Pinhole camera for path: {}", log_path);
                        
                        if is_static {
                            rec.log_static(
                                log_path.as_str(),
                                &rerun::Pinhole::from_focal_length_and_resolution(
                                    pinhole.focal_length,
                                    pinhole.resolution,
                                ),
                            )?;
                        } else{

                            rec.log(
                                log_path.as_str(),
                                &rerun::Pinhole::from_focal_length_and_resolution(
                                    pinhole.focal_length,
                                    pinhole.resolution,
                                ),
                            )?;
                        }
                    }
                    
                    RerunMessage::DepthImage(log_path, depth_image) => {
                        println!("Received DepthImage with {} pixels for path: {}", depth_image.bytes.len(), log_path);
                        
                        let image_data: Vec<u8> = depth_image.bytes.iter().cloned().collect();
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
                        if is_static {
                            rec.log_static(
                                log_path.as_str(),
                                &rerun::DepthImage::new(
                                    byte_data,
                                    rerun::ImageFormat::depth([width, height], rerun_datatype),
                                )
                                .with_meter(depth_image.meter)
                                .with_depth_range(depth_image.depth_range),
                            )?;
                        } else {
                            rec.log(
                                log_path.as_str(),
                                &rerun::DepthImage::new(
                                    byte_data,
                                    rerun::ImageFormat::depth([width, height], rerun_datatype),
                                )
                                .with_meter(depth_image.meter)
                                .with_depth_range(depth_image.depth_range),
                            )?;
                        }
                    }

                    RerunMessage::Arrows3D(log_path, arrows) => {
                        println!("Received Arrows3D with {} vectors for path: {}", arrows.vectors.len(), log_path);
                        
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
                        if is_static {
                            rec.log_static(log_path.as_str(), &arrows_3d)?;
                        } else{
                            rec.log(log_path.as_str(), &arrows_3d)?;
                        }
                    }

                    RerunMessage::ViewCoordinates(log_path, _view_coords) => {
                        println!("Received ViewCoordinates for path: {}", log_path);
                        println!("[INFO] Using right hand Y up.");
                        if is_static {
                            rec.log_static(log_path, &rerun::ViewCoordinates::RIGHT_HAND_Y_UP())?;
                        } else {
                            rec.log(log_path, &rerun::ViewCoordinates::RIGHT_HAND_Y_UP())?;
                        }
                    }

                    RerunMessage::Asset3D(log_path, asset) => {
                        println!("Received Asset3D ({}) for path: {}", asset.media_type, log_path);
                        if is_static {
                            rec.log_static(
                                log_path.as_str(),
                                &rerun::Asset3D::new(asset.data),
                            )?;
                        } else {
                            rec.log(
                                log_path.as_str(),
                                &rerun::Asset3D::new(asset.data),
                            )?;
                        }
                    }
                }
            }
            None => {
                // No messages available, sleep briefly
                std::thread::sleep(CYCLE_TIME);
            }
        }
    }
    
    println!("Stopping heartbeat...");
    heartbeat.stop().expect("Failed to stop heartbeat");
    println!("Rerun IPC subscriber stopped cleanly.");
    
    Ok(())
}
