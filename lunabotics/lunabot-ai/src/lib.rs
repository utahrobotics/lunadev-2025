use std::{sync::Arc, time::Instant, vec};

use ares_bt::{
    action::{AlwaysSucceed, RunOnce},
    branching::TryCatch,
    converters::{CatchPanic, Invert},
    looping::WhileLoop,
    sequence::Sequence,
    EternalBehavior, FallibleStatus, InfallibleStatus,
};
use autonomy::autonomy;
use blackboard::LunabotBlackboard;
use common::{FromLunabase, LunabotStage, Steering};
use k::Chain;
use log::warn;
use nalgebra::{distance, Const, Matrix2, OPoint, Point2, Point3, Vector2, Vector3};
use teleop::teleop;

mod autonomy;
mod blackboard;
mod teleop;

pub use blackboard::Input;

#[derive(Debug, Clone)]
pub enum Action {
    /// Wait indefinitely for a message from lunabase.
    WaitForLunabase,
    SetSteering(Steering),
    SetStage(LunabotStage),
    CalculatePath {
        from: Point3<f64>,
        to: Point3<f64>,
        into: Vec<Point3<f64>>,
    },
    /// Wait until the given instant for any input, otherwise poll the ai again.
    WaitUntil(Instant),
    PollAgain,
}

pub fn run_ai(chain: Arc<Chain<f64>>, mut on_action: impl FnMut(Action, &mut Vec<Input>)) {
    let mut blackboard = LunabotBlackboard::new(chain);
    let mut b = WhileLoop::new(
        AlwaysSucceed,
        Sequence::new((
            RunOnce::from(|| Action::SetStage(LunabotStage::SoftStop)),
            RunOnce::from(|| Action::SetSteering(Steering::default())),
            Invert(WhileLoop::new(
                AlwaysSucceed,
                |blackboard: &mut LunabotBlackboard| {
                    while let Some(msg) = blackboard.pop_from_lunabase() {
                        match msg {
                            FromLunabase::ContinueMission => {
                                warn!("Continuing mission");
                                *blackboard.lunabase_disconnected() = false;
                                return FallibleStatus::Failure;
                            }
                            _ => {}
                        }
                    }
                    FallibleStatus::Running(Action::WaitForLunabase)
                },
            )),
            Sequence::new((
                follow_path,
                RunOnce::from(|| Action::SetSteering(Steering::new(0.0, 0.0)))
            )),
            TryCatch::new(
                WhileLoop::new(
                    AlwaysSucceed,
                    Sequence::new((CatchPanic(teleop()), CatchPanic(autonomy()))),
                ),
                AlwaysSucceed,
            ),
        )),
    );

    let mut inputs = vec![];
    loop {
        on_action(b.run_eternal(&mut blackboard).unwrap(), &mut inputs);
        for input in inputs.drain(..) {
            blackboard.digest_input(input);
        }
    }
}

fn follow_path(blackboard: &mut LunabotBlackboard) -> InfallibleStatus<Action> {
    // if let None = blackboard.get_path() {
    //     return InfallibleStatus::Success
    // }
    // let path = blackboard.get_path().unwrap();


    // ALL DEMOS ASSUME ROBOT STARTS AT (-1.0, -1.0), and that the rocks are out of the way
    // path must not intersect with itself

    //DEMO: MOVING IN STRAIGHT LINES 
    let path = &[ 
        OPoint::<f64, Const<3>>::new(-1.0, 0.0, -1.0),
        OPoint::<f64, Const<3>>::new(-3.0, 0.0, -1.0),
        OPoint::<f64, Const<3>>::new(-3.0, 0.0, -3.0),
        OPoint::<f64, Const<3>>::new(-1.0, 0.0, -3.0),
        OPoint::<f64, Const<3>>::new(-1.0, 0.0, -5.0),
        OPoint::<f64, Const<3>>::new(-3.0, 0.0, -5.0),
    ];

    // DEMO: MOVING CONTINUOUSLY IN AN ARC 
    let path = &[
        OPoint::<f64, Const<3>>::new(-1.0, 0.0, -1.0),
        OPoint::<f64, Const<3>>::new(-1.2, 0.0, -1.6),
        OPoint::<f64, Const<3>>::new(-1.4, 0.0, -2.0),
        OPoint::<f64, Const<3>>::new(-1.6, 0.0, -2.2),
        OPoint::<f64, Const<3>>::new(-1.8, 0.0, -2.4),
        OPoint::<f64, Const<3>>::new(-2.2, 0.0, -2.6),
        OPoint::<f64, Const<3>>::new(-2.8, 0.0, -2.8),
    ];

    // DEMO: MIXED PATH WITH CLUSTERED POINTS IN ARC SHAPE AND LONG STRAIGHT SEGMENTS  
    let path = &[
        OPoint::<f64, Const<3>>::new(-1.0, 0.0, -1.0),
        OPoint::<f64, Const<3>>::new(-1.2, 0.0, -1.6),
        OPoint::<f64, Const<3>>::new(-1.4, 0.0, -2.0),
        OPoint::<f64, Const<3>>::new(-1.6, 0.0, -2.2),
        OPoint::<f64, Const<3>>::new(-1.8, 0.0, -2.4),
        OPoint::<f64, Const<3>>::new(-2.2, 0.0, -2.6),
        OPoint::<f64, Const<3>>::new(-2.8, 0.0, -2.8),

        OPoint::<f64, Const<3>>::new(-3.0, 0.0, -3.0),
        OPoint::<f64, Const<3>>::new(-3.0, 0.0, -5.0),
        OPoint::<f64, Const<3>>::new(-1.0, 0.0, -7.0),
        OPoint::<f64, Const<3>>::new(-1.0, 0.0, -5.0),
    ];
        
    let robot = blackboard.get_robot_isometry();
    let pos = Point2::new(robot.translation.x, robot.translation.z);
    let heading = robot.rotation.transform_vector(&Vector3::new(0.0, 0.0, -1.0)).xz();

    match find_target_point(pos, path) {
        Some(i) => {

            let heading_angle = heading.angle(&Vector2::new(0.0, -1.0));
            let to_first_point = (path[i].xz() - pos).normalize();
        
            // direction to first point of path, from robot's pov 
            let to_first_point = 
                if heading.x < 0.0  { rotate_v2_ccw(to_first_point,  heading_angle) }
                else                { rotate_v2_ccw(to_first_point, -heading_angle) };

            // when approaching an arc turn gradually
            if distance(&pos, &path[i].xz()) < ARC_THRESHOLD && within_arc(path, i) {
                let (l, r) = scaled_clamp(-to_first_point.y + to_first_point.x, -to_first_point.y - to_first_point.x, 0.8);
                return InfallibleStatus::Running(Action::SetSteering(Steering::new_left_right(l, r)));
            }
        
            return if to_first_point.angle(&Vector2::new(0.0, -1.0)) > 0.1 {
                if to_first_point.x > 0.0    
                    { InfallibleStatus::Running(Action::SetSteering(Steering::new_left_right( 1.0, -1.0))) }
                else
                    { InfallibleStatus::Running(Action::SetSteering(Steering::new_left_right(-1.0,  1.0))) }
            }
            else 
                { InfallibleStatus::Running(Action::SetSteering(Steering::new_left_right( 1.0, 1.0))) }
        
        } 
        None => InfallibleStatus::Success
    }
}

