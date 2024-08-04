use serde::Deserialize;
use urobotics::{app::Application, task::SyncTask};

use crate::multiples_of_two::MultiplesOfTwo;


#[derive(Deserialize)]
pub struct MultiplesOfTwoSolution {}

impl Application for MultiplesOfTwoSolution {
    const APP_NAME: &'static str = "mul2soln";
    const DESCRIPTION: &'static str = "A solution to the multiples of two problem";

    fn run(self) {
        let mul2 = MultiplesOfTwo::default();
        let answer_fn = mul2.get_answer_fn();
        mul2.random_input_callbacks_ref().add_fn(move |num| {
            answer_fn(num * 2);
        });
        let _ = mul2.spawn().join();
    }
}