use rand::{thread_rng, Rng};
use urobotics::{define_callbacks, fn_alias, log::OwoColorize, task::SyncTask};

pub mod solution;

define_callbacks!(pub RandIntCallbacks => Fn(num: isize) + Send);
fn_alias! {
    pub type RandIntCallbacksRef = CallbacksRef(isize) + Send
}

pub struct MultiplesOfTwo {
    random_input_callbacks: RandIntCallbacks,
    answer_tx: std::sync::mpsc::Sender<isize>,
    answer_rx: std::sync::mpsc::Receiver<isize>,
}

impl Default for MultiplesOfTwo {
    fn default() -> Self {
        let (answer_tx, answer_rx) = std::sync::mpsc::channel();
        Self {
            random_input_callbacks: RandIntCallbacks::default(),
            answer_tx,
            answer_rx,
        }
    }
}

impl MultiplesOfTwo {
    pub fn random_input_callbacks_ref(&self) -> RandIntCallbacksRef {
        self.random_input_callbacks.get_ref()
    }

    pub fn get_answer_fn(&self) -> impl Fn(isize) + Clone {
        let tx = self.answer_tx.clone();
        move |num| {
            let _ = tx.send(num);
        }
    }
}

impl SyncTask for MultiplesOfTwo {
    type Output = Result<String, String>;

    fn run(mut self) -> Self::Output {
        drop(self.answer_tx);
        for _ in 0..10 {
            let n = thread_rng().gen_range(-100..100);
            self.random_input_callbacks.call(n);
            let ans = self
                .answer_rx
                .recv_timeout(std::time::Duration::from_secs(1))
                .map_err(|e| match e {
                    std::sync::mpsc::RecvTimeoutError::Timeout => {
                        "Your program took longer than 1 second".to_string()
                    }
                    std::sync::mpsc::RecvTimeoutError::Disconnected => {
                        "Your program ended prematurely, or it never used `get_answer_fn"
                            .to_string()
                    }
                })?;
            if ans != n * 2 {
                return Err(format!(
                    "Your program returned an incorrect answer for input {}. Expected {}, got {}",
                    n,
                    n * 2,
                    ans
                ));
            }
        }

        if self
            .answer_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .is_ok()
        {
            return Err("Your program did not terminate after 10 tests".to_string());
        }

        Ok("Your program has passed all tests!".green().to_string())
    }
}
