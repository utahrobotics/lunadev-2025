//! Services can be thought of as a long running function whose execution has to be requested through
//! an API, and its status can be tracked while it is running, and the return value will be provided
//! back to the service requester.

use std::future::Future;

pub trait Service<ScheduleData, TaskData = ()> {
    type Future: Future;

    fn invoke(&mut self, data: ScheduleData) -> (TaskData, Self::Future);
}

impl<ScheduleData, TaskData, Fut: Future, F: FnMut(ScheduleData) -> (TaskData, Fut)>
    Service<ScheduleData, TaskData> for F
{
    type Future = Fut;

    fn invoke(&mut self, data: ScheduleData) -> (TaskData, Self::Future) {
        self(data)
    }
}

#[cfg(test)]
mod tests {
    fn test(_record: &log::Record) -> ((), impl Future) {
        ((), async {})
    }

    fn test2() -> impl for<'a, 'b> Service<&'a log::Record<'b>> {
        test
    }
}
