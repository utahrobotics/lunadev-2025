//! Services can be thought of as a long running function whose execution has to be requested through
//! an API, and its status can be tracked while it is running, and the return value will be provided
//! back to the service requester.

use std::future::Future;

pub struct Service<F: ?Sized> {
    func: Box<F>,
}

macro_rules! implementation {
    ($($t:tt)*) => {
        impl<ScheduleData, TaskData, Fut: Future> Service<dyn FnMut(ScheduleData) -> (TaskData, Fut)$($t)*> {
            pub fn call_with_data(&mut self, schedule_data: ScheduleData) -> (TaskData, Fut) {
                (self.func)(schedule_data)
            }
        }

        impl<ScheduleData, Fut: Future> Service<dyn FnMut(ScheduleData) -> Fut$($t)*> {
            pub fn call(&mut self, schedule_data: ScheduleData) -> Fut {
                (self.func)(schedule_data)
            }
        }

        impl<ScheduleData, F, T> From<F> for Service<dyn FnMut(ScheduleData) -> T$($t)*>
        where
            F: FnMut(ScheduleData) -> T + 'static$($t)*,
        {
            fn from(func: F) -> Self {
                Self {
                    func: Box::new(func)
                }
            }
        }
    };
}

implementation!();
implementation!(+Send);
implementation!(+Sync);
implementation!(+Send+Sync);
