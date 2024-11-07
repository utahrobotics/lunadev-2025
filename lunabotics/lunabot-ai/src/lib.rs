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
use nalgebra::{Matrix2, Point3, Vector2, Vector3};
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
            // |blackboard: &mut LunabotBlackboard| follow_path(blackboard),
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

    // let first_point = blackboard.get_path().unwrap()[0].xz();
    let first_point = nalgebra::OPoint::<f64, nalgebra::Const<2>>::new(-5.0, 0.0);

    let robot = blackboard.get_robot_isometry();

    println!("{}", robot);
                                   
    let heading = robot.rotation.transform_vector(&Vector3::new(0.0, 0.0, -1.0)).xz();

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