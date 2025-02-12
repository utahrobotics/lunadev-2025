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
        if godot::classes::Engine::singleton().is_editor_hint() {
            return Self {
                thalassic_data,
                request_thalassic: Box::new(|| {}),
                base,
            };
        }
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

            let mut obstacle_img = Image::new_gd();
            let mut grad_img = Image::new_gd();
            let mut height_img = Image::new_gd();

            let mut y_count:i32 = 0;
            let mut x_count = 0;
            for i in thalassic_data.heightmap{
                let obstacle_val = thalassic_data.expanded_obstacle_map[(y_count as usize*256)+x_count as usize];
                let grad_val:f32 = thalassic_data.gradmap[(y_count as usize*256)+x_count as usize];
                let height_val:f32 = thalassic_data.heightmap[(y_count as usize*256)+x_count as usize];

                if obstacle_val.occupied(){
                    obstacle_img.set_pixel(x_count, y_count, Color { r: (1f32), g: (1f32), b: (1f32), a: (1f32) });
                }else{
                    obstacle_img.set_pixel(x_count, y_count, Color { r: (0f32), g: (0f32), b: (0f32), a: (0f32) });
                }

                let grad = (grad_val)/(3.14f32/2f32);
                grad_img.set_pixel(x_count, y_count, Color { r: (grad), g: (grad), b: (grad), a: (grad) }); // 0 to pi/2
                let height:f32 = (height_val+1f32)/2f32;
                height_img.set_pixel(x_count, y_count, Color { r: (height), g: (height), b: (height), a: (height) }); // -1 to 1
                x_count+=1;
                if x_count>=256{
                    x_count=0;
                    y_count+=1;
                }
            }

            self.base_mut().emit_signal("send_map_data",
            &[Variant::from(Image::new_gd()),
            Variant::from(point_cloud),
            Variant::from(thalassic_data.heightmap),
            Variant::from(grad_img),
            Variant::from(obstacle_img)
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