/// min distance for 2 path points to be considered part of an arc
const ARC_THRESHOLD: f64 = 0.7;

/// is this point considered part of an arc?
fn within_arc(path: &[OPoint<f64, Const<3>>], i: usize) -> bool {
    return 
        if      path.len() == 1     { false }
        else if i == path.len()-1   { distance(&path[i].xz(), &path[i-1].xz()) < ARC_THRESHOLD } 
        else                        { distance(&path[i].xz(), &path[i+1].xz()) < ARC_THRESHOLD }
}

/// min distance for robot to be considered at a point
const AT_POINT_THRESHOLD: f64 = 0.1;

/// find index of the next point the robot should move towards, based on which path segment the robot is closest to
/// 
/// returns `None` if robot is at the last point
fn find_target_point(pos: Point2<f64>, path: &[OPoint<f64, Const<3>>]) -> Option<usize> {

    for i in 0..path.len() {
        if distance(&pos, &path[i].xz()) < AT_POINT_THRESHOLD {
            return 
                if i == path.len()-1 { None }
                else { Some(i+1) }
        }
    }
    
    let mut min_dist = distance(&pos, &path[0].xz());
    let mut target_point = 0;

    for i in 1..path.len() {

        let dist = dist_to_segment(pos, path[i-1].xz(), path[i].xz());

        if dist < min_dist {
            min_dist = dist;
            target_point = i;
        }
    }

    Some(target_point)
}

fn dist_to_segment(point: Point2<f64>, a: Point2<f64>, b: Point2<f64>) -> f64 {
    let mut line_from_origin = b - a; // move line segment to origin
    let mut point = point - a; // move point the same amount

    let angle = -line_from_origin.y.signum() * line_from_origin.angle( &Vector2::new(1.0, 0.0) );

    // rotate both until segment lines up with the x axis
    line_from_origin = rotate_v2_ccw(line_from_origin, angle);
    point = rotate_v2_ccw(point, angle);

    return 
        if      point.x <= 0.0 { point.magnitude() }
        else if point.x >= line_from_origin.x { (point - Vector2::new(line_from_origin.x, 0.0)).magnitude() }
        else    { point.y.abs() }
}

fn rotate_v2_ccw(vector2: Vector2<f64>, theta: f64) -> Vector2<f64> {
    let rot = Matrix2::new(
        f64::cos(theta), -f64::sin(theta),
        f64::sin(theta),  f64::cos(theta),
    );
    return rot * vector2;
}

/// clamps `a` and `b` so that `a.abs().max(b.abs) <= bound.abs()`,
/// while maintaining the ratio between `a` and `b`
fn scaled_clamp(a: f64, b: f64, bound: f64) -> (f64, f64) {
    let bound = bound.abs();

    if a.abs().max(b.abs()) <= bound {
        (a, b)
    }
    else if a.abs() > b.abs() {
        (bound * a.signum(), (bound * b / a).abs() * b.signum())
    }
    else {
        ((bound * a / b).abs() * a.signum(), bound * b.signum())
    }
}