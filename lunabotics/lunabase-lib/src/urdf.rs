use std::sync::LazyLock;

use godot::prelude::*;
use godot::{builtin::Transform3D, global::godot_error};
use k::Chain;

pub static ROBOT_CHAIN: LazyLock<Chain<f64>> = LazyLock::new(|| {
    let chain =
        Chain::<f64>::from_urdf_file("../../urdf/lunabot.urdf").expect("Failed to load urdf");
    chain.update_transforms();
    chain
});

#[derive(GodotClass)]
#[class(base=Node3D)]
struct RobotNode {
    base: Base<Node3D>,
    #[export]
    link_name: StringName,
    last_link_name: StringName,
    k_node: Option<&'static k::Node<f64>>,
}

#[godot_api]
impl INode3D for RobotNode {
    fn init(base: Base<Node3D>) -> Self {
        Self {
            base,
            link_name: StringName::default(),
            last_link_name: StringName::default(),
            k_node: None,
        }
    }

    fn physics_process(&mut self, _delta: f64) {
        if self.link_name != self.last_link_name {
            self.k_node = ROBOT_CHAIN.find_link(&self.link_name.to_string());
            if !self.link_name.is_empty() && self.k_node.is_none() {
                godot_error!("Failed to find link: {}", self.link_name);
            }
            self.last_link_name = self.link_name.clone();
        }

        if let Some(node) = self.k_node {
            let transform = node.origin();
            self.base_mut().set_global_transform(Transform3D::new(
                Basis::from_quat(Quaternion::new(
                    transform.rotation.coords.x as f32,
                    transform.rotation.coords.y as f32,
                    transform.rotation.coords.z as f32,
                    transform.rotation.coords.w as f32,
                )),
                Vector3::new(
                    transform.translation.x as f32,
                    transform.translation.y as f32,
                    transform.translation.z as f32,
                ),
            ));
        }
    }
}
