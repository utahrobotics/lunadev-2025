use std::sync::LazyLock;

use godot::builtin::math::ApproxEq;
use godot::prelude::*;
use godot::{builtin::Transform3D, global::godot_error};
use k::{Chain, Isometry3};

static ROBOT_CHAIN: LazyLock<Chain<f64>> = LazyLock::new(|| {
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
    #[export]
    verify_only: bool,
}

#[godot_api]
impl INode3D for RobotNode {
    fn init(base: Base<Node3D>) -> Self {
        Self {
            base,
            link_name: StringName::default(),
            last_link_name: StringName::default(),
            k_node: None,
            verify_only: false,
        }
    }

    fn physics_process(&mut self, _delta: f64) {
        let get_transform = |transform: Isometry3<f64>| {
            Transform3D::new(
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
            )
        };
        if self.link_name != self.last_link_name {
            self.k_node = ROBOT_CHAIN.find_link(&self.link_name.to_string());
            if !self.link_name.is_empty() && self.k_node.is_none() {
                godot_error!("Failed to find link: {}", self.link_name);
            }
            self.last_link_name = self.link_name.clone();

            if self.verify_only {
                if let Some(node) = self.k_node {
                    let correct = get_transform(node.origin());
                    if !self.base().get_global_transform().approx_eq(&correct) {
                        let mut rotation = correct.basis.to_euler(self.base().get_rotation_order());
                        rotation.x = rotation.x.to_degrees();
                        rotation.y = rotation.y.to_degrees();
                        rotation.z = rotation.z.to_degrees();
                        godot_error!("Transform mismatch for {}. Please set origin to {} and rotation to {rotation}", self.base().get_path(), correct.origin);
                    }
                }
            }
        }
        if self.verify_only {
            return;
        }

        if let Some(node) = self.k_node {
            let transform = node.origin();
            self.base_mut()
                .set_global_transform(get_transform(transform));
        }
    }
}
