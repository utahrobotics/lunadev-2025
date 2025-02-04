use std::time::{Duration, Instant};

use ares_bt::{
    converters::AssertCancelSafe,
    looping::WhileLoop,
    sequence::{ParallelAny, Sequence},
    Behavior, InfallibleStatus, Status,
};
use common::{FromLunabase, Steering};
use dig::dig;
use dump::dump;
use nalgebra::{distance, Const, Matrix2, OPoint, Point2, Vector2, Vector3};
use tracing::{error, warn};
use traverse::traverse;

use crate::{blackboard::LunabotBlackboard, Action, PollWhen};

mod dig;
mod dump;
mod traverse;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AutonomyStage {
    TraverseObstacles,
    Dig,
    Dump,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Autonomy {
    FullAutonomy(AutonomyStage),
    PartialAutonomy(AutonomyStage),
    None,
}

impl Autonomy {
    fn advance(&mut self) {
        match *self {
            Autonomy::FullAutonomy(autonomy_stage) => match autonomy_stage {
                AutonomyStage::TraverseObstacles => {
                    *self = Autonomy::FullAutonomy(AutonomyStage::Dig)
                }
                AutonomyStage::Dig => *self = Autonomy::FullAutonomy(AutonomyStage::Dump),
                AutonomyStage::Dump => *self = Autonomy::FullAutonomy(AutonomyStage::Dig),
            },
            Autonomy::PartialAutonomy(_) => *self = Self::None,
            Autonomy::None => {}
        }
    }
}

pub fn autonomy() -> impl Behavior<LunabotBlackboard> {
    WhileLoop::new(
        |blackboard: &mut LunabotBlackboard| (*blackboard.get_autonomy() != Autonomy::None).into(),
        ParallelAny::new((
            AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
                if *blackboard.lunabase_disconnected() {
                    error!("Lunabase disconnected");
                    return Status::Failure;
                }
                while let Some(msg) = blackboard.peek_from_lunabase() {
                    match msg {
                        FromLunabase::Steering(_) => {
                            *blackboard.get_autonomy() = Autonomy::None;
                            warn!("Received steering message while in autonomy mode");
                            return Status::Success;
                        }
                        FromLunabase::SoftStop => {
                            blackboard.pop_from_lunabase();
                            return Status::Failure;
                        }
                        _ => blackboard.pop_from_lunabase(),
                    };
                }
                Status::Running
            }),
            Sequence::new((dig(), dump(), traverse())),
        )),
    )
}

fn follow_path(blackboard: &mut LunabotBlackboard) -> InfallibleStatus {
    let robot = blackboard.get_robot_isometry();
    
    let path = blackboard.get_path_mut();

    let pos = Point2::new(robot.translation.x, robot.translation.z);
    let heading = robot
        .rotation
        .transform_vector(&Vector3::new(0.0, 0.0, -1.0))
        .xz();

    match find_target_point(pos, path) {
        Some(target) => {
            let heading_angle = heading.angle(&Vector2::new(0.0, -1.0));
            let to_first_point = (target - pos).normalize();

            // direction to first point of path, from robot's pov
            let to_first_point = if heading.x < 0.0 {
                rotate_v2_ccw(to_first_point, heading_angle)
            } else {
                rotate_v2_ccw(to_first_point, -heading_angle)
            };

            *blackboard.get_poll_when() =
                PollWhen::Instant(Instant::now() + Duration::from_millis(16));

            if to_first_point.angle(&Vector2::new(0.0, -1.0)).to_degrees() > 20.0 {
                if to_first_point.x > 0.0 {
                    blackboard
                    .enqueue_action(Action::SetSteering(Steering::new_left_right(1.0, -1.0)))
                } else {
                    blackboard
                        .enqueue_action(Action::SetSteering(Steering::new_left_right(-1.0, 1.0)))
                }
            } else {
                let (l, r) = scaled_clamp(
                    -to_first_point.y + to_first_point.x * 1.2,
                    -to_first_point.y - to_first_point.x * 1.2,
                    1.0,
                );
                blackboard.enqueue_action(Action::SetSteering(Steering::new_left_right(l, r)))
            }
            InfallibleStatus::Running
        }
        None => {
            blackboard.enqueue_action(Action::SetSteering(Steering::default()));
            InfallibleStatus::Success
        }
    }
}

/// min distance for robot to be considered at a point
const AT_POINT_THRESHOLD: f64 = 0.2;

/// find index of the next point the robot should move towards, based on which path segment the robot is closest to
///
/// returns `None` if robot is at the last point
fn find_target_point(pos: Point2<f64>, path: &mut Vec<OPoint<f64, Const<3>>>) -> Option<Point2<f64>> {
    
    if distance(&pos, &path[path.len() - 1].xz()) < AT_POINT_THRESHOLD {
        println!("path follower: done!", );
        return None;
    }

    let first_point = path[0].xz();

    // if the robot is near the first point, delete it
    if distance(&first_point, &pos) < AT_POINT_THRESHOLD {
        path.remove(0);
        return Some(path[0].xz());
    }

    Some(first_point)
}

fn rotate_v2_ccw(vector2: Vector2<f64>, theta: f64) -> Vector2<f64> {
    let rot = Matrix2::new(
        f64::cos(theta),
        -f64::sin(theta),
        f64::sin(theta),
        f64::cos(theta),
    );
    return rot * vector2;
}

/// clamps `a` and `b` so that `a.abs().max(b.abs) <= bound.abs()`,
/// while maintaining the ratio between `a` and `b`
fn scaled_clamp(a: f64, b: f64, bound: f64) -> (f64, f64) {
    let bound = bound.abs();

    if a.abs().max(b.abs()) <= bound {
        (a, b)
    } else if a.abs() > b.abs() {
        (bound * a.signum(), (bound * b / a).abs() * b.signum())
    } else {
        ((bound * a / b).abs() * a.signum(), bound * b.signum())
    }
}
