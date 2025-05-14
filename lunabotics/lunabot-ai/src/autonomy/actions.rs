use std::time::Duration;

use ares_bt::{
    action::AlwaysSucceed,
    converters::AssertCancelSafe,
    looping::WhileLoop,
    sequence::{ParallelAny, Sequence},
    Behavior, CancelSafe, Status,
};
use common::{Obstacle, Steering};
use embedded_common::{Actuator, ActuatorCommand};

use crate::{blackboard, utils::WaitBehavior, Action, LunabotBlackboard, PollWhen};

use super::{move_actuator::move_actuators, traverse};

/// distance between shovel and center of robot
const SHOVEL_DISTANCE_METERS: f64 = 0.3; // TODO set this to the actual value

/// adds a obstacle to `additional_obstacles` to prevent digging or moving over that spot again
///
/// to be used to avoid holes/mounds
fn add_hole_or_mound_obstacle(blackboard: &mut LunabotBlackboard) {
    let shovel_pos = (blackboard.get_robot_heading() * SHOVEL_DISTANCE_METERS)
        + blackboard.get_robot_pos().xz().coords;

    blackboard.enqueue_action(Action::AvoidObstacle(Obstacle::new_circle(
        (shovel_pos.x, shovel_pos.y),
        0.3,
    )));
}

// pub(super) fn dig() -> impl Behavior<LunabotBlackboard> + CancelSafe {

//     Sequence::new((
//         AssertCancelSafe(
//             |blackboard: &mut LunabotBlackboard| {
//                 // TODO

//                 println!("digging!!");

//                 // add the hole we just dug to `additional_obstacles` to prevent digging or moving over that spot again
//                 add_hole_or_mound_obstacle(blackboard);

//                 Status::Success
//             }
//         ),
//     ))
// }

pub(super) fn dump() -> impl Behavior<LunabotBlackboard> + CancelSafe {
    Sequence::new((
        Sequence::new((
            AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
                *blackboard.get_target_lift() = 1000;
                *blackboard.get_target_tilt() = 2000;
                Status::Success
            }),
            move_actuators(),
            ParallelAny::new((
                WaitBehavior::from(Duration::from_secs_f64(3.0)),
                WhileLoop::new(
                    AlwaysSucceed,
                    AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
                        blackboard.enqueue_action(Action::SetSteering(Steering::new(
                            0.5,
                            0.5,
                            Steering::DEFAULT_WEIGHT,
                        )));
                        blackboard.enqueue_action(Action::SetActuators(ActuatorCommand::forward(
                            Actuator::Lift,
                        )));
                        blackboard.enqueue_action(Action::SetActuators(
                            ActuatorCommand::set_speed(0.8, Actuator::Lift),
                        ));
                        blackboard.enqueue_action(Action::SetActuators(ActuatorCommand::forward(
                            Actuator::Bucket,
                        )));
                        blackboard.enqueue_action(Action::SetActuators(
                            ActuatorCommand::set_speed(0.8, Actuator::Bucket),
                        ));
                        *blackboard.get_poll_when() =
                            PollWhen::Instant(blackboard.get_now() + Duration::from_millis(50));
                        Status::Running
                    }),
                ),
            )),
            AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
                *blackboard.get_target_lift() = 1000;
                Status::Success
            }),
            ParallelAny::new((
                move_actuators(),
                AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
                    blackboard.enqueue_action(Action::SetSteering(Steering::default()));
                    blackboard.enqueue_action(Action::SetActuators(ActuatorCommand::set_speed(
                        0.0,
                        Actuator::Lift,
                    )));
                    blackboard.enqueue_action(Action::SetActuators(ActuatorCommand::set_speed(
                        0.0,
                        Actuator::Bucket,
                    )));
                    Status::Running
                }),
            )),
        )),
        traverse(), // knows to go to dump spot due to autonomy state
        Sequence::new((
            AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
                *blackboard.get_target_tilt() = 2800;
                blackboard.enqueue_action(Action::SetSteering(Steering::default()));
                Status::Success
            }),
            move_actuators(),
            WaitBehavior::from(Duration::from_secs_f64(3.0)),
        )),
        // AssertCancelSafe(
        //     |blackboard: &mut LunabotBlackboard| {
        //         // TODO move arms to dump

        //         println!("dumping!!", );

        //         // add the mound we just dumped to `additional_obstacles` to prevent digging or moving over that spot again
        //         add_hole_or_mound_obstacle(blackboard);

        //         Status::Success
        //     }
        // ),
    ))
}
