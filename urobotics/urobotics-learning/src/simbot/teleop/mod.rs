use std::{collections::BinaryHeap, f64::consts::{FRAC_PI_2, PI}, sync::mpsc::{Receiver, Sender}, time::{Duration, Instant}};

use rand::{thread_rng, Rng};
use urobotics::{define_callbacks, fn_alias, task::SyncTask};

pub mod solution;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DriveInstruction {
    Drive(f64),
    Turn(f64),
}

define_callbacks!(DriveCallbacks => Fn(instruction: DriveInstruction) + Send);
fn_alias! {
    pub type DriveCallbacksRef = CallbacksRef(DriveInstruction) + Send
}

pub const MAX_PING: u64 = 50;


pub struct LinearMazeTeleop<T, F1> {
    filter: F1,
    drive_callbacks: DriveCallbacks,
    raycast_distance_tx: Sender<(f64, T)>,
    raycast_distance_rx: Receiver<(f64, T)>,
}

impl<T, F1: FnMut(T) -> bool + 'static> LinearMazeTeleop<T, F1> {
    pub fn new(filter: F1) -> Self {
        let (raycast_distance_tx, raycast_distance_rx) = std::sync::mpsc::channel();
        Self {
            filter,
            drive_callbacks: DriveCallbacks::default(),
            raycast_distance_tx,
            raycast_distance_rx,
        }
    }

    pub fn drive_callbacks_ref(&self) -> DriveCallbacksRef {
        self.drive_callbacks.get_ref()
    }

    pub fn raycast_callback(&self) -> impl Fn(f64, T) + Clone {
        let tx = self.raycast_distance_tx.clone();
        move |distance, data| {
            let _ = tx.send((distance, data));
        }
    }
}

enum Event<T> {
    Distance(f64, T),
    Drive(DriveInstruction),
}

struct HeapElement<T>(Instant, Event<T>);

impl<T> Ord for HeapElement<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.0.cmp(&self.0)
    }
}

impl<T> PartialOrd for HeapElement<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> PartialEq for HeapElement<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<T> Eq for HeapElement<T> {}


impl<T: Send + 'static, F1: FnMut(T) -> bool + Send + 'static> SyncTask for LinearMazeTeleop<T, F1> {
    type Output = Result<!, String>;

    fn run(mut self) -> Self::Output {
        let mut rng = thread_rng();
        drop(self.raycast_distance_tx);
        let mut heap = BinaryHeap::<HeapElement<T>>::default();
        let mut turned_left = false;

        let mut rand_instant = || {
            Instant::now() + Duration::from_millis(rng.gen_range(0..=MAX_PING))
        };

        loop {
            if let Some(HeapElement(instant, _)) = heap.peek() {
                match self.raycast_distance_rx.recv_deadline(*instant) {
                    Ok((distance, data)) => {
                        heap.push(HeapElement(rand_instant(), Event::Distance(distance, data)));
                    }
                    Err(e) => match e {
                        std::sync::mpsc::RecvTimeoutError::Timeout => match heap.pop().unwrap().1 {
                            Event::Distance(distance, data) => if (self.filter)(data) {
                                if (0.5 - distance).abs() < 0.01 {
                                    if turned_left {
                                        turned_left = false;
                                        heap.push(HeapElement(rand_instant(), Event::Drive(DriveInstruction::Turn(-PI))));
                                    } else {
                                        turned_left = true;
                                        heap.push(HeapElement(rand_instant(), Event::Drive(DriveInstruction::Turn(FRAC_PI_2))));
                                    }
                                } else {
                                    turned_left = false;
                                    heap.push(HeapElement(rand_instant(), Event::Drive(DriveInstruction::Drive(distance - 0.5))));
                                }
                            },
                            Event::Drive(d) => self.drive_callbacks.call(d),
                        }
                        std::sync::mpsc::RecvTimeoutError::Disconnected => break Err("Your program terminated prematurely".to_string()),
                    }
                }
            } else {
                let Ok((distance, data)) = self.raycast_distance_rx.recv() else {
                    break Err("Your program terminated prematurely".to_string());
                };
                heap.push(HeapElement(rand_instant(), Event::Distance(distance, data)));
            }
        }
    }
}
