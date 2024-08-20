#![feature(backtrace_frames)]
use std::sync::{Arc, Once};

use cakap::{CakapSender, CakapSocket};
use common::{FromLunabase, FromLunabot, Steering};
use crossbeam::queue::SegQueue;
use godot::{classes::Engine, prelude::*};
use log::Log;
use tasker::BlockOn;
struct LunabaseLib;

#[gdextension]
unsafe impl ExtensionLibrary for LunabaseLib {}

static PANIC_INIT: Once = Once::new();

pub fn init_panic_hook() {
    PANIC_INIT.call_once(|| {
        // To enable backtrace, you will need the `backtrace` crate to be included in your cargo.toml, or
        // a version of Rust where backtrace is included in the standard library (e.g. Rust nightly as of the date of publishing)
        // use backtrace::Backtrace;
        use std::backtrace::Backtrace;
        let old_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic_info| {
            let loc_string;
            if let Some(location) = panic_info.location() {
                loc_string = format!("file '{}' at line {}", location.file(), location.line());
            } else {
                loc_string = "unknown location".to_owned()
            }

            let error_message;
            if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
                error_message = format!("[RUST] {}: panic occurred: {:?}", loc_string, s);
            } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
                error_message = format!("[RUST] {}: panic occurred: {:?}", loc_string, s);
            } else {
                error_message = format!("[RUST] {}: unknown panic occurred", loc_string);
            }
            godot_error!("{}", error_message);
            // Uncomment the following line if backtrace crate is included as a dependency
            for frame in Backtrace::force_capture().frames() {
                godot_error!("{frame:?}");
            }
            (*(old_hook.as_ref()))(panic_info);

            // unsafe {
            // if let Some(gd_panic_hook) = godot::api::utils::autoload::<gdnative::api::Node>("rust_panic_hook") {
            //     gd_panic_hook.call("rust_panic_hook", &[GodotString::from_str(error_message).to_variant()]);
            // }
            // }
        }));
    });
}


struct LunabotShared {
    from_lunabot: SegQueue<FromLunabot>
}

const STEERING_DELAY: f64 = 1.0 / 15.0;

struct LunabotConnInner {
    lunabase_conn: CakapSender,
    shared: Arc<LunabotShared>,
    steering: Steering,
    steering_timer: f64,
}

#[derive(GodotClass)]
#[class(base=Node)]
struct LunabotConn {
    inner: Option<LunabotConnInner>,
    base: Base<Node>,
}

struct GodotLog;

impl Log for GodotLog {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        match record.level() {
            log::Level::Error => godot_error!("{}", record.args()),
            log::Level::Warn => godot_warn!("{}", record.args()),
            _ => godot_print!("{}", record.args()),
        }
    }

    fn flush(&self) {}
}

#[godot_api]
impl INode for LunabotConn {
    fn init(base: Base<Node>) -> Self {
        if Engine::singleton().is_editor_hint() {
            return Self {
                inner: None,
                base
            }
        }
        init_panic_hook();
        log::set_boxed_logger(Box::new(GodotLog)).unwrap();

        let shared = Arc::new(LunabotShared {
            from_lunabot: SegQueue::default()
        });

        let socket = CakapSocket::bind(10600).block_on().expect("Failed to bind to 10600");
        let lunabase_conn = socket.get_stream();
        
        let shared2 = shared.clone();
        socket
            .get_bytes_callback_ref()
            .add_dyn_fn(Box::new(move |bytes| {
                let msg: FromLunabot = match TryFrom::try_from(bytes) {
                    Ok(x) => x,
                    Err(e) => {
                        godot_error!("Failed to parse message from lunabase: {e}");
                        return;
                    }
                };
                shared2.from_lunabot.push(msg);
            }));
        socket.spawn_looping();
Self {
        inner: Some(LunabotConnInner {
            lunabase_conn,
            shared,
            steering: Steering::default(),
            steering_timer: STEERING_DELAY,
        }),
        base,}
    }

    fn process(&mut self, delta: f64) {
        if let Some(inner) = &mut self.inner {
            inner.steering_timer -= delta;
            if inner.steering_timer <= 0.0 {
                inner.steering_timer = STEERING_DELAY;
                FromLunabase::Steering(inner.steering).encode(|bytes| {
                    inner.lunabase_conn.send_unreliable(bytes).block_on();
                });
            }
    
            let mut received = false;

            while let Some(msg) = inner.shared.from_lunabot.pop() {
                received = true;
                match msg {
                    FromLunabot::Ping => {}
                }
            }

            if received {
                self.base_mut()
                    .emit_signal("something_received".into(), &[]);
            }
        }
    }
}

impl LunabotConn {
    fn send_reliable(&self, msg: &FromLunabase) {
        if let Some(inner) = &self.inner {
            msg.encode(|bytes| {
                let mut vec = inner.lunabase_conn.get_recycled_byte_vecs().get_vec();
                vec.extend_from_slice(bytes);
                inner.lunabase_conn.send_reliable(vec);
            });
        }
    }
}

#[godot_api]
impl LunabotConn {
    #[signal]
    fn something_received(&self);

    #[func]
    fn set_steering(&mut self, drive: f64, steering: f64) {
        if let Some(inner) = &mut self.inner {
            inner.steering = Steering::new(drive, steering);
    }
    }

    #[func]
    fn continue_mission(&self) {
        self.send_reliable(&FromLunabase::ContinueMission);
    }

    #[func]
    fn trigger_setup(&self) {
        self.send_reliable(&FromLunabase::TriggerSetup);
    }

    #[func]
    fn traverse_obstacles(&self) {
        self.send_reliable(&FromLunabase::TraverseObstacles);
    }

    #[func]
    fn soft_stop(&self) {
        self.send_reliable(&FromLunabase::SoftStop);
    }

    #[func]
    fn send_ping(&self) {
        self.send_reliable(&FromLunabase::Pong);
    }
}
