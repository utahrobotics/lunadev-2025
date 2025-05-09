use ares_bt::{converters::AssertCancelSafe, looping::WhileLoop, sequence::{ParallelAny, Sequence}, Behavior, CancelSafe, Status};
use embedded_common::{Actuator, ActuatorCommand};

use crate::{Action, LunabotBlackboard};


const COMPLETION_TOLERANCE: u16 = 10;


pub(super) fn move_actuators() -> impl Behavior<LunabotBlackboard> + CancelSafe {
    Sequence::new((
        AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
            let lift_positive = *blackboard.get_target_lift() > blackboard.get_actual_lift();
            let tilt_positive = *blackboard.get_target_tilt() > blackboard.get_actual_tilt();
            *blackboard.get_lift_travel_positive() = lift_positive;
            *blackboard.get_tilt_travel_positive() = tilt_positive;

            if lift_positive {
                blackboard.enqueue_action(Action::SetActuators(ActuatorCommand::forward(Actuator::Lift)));
            } else {
                blackboard.enqueue_action(Action::SetActuators(ActuatorCommand::backward(Actuator::Lift)));
            }

            if tilt_positive {
                blackboard.enqueue_action(Action::SetActuators(ActuatorCommand::forward(Actuator::Bucket)));
            } else {
                blackboard.enqueue_action(Action::SetActuators(ActuatorCommand::backward(Actuator::Bucket)));
            }

            Status::Success
        }),
        ParallelAny::new((
            WhileLoop::new(
                AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
                    if *blackboard.get_lift_travel_positive() {
                        if blackboard.get_actual_lift() >= *blackboard.get_target_lift() - COMPLETION_TOLERANCE {
                            Status::Success
                        } else {
                            Status::Failure
                        }
                    } else {
                        if blackboard.get_actual_lift() <= *blackboard.get_target_lift() + COMPLETION_TOLERANCE {
                            Status::Success
                        } else {
                            Status::Failure
                        }
                    }
                }),
                AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
                    let speed = blackboard.get_actual_lift().abs_diff(*blackboard.get_target_lift()).min(1000).max(100) as f64 / 1000.0;
                    blackboard.enqueue_action(Action::SetActuators(ActuatorCommand::set_speed(speed, Actuator::Lift)));
                    Status::Success
                })
            ),
            WhileLoop::new(
                AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
                    if *blackboard.get_tilt_travel_positive() {
                        if blackboard.get_actual_tilt() >= *blackboard.get_target_tilt() - COMPLETION_TOLERANCE {
                            Status::Success
                        } else {
                            Status::Failure
                        }
                    } else {
                        if blackboard.get_actual_tilt() <= *blackboard.get_target_tilt() + COMPLETION_TOLERANCE {
                            Status::Success
                        } else {
                            Status::Failure
                        }
                    }
                }),
                AssertCancelSafe(|blackboard: &mut LunabotBlackboard| {
                    let speed = blackboard.get_actual_tilt().abs_diff(*blackboard.get_target_tilt()).min(1000).max(100) as f64 / 1000.0;
                    blackboard.enqueue_action(Action::SetActuators(ActuatorCommand::set_speed(speed, Actuator::Bucket)));
                    Status::Success
                })
            ),
        )),
    ))
}