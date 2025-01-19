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
    thalassic_data: &'static Mutex<(ThalassicData, Vec<Vector3>, bool)>,
    request_thalassic: Box<dyn Fn() + Send + Sync>,
    base: Base<Node>,
}

#[godot_api]
impl INode for Lunasim {
    fn init(base: Base<Node>) -> Self {
        let thalassic_data: &_ = Box::leak(Box::new(Mutex::new((ThalassicData::default(), vec![], false))));
        let request_thalassic = lunabase_task(|data, points| {
            let mut guard = thalassic_data.lock();
            guard.0 = *data;
            guard.1.clear();
            guard.1.extend_from_slice(points);
            guard.2 = true;
        });

        Self {
            thalassic_data,
            request_thalassic: Box::new(request_thalassic),
            base,
        }
    }

    fn process(&mut self, _delta: f64) {
        if let Some(guard) = self.thalassic_data.try_lock()
            && guard.2
        {
            let thalassic_data = &guard.0;
            let point_cloud = guard.1.as_slice();
            // TODO: Emit signals
        }
    }
}

#[godot_api]
impl Lunasim {
    // TODO: Create signals

    #[func]
    pub fn request_thalassic_data(&self) {
        (self.request_thalassic)();
    }
}
