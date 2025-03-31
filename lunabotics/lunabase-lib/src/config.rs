use std::path::Path;

use godot::global::{godot_error, godot_warn};
use serde::Deserialize;
use simple_motion::{ChainBuilder, Node, NodeData, NodeSerde};


#[derive(Deserialize)]
struct CameraInfo {
    link_name: String,
    stream_index: usize,
}

#[derive(Deserialize)]
struct Main {
    #[serde(default)]
    cameras: fxhash::FxHashMap<String, CameraInfo>,
    #[serde(default)]
    depth_cameras: fxhash::FxHashMap<String, CameraInfo>,
    robot_layout: String,
}

#[derive(Deserialize)]
struct TmpConfig {
    #[serde(rename = "Main")]
    main: Main,
}

pub struct AppConfig {
    pub camera_nodes: &'static [Option<Node<&'static [NodeData]>>],
    pub robot_chain: Node<&'static [NodeData]>,
}

pub fn load_config() -> Option<AppConfig> {
    if !Path::new("app-config.toml").exists() {
        godot_warn!("app-config.toml not found");
        return None;
    }
    let text = match std::fs::read_to_string("app-config.toml") {
        Ok(text) => text,
        Err(e) => {
            godot_error!("Failed to read app-config.toml: {}", e);
            return None;
        }
    };
    let main = match toml::de::from_str::<TmpConfig>(&text) {
        Ok(config) => config.main,
        Err(e) => {
            godot_error!("Failed to parse app-config.toml: {}", e);
            return None;
        }
    };

    let file = match std::fs::File::open(&main.robot_layout) {
        Ok(file) => file,
        Err(e) => {
            godot_error!("Failed to open robot layout file {}: {}", main.robot_layout, e);
            return None;
        }
    };
    let robot_chain = match NodeSerde::from_reader(
        file,
    ) {
        Ok(chain) => chain,
        Err(e) => {
            godot_error!("Failed to parse robot chain: {}", e);
            return None;
        }
    };
    let robot_chain = ChainBuilder::from(robot_chain).finish_static();
    let mut camera_nodes = Vec::new();

    for (_port, CameraInfo { stream_index, link_name }) in main.cameras.into_iter().chain(main.depth_cameras) {
        let Some(node) = robot_chain.get_node_with_name(&link_name) else {
            godot_error!("Camera link {} not found in robot chain", link_name);
            continue;
        };
        if camera_nodes.len() <= stream_index {
            camera_nodes.resize(stream_index + 1, None);
        }
        if camera_nodes[stream_index].is_some() {
            godot_error!(
                "Camera stream index {} already occupied",
                stream_index,
            );
            continue;
        }
        camera_nodes[stream_index] = Some(node);
    }

    Some(AppConfig {
        camera_nodes: Box::leak(camera_nodes.into_boxed_slice()),
        robot_chain,
    })
}