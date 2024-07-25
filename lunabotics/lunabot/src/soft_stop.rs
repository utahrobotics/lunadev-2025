use bonsai_bt::Status;
use common::FromLunabase;
use urobotics::log::info;

use crate::{setup::Blackboard, LunabotApp};

pub(super) fn soft_stop(bb: &mut Option<Blackboard>, dt: f64, first_time: bool, lunabot_app: &LunabotApp) -> (Status, f64) {
    if first_time {
        info!("Entered SoftStop")
    }
    if let Some(bb) = bb {
        if first_time {
            info!("Blackboard present, awaiting lunabase command");
        }
        // We have a connection to lunabase, so wait for commands
        // Operator may request Failure to trigger setup again
        // or Success to trigger run

        if let Ok(msg) = bb.try_get_msg_from_lunabase(lunabot_app.get_target_delta()) {
            match msg {
                FromLunabase::Ping => info!("Pinged"),
                FromLunabase::ContinueMission => {
                    info!("Continuing mission");
                    return (Status::Success, 0.0);
                }
                FromLunabase::TriggerSetup => {
                    return (Status::Failure, 0.0);
                }
            }
        }

        (Status::Running, 0.0)
    } else {
        if first_time {
            info!("No blackboard, so must trigger setup");
        }
        // No connection, so must trigger setup to form connection with lunabase
        (Status::Failure, dt)
    }
}
