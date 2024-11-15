use std::{sync::Arc, time::Instant, vec};

use ares_bt::{
    action::AlwaysSucceed,
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
}

pub fn run_ai(chain: Arc<Chain<f64>>, mut on_action: impl FnMut(Action, &mut Vec<Input>)) {
    let mut blackboard = LunabotBlackboard::new(chain);
    let mut b = WhileLoop::new(
        AlwaysSucceed,
        Sequence::new((
            |blackboard: &mut LunabotBlackboard| {
                blackboard.enqueue_action(Action::SetStage(LunabotStage::SoftStop));
                blackboard.enqueue_action(Action::SetSteering(Steering::default()));
                InfallibleStatus::Success
            },
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
                    blackboard.enqueue_action(Action::WaitForLunabase);
                    FallibleStatus::Running
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
        b.run_eternal(&mut blackboard);
        for action in blackboard.drain_actions() {
            on_action(action, &mut inputs);
        }
        for input in inputs.drain(..) {
            blackboard.digest_input(input);
        }
    }
}

fn follow_path(blackboard: &mut LunabotBlackboard) -> InfallibleStatus {
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
    let to_first_point = (first_point - Vector2::new(robot.translation.x, robot.translation.z)).coords.normalize();

    // direction to first point of path, from robot's pov 
    let to_first_point = 
        if heading.x < 0.0  { rotate_v2_ccw(to_first_point,  heading_angle) }
        else                { rotate_v2_ccw(to_first_point, -heading_angle) };

    return if to_first_point.angle(&Vector2::new(0.0, -1.0)) > 0.1 {
        if to_first_point.x > 0.0    
            { InfallibleStatus::Running(Action::SetSteering(Steering::new_left_right( 1.0, -1.0))) }
        else
            { InfallibleStatus::Running(Action::SetSteering(Steering::new_left_right(-1.0,  1.0))) }
    }
    else 
        { InfallibleStatus::Running(Action::SetSteering(Steering::new_left_right( 1.0, 1.0))) }


    // gradually turn towards next point - avoid frequent stopping to turn for path points in an arc shape
    // let (l, r) = scaled_clamp(-to_first_point.y + to_first_point.x, -to_first_point.y - to_first_point.x, 1.0);
    // InfallibleStatus::Running(Action::SetSteering(Steering::new_left_right(l, r)))
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