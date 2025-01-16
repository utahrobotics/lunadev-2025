use std::{collections::VecDeque, ops::Deref, sync::Arc};

use crossbeam::atomic::AtomicCell;
use nalgebra::{Isometry3, Point3, UnitQuaternion, UnitVector3, Vector3};
use serde::Deserialize;
use tracing::error;

pub enum TranslationRestriction {
    Fixed {
        origin: Point3<f64>,
    },
    Linear {
        start_origin: Point3<f64>,
        axis: UnitVector3<f64>,
        min_length: Option<f64>,
        max_length: Option<f64>,
        current_length: Option<f64>,
    },
    Free {
        origin: Point3<f64>,
    },
}

pub enum RotationRestriction {
    Fixed {
        rotation: UnitQuaternion<f64>,
    },
    OneAxis {
        start_rotation: UnitQuaternion<f64>,
        axis: UnitVector3<f64>,
        min_angle: Option<f64>,
        max_angle: Option<f64>,
        current_angle: Option<f64>,
    },
    Free {
        rotation: UnitQuaternion<f64>,
    },
}

#[derive(Clone, Copy)]
struct LinearDynamicState {
    current_origin: Point3<f64>,
    current_length: f64,
}

enum TranslationRestrictionState {
    Fixed {
        origin: Point3<f64>,
    },
    Linear {
        start_origin: Point3<f64>,
        axis: UnitVector3<f64>,
        min_length: Option<f64>,
        max_length: Option<f64>,
        dynamic: AtomicCell<LinearDynamicState>,
    },
    Free {
        origin: AtomicCell<Point3<f64>>,
    },
}

impl From<TranslationRestriction> for TranslationRestrictionState {
    fn from(translation: TranslationRestriction) -> Self {
        match translation {
            TranslationRestriction::Fixed { origin } => {
                TranslationRestrictionState::Fixed { origin }
            }
            TranslationRestriction::Linear {
                start_origin,
                axis,
                min_length,
                max_length,
                current_length
            } => TranslationRestrictionState::Linear {
                start_origin,
                axis,
                min_length,
                max_length,
                dynamic: AtomicCell::new(if let Some(current_length) = current_length {
                    LinearDynamicState {
                        current_origin: start_origin + axis.into_inner() * current_length,
                        current_length,
                    }

                } else if let Some(min_length) = min_length {
                    LinearDynamicState {
                        current_origin: start_origin + axis.into_inner() * min_length,
                        current_length: min_length,
                    }

                } else {
                    LinearDynamicState {
                        current_origin: start_origin,
                        current_length: 0.0,
                    }
                }),
            },
            TranslationRestriction::Free { origin } => TranslationRestrictionState::Free {
                origin: AtomicCell::new(origin),
            },
        }
    }
}

#[derive(Clone, Copy)]
struct OneAxisDynamicState {
    current_rotation: UnitQuaternion<f64>,
    current_angle: f64,
}

enum RotationRestrictionState {
    Fixed {
        rotation: UnitQuaternion<f64>,
    },
    OneAxis {
        start_rotation: UnitQuaternion<f64>,
        axis: UnitVector3<f64>,
        min_angle: Option<f64>,
        max_angle: Option<f64>,
        dynamic: AtomicCell<OneAxisDynamicState>,
    },
    Free {
        rotation: AtomicCell<UnitQuaternion<f64>>,
    },
}

impl From<RotationRestriction> for RotationRestrictionState {
    fn from(rotation: RotationRestriction) -> Self {
        match rotation {
            RotationRestriction::Fixed { rotation } => {
                RotationRestrictionState::Fixed { rotation }
            }
            RotationRestriction::OneAxis {
                start_rotation,
                axis,
                min_angle,
                max_angle,
                current_angle
            } => RotationRestrictionState::OneAxis {
                start_rotation,
                axis,
                min_angle,
                max_angle,
                dynamic: AtomicCell::new(if let Some(current_angle) = current_angle {
                    OneAxisDynamicState {
                        current_rotation: UnitQuaternion::from_axis_angle(&axis, current_angle) * start_rotation,
                        current_angle,
                    }
                } else if let Some(min_angle) = min_angle {
                    OneAxisDynamicState {
                        current_rotation: UnitQuaternion::from_axis_angle(&axis, min_angle) * start_rotation,
                        current_angle: min_angle,
                    }
                } else {
                    OneAxisDynamicState {
                        current_rotation: start_rotation,
                        current_angle: 0.0,
                    }
                }),
            },
            RotationRestriction::Free { rotation } => RotationRestrictionState::Free {
                rotation: AtomicCell::new(rotation),
            },
        }
    }
}

