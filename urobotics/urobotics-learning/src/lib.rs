// use std::cell::Cell;

// use urobotics::log::error;

pub mod multiples_of_two;
pub mod solutions;

// thread_local! {
//     static EXTENSION: Cell<Option<Box<dyn FnOnce()>>> = Cell::new(None);
// }

// pub fn install_extension(f: impl FnOnce() + 'static) {
//     EXTENSION.with(|ext| {
//         ext.set(Some(Box::new(f)));
//     });
// }

// fn run_extension() {
//     if let Some(f) = EXTENSION.take() {
//         f();
//     } else {
//         error!("No extension installed. Install one using `install_extension`.");
//     }
// }