use std::time::Duration;

use ares_bt::{converters::AssertCancelSafe, sequence::Sequence, Behavior, CancelSafe, Status};
use embedded_common::{Actuator, ActuatorCommand};

use crate::{Action, LunabotBlackboard, PollWhen};

const COMPLETION_TOLERANCE: u16 = 10;

pub(super) fn move_actuators() -> impl Behavior<LunabotBlackboard> + CancelSafe {
    Sequence::new((
        AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
            let lift_positive = *blackboard.get_target_lift() > blackboard.get_actual_lift();
            let tilt_positive = *blackboard.get_target_tilt() > blackboard.get_actual_tilt();
            *blackboard.get_lift_travel_positive() = lift_positive;
            *blackboard.get_tilt_travel_positive() = tilt_positive;

            if lift_positive {
                blackboard.enqueue_action(Action::SetActuators(ActuatorCommand::backward(
                    Actuator::Lift,
                )));
            } else {
                blackboard.enqueue_action(Action::SetActuators(ActuatorCommand::forward(
                    Actuator::Lift,
                )));
            }

            if tilt_positive {
                blackboard.enqueue_action(Action::SetActuators(ActuatorCommand::backward(
                    Actuator::Bucket,
                )));
            } else {
                blackboard.enqueue_action(Action::SetActuators(ActuatorCommand::forward(
                    Actuator::Bucket,
                )));
            }

            Status::Success
        }),
        AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
            let lift_done;
            let tilt_done;

            let speed = blackboard
                .get_actual_tilt()
                .abs_diff(*blackboard.get_target_tilt())
                .clamp(100, 1000) as f64
                / 1000.0;

            if *blackboard.get_tilt_travel_positive() {
                tilt_done = blackboard.get_actual_tilt()
                    >= *blackboard.get_target_tilt() - COMPLETION_TOLERANCE;
                if tilt_done {
                    blackboard.enqueue_action(Action::SetActuators(ActuatorCommand::set_speed(
                        0.0,
                        Actuator::Bucket,
                    )));
                } else {
                    blackboard.enqueue_action(Action::SetActuators(ActuatorCommand::set_speed(
                        speed,
                        Actuator::Bucket,
                    )));
                }
            } else {
                tilt_done = blackboard.get_actual_tilt()
                    <= *blackboard.get_target_tilt() + COMPLETION_TOLERANCE;
                if tilt_done {
                    blackboard.enqueue_action(Action::SetActuators(ActuatorCommand::set_speed(
                        0.0,
                        Actuator::Bucket,
                    )));
                } else {
                    blackboard.enqueue_action(Action::SetActuators(ActuatorCommand::set_speed(
                        speed,
                        Actuator::Bucket,
                    )));
                }
            }

            let speed = blackboard
                .get_actual_lift()
                .abs_diff(*blackboard.get_target_lift())
                .clamp(100, 1000) as f64
                / 1000.0;

            if *blackboard.get_lift_travel_positive() {
                lift_done = blackboard.get_actual_lift()
                    >= *blackboard.get_target_lift() - COMPLETION_TOLERANCE;
                if lift_done {
                    blackboard.enqueue_action(Action::SetActuators(ActuatorCommand::set_speed(
                        0.0,
                        Actuator::Lift,
                    )));
                } else {
                    blackboard.enqueue_action(Action::SetActuators(ActuatorCommand::set_speed(
                        speed,
                        Actuator::Lift,
                    )));
                }
            } else {
                lift_done = blackboard.get_actual_lift()
                    <= *blackboard.get_target_lift() + COMPLETION_TOLERANCE;
                if lift_done {
                    blackboard.enqueue_action(Action::SetActuators(ActuatorCommand::set_speed(
                        0.0,
                        Actuator::Lift,
                    )));
                } else {
                    blackboard.enqueue_action(Action::SetActuators(ActuatorCommand::set_speed(
                        speed,
                        Actuator::Lift,
                    )));
                }
            }

            if lift_done && tilt_done {
                Status::Success
            } else {
                *blackboard.get_poll_when() =
                    PollWhen::Instant(blackboard.get_now() + Duration::from_millis(50));
                Status::Running
            }
        }),
    ))
}