pub struct ImmutableTransformable {
    translation_restriction: TranslationRestrictionState,
    rotation_restriction: RotationRestrictionState,
}

#[repr(transparent)]
pub struct Transformable(ImmutableTransformable);

impl ImmutableTransformable {
    pub fn get_local_origin(&self) -> Point3<f64> {
        match &self.translation_restriction {
            TranslationRestrictionState::Fixed { origin } => *origin,
            TranslationRestrictionState::Linear { dynamic, .. } => dynamic.load().current_origin,
            TranslationRestrictionState::Free { origin } => origin.load(),
        }
    }

    pub fn get_local_rotation(&self) -> UnitQuaternion<f64> {
        match &self.rotation_restriction {
            RotationRestrictionState::Fixed { rotation } => *rotation,
            RotationRestrictionState::OneAxis { dynamic, .. } => dynamic.load().current_rotation,
            RotationRestrictionState::Free { rotation } => rotation.load(),
        }
    }

    pub fn get_local_isometry(&self) -> Isometry3<f64> {
        Isometry3::from_parts(self.get_local_origin().into(), self.get_local_rotation())
    }

    pub fn get_local_length(&self) -> Option<f64> {
        match &self.translation_restriction {
            TranslationRestrictionState::Linear { dynamic, .. } => {
                Some(dynamic.load().current_length)
            }
            _ => None,
        }
    }

    pub fn get_local_angle_one_axis(&self) -> Option<f64> {
        match &self.rotation_restriction {
            RotationRestrictionState::OneAxis { dynamic, .. } => Some(dynamic.load().current_angle),
            _ => None,
        }
    }

    pub fn is_origin_fixed(&self) -> bool {
        matches!(&self.translation_restriction, TranslationRestrictionState::Fixed { .. })
    }

    pub fn is_rotation_fixed(&self) -> bool {
        matches!(&self.rotation_restriction, RotationRestrictionState::Fixed { .. })
    }

    pub fn is_origin_linear(&self) -> bool {
        matches!(&self.translation_restriction, TranslationRestrictionState::Linear { .. })
    }

    pub fn is_rotation_one_axis(&self) -> bool {
        matches!(&self.rotation_restriction, RotationRestrictionState::OneAxis { .. })
    }

    pub fn is_origin_free(&self) -> bool {
        matches!(&self.translation_restriction, TranslationRestrictionState::Free { .. })
    }

    pub fn is_rotation_free(&self) -> bool {
        matches!(&self.rotation_restriction, RotationRestrictionState::Free { .. })
    }
}

impl Transformable {
    pub fn try_set_origin(&self, new_origin: Point3<f64>) -> bool {
        match &self.0.translation_restriction {
            TranslationRestrictionState::Free { origin } => {
                origin.store(new_origin);
                true
            }
            _ => false,
        }
    }

    pub fn set_origin(&self, new_origin: Point3<f64>) {
        if !self.try_set_origin(new_origin) {
            error!("Cannot set origin for a non-free translation restriction");
        }
    }

    pub fn try_set_length(&self, mut new_length: f64) -> bool {
        match &self.0.translation_restriction {
            TranslationRestrictionState::Linear {
                dynamic,
                start_origin,
                axis,
                min_length,
                max_length,
            } => {
                if let Some(min_length) = min_length {
                    new_length = new_length.max(*min_length);
                }
                if let Some(max_length) = max_length {
                    new_length = new_length.min(*max_length);
                }
                let new = LinearDynamicState {
                    current_origin: start_origin + axis.into_inner() * new_length,
                    current_length: new_length,
                };
                dynamic.store(new);
                true
            }
            _ => false,
        }
    }

