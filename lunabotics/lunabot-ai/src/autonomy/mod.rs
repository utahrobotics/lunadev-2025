use std::time::{Duration, Instant};

use ares_bt::{
    converters::AssertCancelSafe,
    looping::WhileLoop,
    sequence::{ParallelAny, Sequence},
    Behavior, Status,
};
use common::{FromLunabase, PathInstruction, Steering};
use dig::dig;
use dump::dump;
use nalgebra::{distance, Matrix2, Point3, Vector2, Vector3};
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
            
            traverse(),
            // Sequence::new(( dig(), dump(), traverse())), // TODO enable dig, dump
        )),
    )
}

/// how far the robot should back up when its stuck in one spot for too long
const BACKING_AWAY_DISTANCE: f64 = 0.3;

/// max time the robot spends in one spot before backing up and pathfinding again
const MAX_STUCK_DURATION: Duration = Duration::from_millis(1500);

/// min distance moved until `blackboard.latest_transform` gets updated
const MIN_DIST_UNTIL_TRANSFORM_UPDATE: f64 = 0.01;

/// min angle moved until `blackboard.latest_transform` gets updated
const MIN_ANGLE_UNTIL_TRANSFORM_UPDATE: f64 = 0.1;

fn follow_path(blackboard: &mut LunabotBlackboard) -> Status {
    let robot = blackboard.get_robot_isometry();
    let pos: Point3<f64> = robot.translation.vector.into();
    
    if let Some(backing_away_from) = blackboard.backing_away_from() {
        
        if distance(&pos.xz(), &backing_away_from.xz()) > BACKING_AWAY_DISTANCE {
            println!("path follower: finished backing up");
            blackboard.enqueue_action(Action::SetSteering(Steering::default()));
            *blackboard.backing_away_from() = None;
            return Status::Failure; // return failure to restart traverse section of behavior tree
        }
        
        blackboard.enqueue_action(Action::SetSteering(Steering::new(-1.0, 0.0, Steering::DEFAULT_WEIGHT)));
        return Status::Running;
    }
    
    let latest_transform = blackboard.get_latest_transform();
    let now = blackboard.get_now();
    let path = blackboard.get_path_mut();
    
    if path.is_empty() {
        println!("path follower: empty path", );
        blackboard.enqueue_action(Action::SetSteering(Steering::default()));
        return Status::Running;
    }

    let curr_instr = path[0];
    let heading = robot
        .rotation
        .transform_vector(&Vector3::new(0.0, 0.0, -1.0))
        .xz();

    if curr_instr.is_finished(&pos.xz(), &heading.into()) {
        path.remove(0);
        let path_complete = path.is_empty();

        if path_complete {
            blackboard.enqueue_action(Action::SetSteering(Steering::default()));
            
            // ensures that time between path follows aren't interpreted as being stuck in one place for a long time
            blackboard.clear_latest_transform();
            
            return match curr_instr.instruction {
                PathInstruction::MoveTo => {
                    println!("path follower: done!");
                    blackboard.enqueue_action(Action::ClearPointsToAvoid);
                    Status::Success
                }
                PathInstruction::FaceTowards => Status::Failure
            };
        }
        
        return Status::Running;
    }

    
    match latest_transform {
        None => blackboard.set_latest_transform(pos, robot.rotation),
        Some((prev_pos, prev_rot, time_of_prev_pos)) => {
            
            // robot transform has changed enough to update `latest_transform`
            if 
                distance(&prev_pos, &pos) > MIN_DIST_UNTIL_TRANSFORM_UPDATE ||
                prev_rot.angle_to(&robot.rotation) > MIN_ANGLE_UNTIL_TRANSFORM_UPDATE
            {
                blackboard.set_latest_transform(pos, robot.rotation);
            }
            
            // robot has been here for a while now, avoid this spot and start backing up
            else if now.duration_since(time_of_prev_pos) > MAX_STUCK_DURATION {
                println!("path follower: been same pos for too long, starting to back up");
                blackboard.enqueue_action(Action::AvoidPoint(pos));
                *blackboard.backing_away_from() = Some(pos);
                return Status::Running;
            }
        },
    };
    
    let heading_angle = heading.angle(&Vector2::new(0.0, -1.0));
    let to_first_point = (curr_instr.point.xz() - pos.xz()).normalize();

    // direction to first point of path, from robot's pov
    let to_first_point = if heading.x < 0.0 {
        rotate_v2_ccw(to_first_point, heading_angle)
    } else {
        rotate_v2_ccw(to_first_point, -heading_angle)
    };

    match curr_instr.instruction {
        PathInstruction::MoveTo => {
            if to_first_point.angle(&Vector2::new(0.0, -1.0)).to_degrees() > 20.0 {
                if to_first_point.x > 0.0 {
                    blackboard
                        .enqueue_action(Action::SetSteering(Steering::new(1.0, -1.0, Steering::DEFAULT_WEIGHT)))
                } else {
                    blackboard
                        .enqueue_action(Action::SetSteering(Steering::new(-1.0, 1.0, Steering::DEFAULT_WEIGHT)))
                }
            } else {
                let (l, r) = scaled_clamp(
                    -to_first_point.y + to_first_point.x * 1.2,
                    -to_first_point.y - to_first_point.x * 1.2,
                    1.0,
                );
                blackboard.enqueue_action(Action::SetSteering(Steering::new(l, r, Steering::DEFAULT_WEIGHT)))
            }
        }
        PathInstruction::FaceTowards => {
            if to_first_point.x > 0.0 {
                blackboard.enqueue_action(Action::SetSteering(Steering::new(1.0, -1.0, Steering::DEFAULT_WEIGHT)))
            } else {
                blackboard.enqueue_action(Action::SetSteering(Steering::new(-1.0, 1.0, Steering::DEFAULT_WEIGHT)))
            }
        }
    };

    *blackboard.get_poll_when() = PollWhen::Instant(Instant::now() + Duration::from_millis(16));

    Status::Running
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
