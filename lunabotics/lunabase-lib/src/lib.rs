#![feature(ip_bits)]

use godot::prelude::*;

mod telemetry;
struct LunabaseLib;

#[gdextension]
unsafe impl ExtensionLibrary for LunabaseLib {}

// static PANIC_INIT: Once = Once::new();

// pub fn init_panic_hook() {
//     PANIC_INIT.call_once(|| {
//         // To enable backtrace, you will need the `backtrace` crate to be included in your cargo.toml, or
//         // a version of Rust where backtrace is included in the standard library (e.g. Rust nightly as of the date of publishing)
//         // use backtrace::Backtrace;
//         use std::backtrace::Backtrace;
//         let old_hook = std::panic::take_hook();
//         std::panic::set_hook(Box::new(move |panic_info| {
//             let loc_string;
//             if let Some(location) = panic_info.location() {
//                 loc_string = format!("file '{}' at line {}", location.file(), location.line());
//             } else {
//                 loc_string = "unknown location".to_owned()
//             }

//             let error_message;
//             if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
//                 error_message = format!("[RUST] {}: panic occurred: {:?}", loc_string, s);
//             } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
//                 error_message = format!("[RUST] {}: panic occurred: {:?}", loc_string, s);
//             } else {
//                 error_message = format!("[RUST] {}: unknown panic occurred", loc_string);
//             }
//             godot_error!("{}", error_message);
//             // Uncomment the following line if backtrace crate is included as a dependency
//             for frame in Backtrace::force_capture().frames() {
//                 godot_error!("{frame:?}");
//             }
//             (*(old_hook.as_ref()))(panic_info);

//             // unsafe {
//             // if let Some(gd_panic_hook) = godot::api::utils::autoload::<gdnative::api::Node>("rust_panic_hook") {
//             //     gd_panic_hook.call("rust_panic_hook", &[GodotString::from_str(error_message).to_variant()]);
//             // }
//             // }
//         }));
//     });
// }