    pub fn set_length(&self, new_length: f64) {
        if !self.try_set_length(new_length) {
            error!("Cannot set length for a non-linear translation restriction");
        }
    }

    pub fn try_set_angle_one_axis(&self, mut new_angle: f64) -> bool {
        match &self.0.rotation_restriction {
            RotationRestrictionState::OneAxis {
                dynamic,
                start_rotation,
                axis,
                min_angle,
                max_angle,
            } => {
                if let Some(min_angle) = min_angle {
                    new_angle = new_angle.max(*min_angle);
                }
                if let Some(max_angle) = max_angle {
                    new_angle = new_angle.min(*max_angle);
                }
                let new = OneAxisDynamicState {
                    current_rotation: UnitQuaternion::from_axis_angle(&axis, new_angle)
                        * start_rotation,
                    current_angle: new_angle,
                };
                dynamic.store(new);
                true
            }
            _ => false,
        }
    }

    pub fn set_angle_one_axis(&self, new_angle: f64) {
        if !self.try_set_angle_one_axis(new_angle) {
            error!("Cannot set angle for a non-one-axis rotation restriction");
        }
    }
}

impl Deref for Transformable {
    type Target = ImmutableTransformable;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub struct NodeData {
    transformable: ImmutableTransformable,
    parent: Option<usize>,
    name: Option<Box<str>>,
}

#[derive(Clone, Copy)]
pub struct ImmutableNode<S = Arc<[NodeData]>> {
    arena: S,
    index: usize,
}

impl<S: Deref<Target = [NodeData]>> Deref for ImmutableNode<S> {
    type Target = ImmutableTransformable;

    fn deref(&self) -> &Self::Target {
        &self.arena[self.index].transformable
    }
}

impl<S> From<Node<S>> for ImmutableNode<S> {
    fn from(node: Node<S>) -> Self {
        node.0
    }
}

impl<S: Deref<Target = [NodeData]> + Clone> ImmutableNode<S> {
    pub fn get_parent(&self) -> Option<Self> {
        self.arena[self.index]
            .parent
            .map(|parent_index| ImmutableNode {
                arena: self.arena.clone(),
                index: parent_index,
            })
    }

    pub fn get_root(&self) -> Self {
        ImmutableNode {
            arena: self.arena.clone(),
            index: 0,
        }
    }

