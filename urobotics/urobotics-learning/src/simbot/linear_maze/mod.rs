use std::{
    f64::consts::{FRAC_1_SQRT_2, FRAC_PI_2, FRAC_PI_4}, io::Write, sync::atomic::Ordering, time::Instant
};

use fxhash::FxBuildHasher;
use indexmap::IndexSet;
use nalgebra::{Rotation2, Vector2};
use rand::{thread_rng, Rng};
use spin_sleep::SpinSleeper;
use urobotics::{
    define_callbacks, fn_alias, log::OwoColorize, parking_lot::{RwLockReadGuard, RwLockWriteGuard}, task::SyncTask,
};

use crate::simbot::END_POINT;

use super::{Obstacles, COLLIDED, DRIVE_HISTORY, OBSTACLES, REFRESH_RATE, SIMBOT_DIRECTION, SIMBOT_ORIGIN};

pub mod solution;

define_callbacks!(RaycastCallbacks => Fn(metric: (Vector2<f64>, f64)) + Send);
fn_alias! {
    pub type RaycastCallbacksRef = CallbacksRef((Vector2<f64>, f64)) + Send
}

#[derive(Default)]
pub struct LinearMazeSensor {
    raycast_callbacks: RaycastCallbacks,
}

impl LinearMazeSensor {
    pub fn raycast_callbacks_ref(&self) -> RaycastCallbacksRef {
        self.raycast_callbacks.get_ref()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TurnType {
    Left,
    Right,
}

pub(super) fn generate_linear_maze() -> (RwLockReadGuard<'static, Obstacles>, Vector2<f64>) {
    let mut rng = thread_rng();
    let mut obstacles = OBSTACLES.write();
    let end_point;

    if obstacles.vertices.is_empty() {
        let mut origin = SIMBOT_ORIGIN.load();
        let mut direction = SIMBOT_DIRECTION.load();
        let mut vertices = IndexSet::<Vector2<isize>, FxBuildHasher>::default();

        macro_rules! add_wall {
            ($from:expr, $to:expr) => {
                let from = $from / 0.5;
                let to = $to / 0.5;
                let from = Vector2::new(from.x.round() as isize, from.y.round() as isize);
                let to = Vector2::new(to.x.round() as isize, to.y.round() as isize);

                let (from_index, from_is_new) = vertices.insert_full(from);
                let (to_index, to_is_new) = vertices.insert_full(to);

                if from_is_new {
                    obstacles.vertices.push(from.cast::<f64>() * 0.5);
                }

                if to_is_new {
                    obstacles.vertices.push(to.cast::<f64>() * 0.5);
                }

                obstacles.edges.push((from_index, to_index));
            };
        }

        let mut last_left_corner = Rotation2::new(direction + FRAC_PI_2 + FRAC_PI_4)
            * Vector2::new(FRAC_1_SQRT_2, 0.0)
            + origin;
        let mut last_right_corner = Rotation2::new(direction - FRAC_PI_2 - FRAC_PI_4)
            * Vector2::new(FRAC_1_SQRT_2, 0.0)
            + origin;
        add_wall!(last_left_corner, last_right_corner);
        origin += Rotation2::new(direction) * Vector2::new(rng.gen_range(1..=5) as f64, 0.0);

        'main: for _ in 0..rng.gen_range(7..=13) {
            let mut turn_options =
                heapless::Vec::<_, 2>::from_slice(&[TurnType::Left, TurnType::Right]).unwrap();

            loop {
                let rand_turn_index = rng.gen_range(0..turn_options.len());
                let turn_type = turn_options[rand_turn_index];

                match turn_type {
                    TurnType::Left => direction += FRAC_PI_2,
                    TurnType::Right => direction -= FRAC_PI_2,
                }

                let distance = rng.gen_range(1..=5);

                if let Some(raycast_distance) = obstacles.raycast::<f64>(origin, direction) {
                    if raycast_distance < 1.5 {
                        match turn_type {
                            TurnType::Left => direction -= FRAC_PI_2,
                            TurnType::Right => direction += FRAC_PI_2,
                        }
                        turn_options.swap_remove(rand_turn_index);
                        if turn_options.is_empty() {
                            break 'main;
                        }
                        continue;
                    } else if raycast_distance < distance as f64 + 0.5 {
                        match turn_type {
                            TurnType::Left => direction -= FRAC_PI_2,
                            TurnType::Right => direction += FRAC_PI_2,
                        }
                        continue;
                    }
                }

                match turn_type {
                    TurnType::Left => {
                        let new_left_corner = Rotation2::new(direction + FRAC_PI_4)
                            * Vector2::new(FRAC_1_SQRT_2, 0.0)
                            + origin;
                        let new_right_corner =
                            Rotation2::new(direction - FRAC_PI_2 - FRAC_PI_4)
                                * Vector2::new(FRAC_1_SQRT_2, 0.0)
                                + origin;
                        add_wall!(last_left_corner, new_left_corner);
                        add_wall!(last_right_corner, new_right_corner);
                        last_left_corner = new_left_corner;
                        last_right_corner = new_right_corner;
                    }
                    TurnType::Right => {
                        let new_left_corner = Rotation2::new(direction + FRAC_PI_2 + FRAC_PI_4)
                            * Vector2::new(FRAC_1_SQRT_2, 0.0)
                            + origin;
                        let new_right_corner = Rotation2::new(direction - FRAC_PI_4)
                            * Vector2::new(FRAC_1_SQRT_2, 0.0)
                            + origin;
                        add_wall!(last_left_corner, new_left_corner);
                        add_wall!(last_right_corner, new_right_corner);
                        last_left_corner = new_left_corner;
                        last_right_corner = new_right_corner;
                    }
                }
                origin += Rotation2::new(direction) * Vector2::new(distance as f64, 0.0);

                break;
            }
        }

        let new_left_corner =
            Rotation2::new(direction + FRAC_PI_4) * Vector2::new(FRAC_1_SQRT_2, 0.0) + origin;
        let new_right_corner =
            Rotation2::new(direction - FRAC_PI_4) * Vector2::new(FRAC_1_SQRT_2, 0.0) + origin;
        add_wall!(last_left_corner, new_left_corner);
        add_wall!(last_right_corner, new_right_corner);
        add_wall!(new_left_corner, new_right_corner);
        END_POINT.store(origin);
        end_point = origin;

        let mut maze = std::fs::File::create("maze.toml").expect("Failed to create maze.toml");
        writeln!(
            maze,
            "{}\nend = [{:.1}, {:.1}]",
            toml::to_string(&*obstacles).unwrap(),
            end_point.x,
            end_point.y
        )
        .expect("Failed to write to maze.toml");

    } else {
        end_point = END_POINT.load();
    }
    (RwLockWriteGuard::downgrade(obstacles), end_point)
}

impl SyncTask for LinearMazeSensor {
    type Output = Result<String, String>;

