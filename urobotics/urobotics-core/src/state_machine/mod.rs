use std::{cell::{Cell, RefCell}, marker::PhantomData, num::NonZeroUsize, ops::Deref, rc::Rc, sync::{atomic::{AtomicBool, Ordering}, Arc}};

use futures::Future;
use parking_lot::Mutex;
use tokio::{runtime::Builder, sync::{oneshot, Notify}};

thread_local! {
    static RETAIN_STATE: Cell<bool> = Cell::new(false);
    static STATE_DATA: RefCell<Option<Rc<StateThreadData>>> = RefCell::new(None);
}

pub fn drop_this_state() {
    RETAIN_STATE.set(false);
}

pub fn retain_this_state() {
    RETAIN_STATE.set(true);
}


struct EnterNotify {
    enter_requested: AtomicBool,
    entering: Notify,
    dropped: Arc<Notify>
}


struct StateThreadData {
    drop: Notify,
    handle: StateHandle
}


struct StateHandleInner {
    enter: EnterNotify,
    interrupts: Mutex<Vec<Box<dyn FnMut() -> Option<Arc<Self>> + Send>>>,
    active: AtomicBool,
}

#[derive(Clone)]
struct StateHandle(Option<Arc<StateHandleInner>>);


impl StateHandle {
    fn new(inner: StateHandleInner) -> Self {
        Self(Some(Arc::new(inner)))
    }
}

impl Drop for StateHandle {
    fn drop(&mut self) {
        let dropped = self.enter.dropped.clone();
        self.0 = None;
        dropped.notify_waiters();
    }
}

impl Deref for StateHandle {
    type Target = StateHandleInner;

    fn deref(&self) -> &Self::Target {
        self.0.as_ref().unwrap()
    }
}


pub trait State: Sized + Send + 'static {
    type Output;
    const PARALLELISM: NonZeroUsize = NonZeroUsize::new(1).unwrap();

    fn run(&mut self) -> impl Future<Output=Self::Output>;
    fn into_state<'a>(mut self, token: StateMachineToken<'a>) -> (StateRef<'a>, OnStateOutput<'a, Self::Output>) {
        let state_handle = StateHandle::new(StateHandleInner {
            enter: EnterNotify {
                enter_requested: AtomicBool::new(false),
                entering: Notify::new(),
                dropped: Arc::new(Notify::new()),
            },
            interrupts: Mutex::default(),
            active: AtomicBool::new(false),
        });
        let state_handle_clone = state_handle.clone();
        let (f_sender, mut f_receiver) = oneshot::channel();

        std::thread::spawn(move || {
            let state_thr_data = Rc::new(StateThreadData {
                drop: Notify::new(),
                handle: state_handle,
            });
            STATE_DATA.set(Some(state_thr_data.clone()));

            let mut builder = if Self::PARALLELISM.get() == 1 {
                Builder::new_current_thread()
            } else {
                let mut builder = Builder::new_multi_thread();
                builder
                    .worker_threads(Self::PARALLELISM.get());
                builder
            };

            let runtime = builder
                .enable_all()
                .build()
                .unwrap();
            
            runtime.block_on(async {
                let mut on_state_output: Option<Box<dyn FnMut(Self::Output) -> Option<StateHandle> + Send + Sync>> = None;
                'main: loop {
                    loop {
                        if state_thr_data.handle.enter.enter_requested.swap(false, Ordering::Relaxed) {
                            break;
                        }
                        if Arc::strong_count(state_thr_data.handle.0.as_ref().unwrap()) == 1 {
                            break 'main;
                        }
                        tokio::select! {
                            _ = state_thr_data.handle.enter.entering.notified() => {
                                state_thr_data.handle.enter.enter_requested.store(false, Ordering::Relaxed);
                                break
                            }
                            _ = state_thr_data.handle.enter.dropped.notified() => {}
                        }
                    }
                    state_thr_data.handle.active.store(true, Ordering::Relaxed);
                    tokio::select! {
                        output = self.run() => {
                            state_thr_data.handle.active.store(false, Ordering::Relaxed);
                            let f = if let Some(f) = on_state_output.as_mut() {
                                f
                            } else if let Ok(f) = f_receiver.try_recv() {
                                on_state_output = Some(f);
                                on_state_output.as_mut().unwrap()
                            } else {
                                token.ended.notify_waiters();
                                break;
                            };
                            if let Some(next_state) = f(output) {
                                next_state.enter.enter_requested.store(true, Ordering::Relaxed);
                                next_state.enter.entering.notify_waiters();
                            } else {
                                token.ended.notify_waiters();
                                break;
                            }
                        },
                        _ = state_thr_data.drop.notified() => {}
                    }
                }
                STATE_DATA.set(None);
            });
        });

        (
            StateRef { handle: state_handle_clone, phantom: PhantomData },
            OnStateOutput {
                f_sender,
                phantom: PhantomData
            }
        )
    }
}