    pub fn get_node_with_name(&self, name: &str) -> Option<Self> {
        self.arena.iter().enumerate().find_map(|(index, node)| {
            if node.name.as_deref() == Some(name) {
                Some(ImmutableNode {
                    arena: self.arena.clone(),
                    index,
                })
            } else {
                None
            }
        })
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct Node<S = Arc<[NodeData]>>(ImmutableNode<S>);

impl<S: Deref<Target = [NodeData]>> Deref for Node<S> {
    type Target = Transformable;

    fn deref(&self) -> &Self::Target {
        let immut: &ImmutableTransformable = &self.0.arena[self.0.index].transformable;
        unsafe { std::mem::transmute(immut) }
    }
}

impl<S: Deref<Target = [NodeData]> + Clone> Node<S> {
    pub fn get_parent(&self) -> Option<Self> {
        self.0.get_parent().map(Node)
    }

    pub fn get_root(&self) -> Self {
        Node(self.0.get_root())
    }

    pub fn get_node_with_name(&self, name: &str) -> Option<Self> {
        self.0.get_node_with_name(name).map(Node)
    }
}

pub type StaticImmutableNode = ImmutableNode<&'static [NodeData]>;
pub type StaticNode = Node<&'static [NodeData]>;

pub struct ChainBuilder {
    nodes: Vec<NodeData>,
}

impl ChainBuilder {
    pub fn new_fixed() -> Self {
        Self::new(TranslationRestriction::Fixed { origin: Point3::origin() }, RotationRestriction::Fixed { rotation: UnitQuaternion::identity() })
    }

    pub fn new_free() -> Self {
        Self::new(TranslationRestriction::Free { origin: Point3::origin() }, RotationRestriction::Free { rotation: UnitQuaternion::identity() })
    }

    pub fn new(
        translation: TranslationRestriction,
        rotation: RotationRestriction,
    ) -> Self {
        Self {
            nodes: vec![NodeData {
                transformable: ImmutableTransformable {
                    translation_restriction: translation.into(),
                    rotation_restriction: rotation.into(),
                },
                parent: None,
                name: None,
            }],
        }
    }

    pub fn add_node(
        &mut self,
        parent: usize,
        translation: TranslationRestriction,
        rotation: RotationRestriction,
    ) -> usize {
        let new_index = self.nodes.len();
        self.nodes.push(NodeData {
            transformable: ImmutableTransformable {
                translation_restriction: translation.into(),
                rotation_restriction: rotation.into(),
            },
            parent: Some(parent),
            name: None,
        });
        new_index
    }

    pub fn set_node_name(&mut self, node: usize, name: impl Into<Box<str>>) {
        self.nodes[node].name = Some(name.into());
    }

    pub fn finish_with<S>(self, f: impl FnOnce(Vec<NodeData>) -> S) -> Node<S> {
        Node(ImmutableNode {
            arena: f(self.nodes),
            index: 0
        })
    }

    pub fn finish(self) -> Node {
        self.finish_with(|nodes| Arc::from(nodes.into_boxed_slice()))
    }

    pub fn finish_static(self) -> StaticNode {
        self.finish_with(|nodes| {
            let out: &_ = Box::leak(nodes.into_boxed_slice());
            out
        })
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum TranslationRestrictionSerde {
    Free {
        #[serde(skip_serializing_if = "all_zeros")]
        free_origin: [f64; 3],
    },
    Linear {
        #[serde(default)]
        #[serde(skip_serializing_if = "all_zeros")]
        start_origin: [f64; 3],
        axis: [f64; 3],
        #[serde(skip_serializing_if = "Option::is_none")]
        min_length: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        max_length: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        current_length: Option<f64>,
    },
    Fixed {
        #[serde(default)]
        #[serde(skip_serializing_if = "all_zeros")]
        origin: [f64; 3],
    },
}

impl Default for TranslationRestrictionSerde {
    fn default() -> Self {
        TranslationRestrictionSerde::Fixed { origin: [0.0, 0.0, 0.0] }
    }
}

impl From<TranslationRestrictionSerde> for TranslationRestriction {
    fn from(translation: TranslationRestrictionSerde) -> Self {
        match translation {
            TranslationRestrictionSerde::Fixed { origin } => {
                TranslationRestriction::Fixed { origin: Point3::from(origin) }
            }
            TranslationRestrictionSerde::Linear { start_origin, axis, min_length, max_length, current_length } => {
                TranslationRestriction::Linear {
                    start_origin: Point3::from(start_origin),
                    axis: UnitVector3::try_new(Vector3::from(axis), 0.1).expect("Axis is too short"),
                    min_length,
                    max_length,
                    current_length,
                }
            }
            TranslationRestrictionSerde::Free { free_origin } => {
                TranslationRestriction::Free { origin: Point3::from(free_origin) }
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RotationRestrictionSerde {
    Free {
        #[serde(skip_serializing_if = "all_zeros")]
        free_euler: [f64; 3],
    },
    OneAxis {
        #[serde(default)]
        #[serde(skip_serializing_if = "all_zeros")]
        start_euler: [f64; 3],
        axis: [f64; 3],
        min_angle: Option<f64>,
        max_angle: Option<f64>,
        current_angle: Option<f64>,
    },
    Fixed {
        #[serde(default)]
        #[serde(skip_serializing_if = "all_zeros")]
        euler: [f64; 3],
    },
}

impl Default for RotationRestrictionSerde {
    fn default() -> Self {
        RotationRestrictionSerde::Fixed { euler: [0.0, 0.0, 0.0] }
    }
}


impl From<RotationRestrictionSerde> for RotationRestriction {
    fn from(rotation: RotationRestrictionSerde) -> Self {
        match rotation {
            RotationRestrictionSerde::Fixed { euler } => {
                RotationRestriction::Fixed { rotation: UnitQuaternion::from_euler_angles(euler[0], euler[1], euler[2]) }
            }
            RotationRestrictionSerde::OneAxis { start_euler, axis, min_angle, max_angle, current_angle } => {
                RotationRestriction::OneAxis {
                    start_rotation: UnitQuaternion::from_euler_angles(start_euler[0], start_euler[1], start_euler[2]),
                    axis: UnitVector3::try_new(Vector3::from(axis), 0.1).expect("Axis is too short"),
                    min_angle,
                    max_angle,
                    current_angle,
                }
            }
            RotationRestrictionSerde::Free { free_euler } => {
                RotationRestriction::Free { rotation: UnitQuaternion::from_euler_angles(free_euler[0], free_euler[1], free_euler[2]) }
            }
        }
    }
}

#[allow(unused)]
fn all_zeros(v: &[f64; 3]) -> bool {
    v.iter().all(|&x| x == 0.0)
}


#[derive(Deserialize)]
pub struct NodeSerde {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    children: Vec<NodeSerde>,
    #[serde(default, flatten)]
    fields: serde_json::Map<String, serde_json::Value>,
}

impl From<NodeSerde> for ChainBuilder {
    fn from(NodeSerde { name, fields, children }: NodeSerde) -> Self {
        let translation: TranslationRestrictionSerde = serde_json::from_value(serde_json::Value::Object(fields.clone())).unwrap();
        let rotation: RotationRestrictionSerde = serde_json::from_value(serde_json::Value::Object(fields)).unwrap();

        let mut builder = ChainBuilder::new(translation.into(), rotation.into());
        if let Some(name) = name {
            builder.set_node_name(0, name);
        }

        let mut queue: VecDeque<_> = children.into_iter().zip(std::iter::repeat(0)).collect();

        while let Some((node_serde, parent_idx)) = queue.pop_front() {
            let translation: TranslationRestrictionSerde = serde_json::from_value(serde_json::Value::Object(node_serde.fields.clone())).unwrap();
            let rotation: RotationRestrictionSerde = serde_json::from_value(serde_json::Value::Object(node_serde.fields)).unwrap();
            
            let index = builder.add_node(parent_idx, translation.into(), rotation.into());
            if let Some(name) = node_serde.name {
                builder.set_node_name(index, name);
            }
            queue.extend(node_serde.children.into_iter().zip(std::iter::repeat(index)));
        }

        builder
    }
}

impl NodeSerde {
    pub fn from_str(s: &str) -> serde_json::Result<Self> {
        serde_json::from_str(s)
    }

    pub fn from_reader<R: std::io::Read>(r: R) -> serde_json::Result<Self> {
        serde_json::from_reader(r)
    }
}

#[cfg(test)]
mod tests {
    use crate::{ChainBuilder, NodeSerde};

    #[test]
    fn simple_deserialize_json01() {
        let json = serde_json::json!({});
        let node_serde: NodeSerde = serde_json::from_value(json).unwrap();
        let builder = ChainBuilder::from(node_serde);
        let node = builder.finish();
        assert!(node.is_origin_fixed());
        assert!(node.is_rotation_fixed());

        assert_eq!(node.get_local_origin(), nalgebra::Point3::origin());
        assert_eq!(node.get_local_rotation(), nalgebra::UnitQuaternion::identity());
    }

    #[test] 
    fn simple_deserialize_json02() {
        let json = serde_json::json!({
            "origin": [0.0, 1.0, 0.0]
        });
        let node_serde: NodeSerde = serde_json::from_value(json).unwrap();
        let builder = ChainBuilder::from(node_serde);
        let node = builder.finish();
        assert!(node.is_origin_fixed());
        assert!(node.is_rotation_fixed());

        assert_eq!(node.get_local_origin(), nalgebra::Point3::new(0.0, 1.0, 0.0));
        assert_eq!(node.get_local_rotation(), nalgebra::UnitQuaternion::identity());
    }
}