#![feature(result_flattening, deadline_api)]

use std::{
    fs::File,
    net::SocketAddrV4,
    path::Path,
    time::{Duration, Instant},
};

use bonsai_bt::{Behavior::*, Event, Status, UpdateArgs, BT};
use common::{FromLunabase, FromLunabot};
use serde::{Deserialize, Serialize};
use setup::Blackboard;
use spin_sleep::SpinSleeper;
use urobotics::{
    app::{application, Application},
    camera,
    log::{error, warn},
    python, serial,
};

mod run;
mod setup;
mod soft_stop;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum HighLevelActions {
    SoftStop,
    Setup,
    Run,
}

#[derive(Serialize, Deserialize)]
struct LunabotApp {
    #[serde(default = "default_delta")]
    target_delta: f64,
    lunabase_address: SocketAddrV4,
}

fn default_delta() -> f64 {
    1.0 / 60.0
}

impl Application for LunabotApp {
    const APP_NAME: &'static str = "main";

    const DESCRIPTION: &'static str = "The lunabot application";

    fn run(self) {
        if let Err(e) = File::create("from_lunabase.txt")
            .map(|f| FromLunabase::write_code_sheet(f))
            .flatten()
        {
            error!("Failed to write code sheet for FromLunabase: {e}");
        }
        if let Err(e) = File::create("from_lunabot.txt")
            .map(|f| FromLunabot::write_code_sheet(f))
            .flatten()
        {
            error!("Failed to write code sheet for FromLunabot: {e}");
        }
        // Whether or not Run succeeds or fails is ignored as it should
        // never terminate anyway. If it does terminate, that indicates
        // an event that requires user intervention. The event could
        // also be an express instruction to stop from the user.
        let run = AlwaysSucceed(Box::new(Action(HighLevelActions::Run)));
        let top_level_behavior = While(
            Box::new(WaitForever),
            vec![If(
                Box::new(Action(HighLevelActions::SoftStop)),
                Box::new(run.clone()),
                Box::new(Sequence(vec![Action(HighLevelActions::Setup), run])),
            )],
        );

        let mut bt: BT<HighLevelActions, Option<Blackboard>> = BT::new(top_level_behavior, None);
        if let Err(e) = std::fs::write("bt.txt", bt.get_graphviz()) {
            warn!("Failed to generate graphviz of behavior tree: {e}");
        }
        let sleeper = SpinSleeper::default();
        let mut start_time = Instant::now();
        let target_delta = Duration::from_secs_f64(self.target_delta);
        let mut elapsed = target_delta;
        let mut last_action = HighLevelActions::SoftStop;

        loop {
            let e: Event = UpdateArgs {
                dt: elapsed.as_secs_f64(),
            }
            .into();
            let (status, _) = bt.tick(&e, &mut |args, bb| {
                let first_time = last_action != *args.action;
                let result = match args.action {
                    HighLevelActions::SoftStop => {
                        soft_stop::soft_stop(bb.get_db(), args.dt, first_time, &self)
                    }
                    HighLevelActions::Setup => {
                        setup::setup(bb.get_db(), args.dt, first_time, &self)
                    }
                    HighLevelActions::Run => run::run(bb.get_db(), args.dt, first_time, &self),
                };
                last_action = *args.action;
                result
            });
            if status != Status::Running {
                error!("Faced unexpected status while ticking: {status:?}");
                bt.reset_bt();
            }
            elapsed = start_time.elapsed();
            let mut remaining_delta = target_delta.saturating_sub(elapsed);

            if let Some(bb) = bt.get_blackboard().get_db() {
                if let Some(special_instant) = bb.peek_special_instant() {
                    let duration_since = special_instant.duration_since(start_time);
                    if duration_since < remaining_delta {
                        bb.pop_special_instant();
                        remaining_delta = duration_since;
                    }
                }
            }

            sleeper.sleep(remaining_delta);
            elapsed = start_time.elapsed();
            start_time += elapsed;
        }
    }
}

impl LunabotApp {
    pub fn get_target_delta(&self) -> Duration {
        Duration::from_secs_f64(self.target_delta)
    }
}

fn main() {
    let mut app = application!();
    if Path::new("urobotics-venv").exists() {
        app.cabinet_builder.create_symlink_for("urobotics-venv");
    }
    app.add_app::<serial::SerialConnection>()
        .add_app::<python::PythonVenvBuilder>()
        .add_app::<camera::CameraConnection>()
        .add_app::<LunabotApp>()
        .run();
}
