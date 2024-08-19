#![feature(result_flattening, deadline_api)]

use std::{
    fs::File,
    net::SocketAddrV4,
    path::Path,
    process::Stdio,
    time::{Duration, Instant},
};

use bonsai_bt::{Behavior::*, Event, Status, UpdateArgs, BT};
use common::{lunasim::FromLunasim, FromLunabase, FromLunabot};
use serde::{Deserialize, Serialize};
use setup::Blackboard;
use spin_sleep::SpinSleeper;
use urobotics::{
    app::{adhoc_app, application, Application},
    camera, define_callbacks, fn_alias, get_tokio_handle,
    log::{error, warn},
    python, serial,
    tokio::{
        self,
        io::AsyncReadExt,
        process::{ChildStdin, Command},
    },
    video::info::list_media_input,
    BlockOn,
};
use video::VideoTestApp;

mod run;
mod setup;
mod soft_stop;
mod video;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum HighLevelActions {
    SoftStop,
    Setup,
    Run,
}

fn_alias! {
    pub type FromLunasimRef = CallbacksRef(FromLunasim) + Send
}
define_callbacks!(FromLunasimCallbacks => CloneFn(msg: FromLunasim) + Send);

enum RunMode {
    Production,
    Simulation {
        #[allow(dead_code)]
        child_stdin: tokio::sync::Mutex<ChildStdin>,
        #[allow(dead_code)]
        from_lunasim: FromLunasimRef,
    },
}

impl Default for RunMode {
    fn default() -> Self {
        RunMode::Production
    }
}

#[derive(Serialize, Deserialize)]
struct LunabotApp {
    #[serde(default = "default_delta")]
    target_delta: f64,
    lunabase_address: SocketAddrV4,
    #[serde(skip)]
    run_mode: RunMode,
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

        let interrupted = Box::leak(Box::new(std::sync::atomic::AtomicBool::default()));
        get_tokio_handle().spawn(async {
            match tokio::signal::ctrl_c().await {
                Ok(()) => {
                    warn!("Ctrl-C Received");
                    interrupted.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                Err(e) => error!("Failed to await ctrl_c: {e}"),
            }
        });

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

            if interrupted.load(std::sync::atomic::Ordering::Relaxed) {
                warn!("Exiting");
                break;
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

#[derive(Serialize, Deserialize)]
struct LunasimbotApp {
    #[serde(flatten)]
    app: LunabotApp,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[serde(default)]
    simulation_command: Vec<String>,
}

impl Application for LunasimbotApp {
    const APP_NAME: &'static str = "sim";

    const DESCRIPTION: &'static str = "The lunabot application in a simulated environment";

    fn run(mut self) {
        let mut cmd = if self.simulation_command.is_empty() {
            let mut cmd = Command::new("godot");
            cmd.args(["--path", "godot/lunasim"]);
            cmd
        } else {
            let mut cmd = Command::new(self.simulation_command.remove(0));
            cmd.args(self.simulation_command);
            cmd
        };

        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let child;
        let (child_stdin, from_lunasim) = match cmd.spawn() {
            Ok(tmp) => {
                child = tmp;
                let stdin = child.stdin.unwrap();
                let mut stdout = child.stdout.unwrap();
                let mut stderr = child.stderr.unwrap();
                macro_rules! handle_err {
                    ($msg: literal, $err: ident) => {{
                        match $err.kind() {
                            std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::Other => {}
                            _ => {
                                error!(target: "lunasim", "Faced the following error while reading {}: {:?}", $msg, $err);
                            }
                        }
                        break;
                    }}
                }

                let handle = get_tokio_handle();
                handle.spawn(async move {
                    let mut bytes = Vec::with_capacity(1024);
                    let mut buf = [0u8; 1024];

                    loop {
                        if bytes.len() == bytes.capacity() {
                            bytes.reserve(bytes.len());
                        }
                        match stderr.read(&mut buf).await {
                            Ok(0) => {}
                            Ok(n) => {
                                bytes.extend_from_slice(&buf[0..n]);
                                if let Ok(string) = std::str::from_utf8(&bytes) {
                                    if let Some(i) = string.find('\n') {
                                        warn!(target: "lunasim", "{}", &string[0..i]);
                                        bytes.drain(0..=i);
                                    }
                                }
                            }
                            Err(e) => handle_err!("stderr", e),
                        }
                    }
                });

                let mut callbacks = FromLunasimCallbacks::default();
                let callbacks_ref = callbacks.get_ref();

                handle.spawn(async move {
                    let mut size_buf = [0u8; 4];
                    let mut bytes = Vec::with_capacity(1024);
                    loop {
                        let size = match stdout.read_exact(&mut size_buf).await {
                            Ok(_) => u32::from_ne_bytes(size_buf),
                            Err(e) => handle_err!("stdout", e)
                        };
                        bytes.resize(size as usize, 0u8);
                        match stdout.read_exact(&mut bytes).await {
                            Ok(_) => {},
                            Err(e) => handle_err!("stdout", e)
                        }
                        match FromLunasim::try_from(bytes.as_slice()) {
                            Ok(msg) => callbacks.call(msg),
                            Err(e) => {
                                error!(target: "lunasim", "Failed to deserialize from lunasim: {e}");
                                continue;
                            }
                        }
                    }
                });

                (stdin.into(), callbacks_ref)
            }
            Err(e) => {
                error!("Failed to run simulation command: {e}");
                return;
            }
        };
        self.app.run_mode = RunMode::Simulation {
            child_stdin,
            from_lunasim,
        };
        self.app.run()
    }
}

fn info_app() {
    match list_media_input().block_on() {
        Ok(list) => {
            if list.is_empty() {
                println!("No media input found");
            } else {
                println!("Media inputs:");
                for info in list {
                    println!("\t{} ({})", info.name, info.media_type);
                }
            }
        }
        Err(e) => eprintln!("Failed to list media input: {e}"),
    }
    println!();
}

adhoc_app!(InfoApp, "info", "Print diagnostics", info_app);

fn main() {
    let mut app = application!();
    if Path::new("urobotics-venv").exists() {
        app.cabinet_builder.create_symlink_for("urobotics-venv");
    }
    app.cabinet_builder.create_symlink_for("godot");
    app.cabinet_builder.create_symlink_for("target");

    app.add_app::<serial::SerialConnection>()
        .add_app::<python::PythonVenvBuilder>()
        .add_app::<camera::CameraConnection>()
        .add_app::<LunabotApp>()
        .add_app::<VideoTestApp>()
        .add_app::<InfoApp>()
        .add_app::<LunasimbotApp>()
        .run();
}
