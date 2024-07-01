//! Services can be thought of as a long running function whose execution has to be requested through
//! an API, and its status can be tracked while it is running, and the return value will be provided
//! back to the service requester.

use std::future::Future;

pub trait Service: 'static {
    type ScheduleData<'a>
    where
        Self: 'a;
    type TaskData<'a>
    where
        Self: 'a;
    type Output<'a>
    where
        Self: 'a;

    fn call_with_data<'a>(
        &'a mut self,
        data: Self::ScheduleData<'a>,
    ) -> (
        Self::TaskData<'a>,
        impl Future<Output = Self::Output<'a>> + Send,
    );
}

pub trait ServiceExt: for<'a> Service<TaskData<'a> = ()> {
    fn call<'a>(
        &'a mut self,
        data: Self::ScheduleData<'a>,
    ) -> impl Future<Output = Self::Output<'a>> + Send {
        self.call_with_data(data).1
    }
}

impl<T> ServiceExt for T where T: for<'a> Service<TaskData<'a> = ()> {}