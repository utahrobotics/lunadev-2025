#![feature(let_chains)]

use common::thalassic::{lunabase_task, ThalassicData};
use godot::prelude::*;
use parking_lot::Mutex;

struct LunavizLib;

#[gdextension]
unsafe impl ExtensionLibrary for LunavizLib {}

#[derive(GodotClass)]
#[class(base=Node)]
struct Lunasim {
    thalassic_data: &'static Mutex<(ThalassicData, bool)>,
    request_thalassic: Box<dyn Fn()>,
    base: Base<Node>,
}

#[godot_api]
impl INode for Lunasim {
    fn init(base: Base<Node>) -> Self {
        let thalassic_data: &_ = Box::leak(Box::new(Mutex::new((ThalassicData::default(), false))));
        let request_thalassic = lunabase_task(|data| *thalassic_data.lock() = (*data, true));

        Self {
            thalassic_data,
            request_thalassic: Box::new(request_thalassic),
            base,
        }
    }

    fn process(&mut self, _delta: f64) {
        if let Some(guard) = self.thalassic_data.try_lock()
            && guard.1
        {
            let thalassic_data = &guard.0;
            // TODO: Emit signals
        }
    }
}

#[godot_api]
impl Lunasim {
    // TODO: Create signals
}
