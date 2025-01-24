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
use nalgebra::{distance, Const, Matrix2, OPoint, Point2, Point3, Vector2, Vector3};
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
    ParallelAny::new((
        Sequence::new((
            AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
                blackboard.get_path_mut().clear();
                let pos = blackboard.get_robot_isometry().translation;
                let pos = Point3::new(pos.x, pos.y, pos.z);
                blackboard.get_path_mut().extend_from_slice(&[
                    pos,
                    // Point3::new(-0.9144, 0.0, -1.905),
                    Point3::new(1.905, 0.0, -1.5),
                    Point3::new(1.27, 0.0, -0.5),
                ]);
                println!("set path", );
                Status::Success
            }),
            ares_bt::converters::InfallibleShim(AssertCancelSafe(follow_path))
        )),
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
    ))
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
            println!("going to {} from {}", target, pos);
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
                // PollWhen::Instant(Instant::now());

            // We reborrow path so that we can mutably access get_poll_when
            // let path = blackboard.get_path_mut();

            // when approaching an arc turn gradually
            // if distance(&pos, &target) < ARC_THRESHOLD { // && within_arc(path, target) {
                // let (l, r) = scaled_clamp(
                //     -to_first_point.y + to_first_point.x,
                //     -to_first_point.y - to_first_point.x,
                //     1,
                // );
                // blackboard.enqueue_action(Action::SetSteering(Steering::new_left_right(l, r)));
                // return InfallibleStatus::Running;
            // }
            println!("angle {} to next pt {}", to_first_point.angle(&Vector2::new(0.0, -1.0)), to_first_point);
            if to_first_point.angle(&Vector2::new(0.0, -1.0)).to_degrees() > 20.0 {
                if to_first_point.x > 0.0 {
                    println!("turning right", );
                    blackboard
                    .enqueue_action(Action::SetSteering(Steering::new_left_right(1.0, -1.0)))
                } else {
                    println!("turning left", );
                    blackboard
                        .enqueue_action(Action::SetSteering(Steering::new_left_right(-1.0, 1.0)))
                }
            } else {
                println!("straight ahead ", );
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

    // if the robot is near any point, delete all previous points
    for (i, point) in path.iter().enumerate().rev() {
        if distance(&point.xz(), &pos) < AT_POINT_THRESHOLD {
            println!("path follower: made it to point {}", point);

            // let res = Some(path[i+1].xz());
            path.drain(0..=i);

            println!("drained path to {:?}", path);
            return Some(path[0].xz());
        }
    }
    //TODO: repeat for line segments

    Some(path[0].xz())

    // let mut min_dist = distance(&pos, &path[0].xz());
    // let mut res = path[0].xz();

    // for i in 1..path.len() {
    //     let dist = dist_to_segment(pos, path[i - 1].xz(), path[i].xz());

    //     if dist < min_dist {
    //         min_dist = dist;
    //         res = path[i].xz();
    //     }
    // }
}

fn dist_to_segment(point: Point2<f64>, a: Point2<f64>, b: Point2<f64>) -> f64 {
    let mut line_from_origin = b - a; // move line segment to origin
    let mut point = point - a; // move point the same amount

    let angle = -line_from_origin.y.signum() * line_from_origin.angle(&Vector2::new(1.0, 0.0));

    // rotate both until segment lines up with the x axis
    line_from_origin = rotate_v2_ccw(line_from_origin, angle);
    point = rotate_v2_ccw(point, angle);

    return if point.x <= 0.0 {
        point.magnitude()
    } else if point.x >= line_from_origin.x {
        (point - Vector2::new(line_from_origin.x, 0.0)).magnitude()
    } else {
        point.y.abs()
    };
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

/// min distance for 2 path points to be considered part of an arc
const ARC_THRESHOLD: f64 = 0.3;

/// is this point considered part of an arc?
fn within_arc(path: &[Point3<f64>], i: usize) -> bool {
    if path.len() == 1 {
        false
    } else if i == path.len() - 1 {
        distance(&path[i].xz(), &path[i - 1].xz()) < ARC_THRESHOLD
    } else {
        distance(&path[i].xz(), &path[i + 1].xz()) < ARC_THRESHOLD
    }
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