impl<Fut: Future, F: FnMut() -> Fut + Send + 'static> State for F {
    type Output = Fut::Output;

    fn run(&mut self) -> impl Future<Output=Self::Output> {
        self()
    }
}


#[derive(Clone)]
pub struct StateRef<'a> {
    handle: StateHandle,
    phantom: PhantomData<&'a ()>
}


impl<'a> StateRef<'a> {
    pub fn add_interrupt(&self, mut f: impl FnMut() -> Option<Self> + Send + 'static) {
        self.handle.interrupts.lock().push(Box::new(move || f().map(|state_ref| state_ref.handle.0.clone().unwrap())));
    }
}


pub struct OnStateOutput<'a, T> {
    f_sender: oneshot::Sender<Box<dyn FnMut(T) -> Option<StateHandle> + Send + Sync>>,
    phantom: PhantomData<&'a ()>
}


impl<'a, T> OnStateOutput<'a, T> {
    pub fn set(self, mut f: impl FnMut(T) -> Option<StateRef<'a>> + Send + Sync + 'static) {
        let _ = self.f_sender.send(Box::new(move |output| f(output).map(|state_ref| state_ref.handle.clone())));
    }
}


#[derive(Clone)]
pub struct StateMachineToken<'a> {
    ended: Arc<Notify>,
    phantom: PhantomData<&'a ()>
}


pub fn check_interrupts() -> Option<impl Future<Output=()>> {
    let state_thr_data = STATE_DATA.with_borrow(Clone::clone).expect("Not running in a State");
    for f in state_thr_data.handle.clone().interrupts.lock().iter_mut() {
        if let Some(next_state) = f() {
            return Some(async move {
                if !RETAIN_STATE.replace(true) {
                    state_thr_data.drop.notify_waiters();
                    std::future::pending::<()>().await;
                }
                next_state.enter.enter_requested.store(true, Ordering::Relaxed);
                next_state.enter.entering.notify_waiters();
                state_thr_data.handle.enter.entering.notified().await;
            });
        }
    }
    None
}


#[inline]
pub async fn interrupts() {
    let state_thr_data = STATE_DATA.with_borrow(Clone::clone).expect("Not running in a State");
    for f in state_thr_data.handle.clone().interrupts.lock().iter_mut() {
        if let Some(next_state) = f() {
            next_state.enter.enter_requested.store(true, Ordering::Relaxed);
            next_state.enter.entering.notify_waiters();
            state_thr_data.handle.enter.entering.notified().await;
        }
    }
}


#[derive(Default)]
pub struct StateMachine {

}


impl StateMachine {
    pub fn build<'a>(self, f: impl FnOnce(StateMachineToken<'a>) -> StateRef<'a>) -> RunningStateMachine {
        let ended = Arc::new(Notify::new());
        let state = f(StateMachineToken {
            ended: ended.clone(),
            phantom: PhantomData
        });
        state.handle.enter.enter_requested.store(true, Ordering::Relaxed);
        state.handle.enter.entering.notify_waiters();
        RunningStateMachine { ended }
    }
}


pub struct RunningStateMachine {
    ended: Arc<Notify>
}

impl RunningStateMachine {
    pub async fn wait(self) {
        self.ended.notified().await;
        loop {
            if Arc::strong_count(&self.ended) == 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
    }
}


#[cfg(test)]
mod test {
    use super::*;
    #[tokio::test]

    async fn adder_test() {
        let sum = Arc::new(AtomicUsize::new(0));
        StateMachine::default()
            .build(|token| {
                let mut sum2 = sum.clone();
                let (adder, adder_on_output) = (move || {
                    sum2.fetch_add(1, Ordering::Relaxed);
                    async {}
                }).into_state(token.clone());
    
                sum2 = sum.clone();
                let (checker, checker_on_output) = (move || {
                    assert!(sum2.load(Ordering::Relaxed) <= 1500);
                    let result = sum2.load(Ordering::Relaxed) == 1500;
                    async move { result }
                }).into_state(token);
    
                let adder2 = adder.clone();
                checker_on_output.set(move |result| {
                    if result {
                        None
                    } else {
                        Some(adder2.clone())
                    }
                });
                adder_on_output.set(move |_| Some(checker.clone()));
    
                adder
            }).wait().await;
        
        assert_eq!(*Arc::try_unwrap(sum).unwrap().get_mut(), 1500);
    }
}
