#![feature(let_chains)]

use common::thalassic::{lunabase_task, ThalassicData};
use godot::{classes::{Image, ImageTexture}, prelude::*};
use parking_lot::Mutex;

struct LunavizLib;

#[gdextension]
unsafe impl ExtensionLibrary for LunavizLib {}

#[derive(GodotClass)]
#[class(base=Node)]
struct Lunaviz {
    thalassic_data: &'static Mutex<(ThalassicData, Vec<Vector3>, bool)>,
    request_thalassic: Box<dyn Fn() + Send + Sync>,
    base: Base<Node>,
}

#[godot_api]
impl INode for Lunaviz {
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

            self.base_mut().emit_signal("send_map_data",
            &[Variant::from(Image::new_gd()),
            Variant::from(point_cloud),
            Variant::from(thalassic_data.heightmap),
            Variant::from(thalassic_data.gradmap),
            Variant::from(thalassic_data.expanded_obstacle_map)
            ]);
        }
    }
}

#[godot_api]
impl Lunaviz {
    #[signal]
    fn send_map_data(&self);

    #[func]
    pub fn request_thalassic_data(&self) {
        (self.request_thalassic)();
    }
}
