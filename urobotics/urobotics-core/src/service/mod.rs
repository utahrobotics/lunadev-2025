//! Services can be thought of as a long running function whose execution has to be requested through
//! an API, and its status can be tracked while it is running, and the return value will be provided
//! back to the service requester.

use std::future::Future;

pub trait Service<ScheduleData, TaskData = ()>:
    FnMut(ScheduleData) -> (TaskData, Self::Future)
{
    type Future: Future;
}

impl<ScheduleData, TaskData, Fut: Future, F: FnMut(ScheduleData) -> (TaskData, Fut)>
    Service<ScheduleData, TaskData> for F
{
    type Future = Fut;
}
