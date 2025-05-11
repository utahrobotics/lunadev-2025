use std::time::{Duration, Instant};

use ares_bt::{branching::TryCatch, converters::{AssertCancelSafe, Invert}, sequence::Sequence, Behavior, CancelSafe, Status};
use common::{cell_to_world_point, world_point_to_cell, PathInstruction, Steering};
use nalgebra::{distance, Matrix2, Point3, Vector2};

use crate::{blackboard::LunabotBlackboard, utils::WaitBehavior, Action, PollWhen};


/// how far the robot should back up when its stuck in one spot for too long
const BACKING_AWAY_DISTANCE: f64 = 0.3;

/// max time the robot spends in one spot before backing up and pathfinding again
const MAX_STUCK_DURATION: Duration = Duration::from_millis(1500);

/// min distance moved until `blackboard.latest_transform` gets updated
const MIN_DIST_UNTIL_TRANSFORM_UPDATE: f64 = 0.01;

/// min angle moved until `blackboard.latest_transform` gets updated
const MIN_ANGLE_UNTIL_TRANSFORM_UPDATE: f64 = 0.1;


const PAUSE_AFTER_MOVING_DURATION: Duration = Duration::from_secs(2);

pub(super) fn follow_path() -> impl Behavior<LunabotBlackboard> + CancelSafe {
    
    do_then_wait(
        AssertCancelSafe(follow_path_inner), 
        PAUSE_AFTER_MOVING_DURATION
    )
}

fn follow_path_inner(blackboard: &mut LunabotBlackboard) -> Status {
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
    let target_cell = blackboard.get_target_cell();
    let mut heading: Vector2<f64> = blackboard.get_robot_heading();
    
    let path = blackboard.get_path_mut();
    
    if path.is_empty() {
        println!("path follower: empty path", );
        blackboard.enqueue_action(Action::SetSteering(Steering::default()));
        return Status::Running;
    }

    let curr_instr = path[0];

    if curr_instr.is_finished(&pos.xz(), &heading.into()) {
        
        // sometimes path may be cut short to scan an unknown cell
        let path_leads_to_goal = path.last().map(|p| p.cell) == target_cell;
        
        path.remove(0);
        let path_complete = path.is_empty();

        if path_complete {
            
            
            *blackboard.get_path_mut() = vec![];
            blackboard.enqueue_action(Action::SetSteering(Steering::default()));
            
            // ensures that time between path follows aren't interpreted as being stuck in one place for a long time
            blackboard.clear_latest_transform();
            
            return match path_leads_to_goal {
                true => {
                    println!("path follower: done! {:?}", blackboard.get_autonomy_state());
                    
                    blackboard.enqueue_action(Action::ClearPointsToAvoid);
                    Status::Success
                }
                false => Status::Failure
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
                blackboard.enqueue_action(Action::AvoidCell(world_point_to_cell(pos)));
                *blackboard.backing_away_from() = Some(pos);
                return Status::Running;
            }
        },
    };
    
    if curr_instr.instruction == PathInstruction::MoveToBackwards {
        heading *= -1.0;
    }
    
    let heading_angle = heading.angle(&Vector2::new(0.0, -1.0));
    let to_first_point = (cell_to_world_point(curr_instr.cell, 0.).xz() - pos.xz()).normalize();

    // direction to first point of path, from robot's pov
    let to_first_point = if heading.x < 0.0 {
        rotate_v2_ccw(to_first_point, heading_angle)
    } else {
        rotate_v2_ccw(to_first_point, -heading_angle)
    };

    match curr_instr.instruction {
        PathInstruction::MoveTo | PathInstruction::MoveToBackwards => {
            if to_first_point.angle(&Vector2::new(0.0, -1.0)).to_degrees() > 20.0 {
                if to_first_point.x > 0.0 {
                    blackboard
                        .enqueue_action(Action::SetSteering(Steering::new(1.0, -1.0, Steering::DEFAULT_WEIGHT)))
                } else {
                    blackboard
                        .enqueue_action(Action::SetSteering(Steering::new(-1.0, 1.0, Steering::DEFAULT_WEIGHT)))
                }
            } else {
                let (mut l, mut r) = scaled_clamp(
                    -to_first_point.y + to_first_point.x * 1.2,
                    -to_first_point.y - to_first_point.x * 1.2,
                    1.0,
                );
                if curr_instr.instruction == PathInstruction::MoveToBackwards {
                    l *= -1.0;
                    r *= -1.0;
                }
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

/// after `behavior` ends, waits for `duration` then returns the same status that `behavior` did
fn do_then_wait(behavior: impl Behavior<LunabotBlackboard> + CancelSafe, duration: Duration) -> impl Behavior<LunabotBlackboard> + CancelSafe
{
    TryCatch::new(
        Sequence::new((
            behavior,
            WaitBehavior::from(duration),
        )),
        Invert(WaitBehavior::from(duration)),
    )
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