    fn run(mut self) -> Self::Output {
        let (obstacles, end_point) = generate_linear_maze();
        let mut obstacles_obj = std::io::BufWriter::new(
            std::fs::File::create("maze.obj").expect("Failed to create maze.obj"),
        );

        for &vertex in &obstacles.vertices {
            writeln!(obstacles_obj, "v {} {} 0.0", vertex.x, vertex.y)
                .expect("Failed to write to maze.obj");
        }

        for &vertex in &obstacles.vertices {
            writeln!(obstacles_obj, "v {} {} 0.3", vertex.x, vertex.y)
                .expect("Failed to write to maze.obj");
        }

        let offset = obstacles.vertices.len();
        for &(mut from, mut to) in &obstacles.edges {
            from += 1;
            to += 1;
            writeln!(
                obstacles_obj,
                "f {to} {from} {} {}",
                from + offset,
                to + offset
            )
            .expect("Failed to write to maze.obj");
        }

        let sleeper = SpinSleeper::default();
        let start_time = Instant::now();
        let start_origin = SIMBOT_ORIGIN.load();
        writeln!(
            obstacles_obj,
            "v {} {} -0.2",
            start_origin.x, start_origin.y
        )
        .expect("Failed to write to maze.obj");
        writeln!(obstacles_obj, "v {} {} 0.5", start_origin.x, start_origin.y)
            .expect("Failed to write to maze.obj");

        let result = loop {
            if COLLIDED.load(Ordering::Relaxed) {
                break Err("Your program collided with an obstacle!".to_string());
            }
            let origin = SIMBOT_ORIGIN.load();
            if (origin - end_point).magnitude() <= 0.5 {
                break Ok(format!(
                    "Your program reached the end in {:.2} secs!",
                    start_time.elapsed().as_secs_f32()
                )
                .green()
                .to_string());
            }
            self.raycast_callbacks
                .call(obstacles.raycast(origin, SIMBOT_DIRECTION.load()).unwrap());
            sleeper.sleep(REFRESH_RATE);
            if start_time.elapsed().as_secs_f32() > 5.0 {
                break Err("Your program took longer than 5 secs to reach the end".to_string());
            }
        };

        let history_len = DRIVE_HISTORY.len();
        for _ in 0..history_len {
            let next = DRIVE_HISTORY.pop().unwrap();
            writeln!(obstacles_obj, "v {} {} -0.2", next.x, next.y)
                .expect("Failed to write to maze.obj");
            writeln!(obstacles_obj, "v {} {} 0.5", next.x, next.y)
                .expect("Failed to write to maze.obj");
        }
        let mut current = obstacles.vertices.len() * 2 + 1;
        for _ in 0..history_len {
            writeln!(
                obstacles_obj,
                "f {} {} {} {}",
                current,
                current + 1,
                current + 3,
                current + 2
            )
            .expect("Failed to write to maze.obj");
            current += 2;
        }

        obstacles_obj.flush().expect("Failed to write to maze.obj");
        result
    }
}
