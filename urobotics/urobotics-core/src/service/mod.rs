//! Services can be thought of as a long running function whose execution has to be requested through
//! an API, and its status can be tracked while it is running, and the return value will be provided
//! back to the service requester.

use tokio::sync::{mpsc, oneshot};

/// The API for a `Service`. This is essentially the public facing end of a `Service`.
///
/// There are 4 generic types that must be specified:
///
/// 1. `ScheduleInput` - The input data that the `Service` requires to schedule a task.
/// 2. `ScheduleError` - The type of the error that may occur during scheduling.
/// 3. `ScheduleOutput` - The data that the `Service` returned when scheduling is successful.
/// 4. `TaskOutput` - The data that is returned when the scheduled task is done.
pub struct ServiceHandle<ScheduleInput, ScheduleError, ScheduleOutput, TaskOutput> {
    schedule_sender: mpsc::UnboundedSender<
        ScheduleRequest<ScheduleInput, ScheduleError, ScheduleOutput, TaskOutput>,
    >,
}

impl<ScheduleInput, ScheduleError, ScheduleOutput, TaskOutput> Clone
    for ServiceHandle<ScheduleInput, ScheduleError, ScheduleOutput, TaskOutput>
{
    fn clone(&self) -> Self {
        Self {
            schedule_sender: self.schedule_sender.clone(),
        }
    }
}

impl<ScheduleInput, ScheduleError, ScheduleOutput, TaskOutput>
    ServiceHandle<ScheduleInput, ScheduleError, ScheduleOutput, TaskOutput>
{
    /// Try to schedule a task, or return `None` if the `Service` was dropped.
    pub async fn try_schedule_or_closed(
        &self,
        input: ScheduleInput,
    ) -> Option<Result<ScheduledService<ScheduleOutput, TaskOutput>, ScheduleError>> {
        let (sender, receiver) = oneshot::channel();
        self.schedule_sender
            .send(ScheduleRequest {
                input: Some(input),
                sender,
            })
            .ok()?;
        receiver.await.ok()
    }

    /// Try to schedule a task, awaiting forever if the `Service` was dropped.
    pub async fn try_schedule(
        &self,
        input: ScheduleInput,
    ) -> Result<ScheduledService<ScheduleOutput, TaskOutput>, ScheduleError> {
        let Some(out) = self.try_schedule_or_closed(input).await else {
            std::future::pending::<()>().await;
            unreachable!()
        };
        out
    }
}

/// Represents a task that was successfully scheduled.
///
/// This is used by service requesters.
pub struct ScheduledService<ScheduleOutput, TaskOutput> {
    data: Option<ScheduleOutput>,
    output_recv: oneshot::Receiver<TaskOutput>,
}

/// Represents a request to schedule a task.
///
/// This is used by service providers.
pub struct ScheduleRequest<ScheduleInput, ScheduleError, ScheduleOutput, TaskOutput> {
    input: Option<ScheduleInput>,
    sender: oneshot::Sender<Result<ScheduledService<ScheduleOutput, TaskOutput>, ScheduleError>>,
}

/// A scheduled task that is awaiting output.
///
/// This is used by service providers.
pub struct Pending<TaskOutput> {
    output_sender: oneshot::Sender<TaskOutput>,
}

/// The private end of a `Service`.
///
/// Users will access this `Service` through its `ServiceHandle`. As such, no references
/// to this should be shared.
///
/// There are 4 generic types that must be specified:
///
/// 1. `ScheduleInput` - The input data that the `Service` requires to schedule a task.
/// 2. `ScheduleError` - The type of the error that may occur during scheduling.
/// 3. `ScheduleOutput` - The data that the `Service` returned when scheduling is successful.
/// 4. `TaskOutput` - The data that is returned when the scheduled task is done.
pub struct Service<ScheduleInput, ScheduleError, ScheduleOutput, TaskOutput> {
    schedule_recv: mpsc::UnboundedReceiver<
        ScheduleRequest<ScheduleInput, ScheduleError, ScheduleOutput, TaskOutput>,
    >,
}

/// Creates a new `Service` and `ServiceHandle` that are tied together.
///
/// There are 4 generic types that must be specified:
///
/// 1. `ScheduleInput` - The input data that the `Service` requires to schedule a task.
/// 2. `ScheduleError` - The type of the error that may occur during scheduling.
/// 3. `ScheduleOutput` - The data that the `Service` returned when scheduling is successful.
/// 4. `TaskOutput` - The data that is returned when the scheduled task is done.
#[must_use]
pub fn new_service<ScheduleInput, ScheduleError, ScheduleOutput, TaskOutput>() -> (
    Service<ScheduleInput, ScheduleError, ScheduleOutput, TaskOutput>,
    ServiceHandle<ScheduleInput, ScheduleError, ScheduleOutput, TaskOutput>,
) {
    let (schedule_sender, schedule_recv) = mpsc::unbounded_channel();
    (Service { schedule_recv }, ServiceHandle { schedule_sender })
}

impl<ScheduleInput, ScheduleError, ScheduleOutput, TaskOutput>
    Service<ScheduleInput, ScheduleError, ScheduleOutput, TaskOutput>
{
    /// Waits for a new scheduling request.
    pub async fn wait_for_request(
        &mut self,
    ) -> Option<ScheduleRequest<ScheduleInput, ScheduleError, ScheduleOutput, TaskOutput>> {
        self.schedule_recv.recv().await
    }

    /// Waits for a new scheduling request in a blocking way.
    pub fn blocking_wait_for_request(
        &mut self,
    ) -> Option<ScheduleRequest<ScheduleInput, ScheduleError, ScheduleOutput, TaskOutput>> {
        self.schedule_recv.blocking_recv()
    }
}

impl<ScheduleInput, ScheduleError, ScheduleOutput, TaskOutput>
    ScheduleRequest<ScheduleInput, ScheduleError, ScheduleOutput, TaskOutput>
{
    /// Accepts this `ScheduleRequest`, indicating to the requester that the task
    /// will now start processing.
    ///
    /// The given data will be sent to the requester.
    pub fn accept(self, data: ScheduleOutput) -> Option<Pending<TaskOutput>> {
        let (output_sender, output_recv) = oneshot::channel();
        if self
            .sender
            .send(Ok(ScheduledService {
                data: Some(data),
                output_recv,
            }))
            .is_err()
        {
            None
        } else {
            Some(Pending { output_sender })
        }
    }

    /// Rejects the `ScheduleRequest` with the given error that the requester will receive.
    pub fn reject(self, error: ScheduleError) {
        let _ = self.sender.send(Err(error));
    }

    /// Get a reference to the `ScheduleInput`, assuming it has not been taken yet.
    pub fn get_input(&self) -> Option<&ScheduleInput> {
        self.input.as_ref()
    }

    /// Get a mutable reference to the `ScheduleInput`, assuming it has not been taken yet.
    pub fn get_mut_input(&mut self) -> Option<&mut ScheduleInput> {
        self.input.as_mut()
    }

    /// Takes the `ScheduleInput`.
    ///
    /// Subsequent calls to this and the various `get_*` methods will return `None`.
    pub fn take_input(&mut self) -> Option<ScheduleInput> {
        self.input.take()
    }
}

impl<TaskOutput> Pending<TaskOutput> {
    /// Finishes this task by providing its `TaskOutput`.
    ///
    /// It is not guaranteed that the requester will receive this, since
    /// the requester may drop their handle before this method is called.
    pub fn finish(self, output: TaskOutput) {
        let _ = self.output_sender.send(output);
    }

    /// Checks if the requester has dropped their handle to the task.
    ///
    /// Do not rely on this to guarantee that the requester will receive
    /// the `TaskOutput` due to TOCTOU issues.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.output_sender.is_closed()
    }
}

impl<ScheduleOutput, TaskOutput> ScheduledService<ScheduleOutput, TaskOutput> {
    /// Get a reference to the `ScheduleOutput`, assuming it has not been taken yet.
    pub fn get_data(&self) -> Option<&ScheduleOutput> {
        self.data.as_ref()
    }

    /// Get a mutable reference to the `ScheduleOutput`, assuming it has not been taken yet.
    pub fn get_mut_data(&mut self) -> Option<&mut ScheduleOutput> {
        self.data.as_mut()
    }

    /// Takes the `ScheduleOutput`.
    ///
    /// Subsequent calls to this and the various `get_*` methods will return `None`.
    pub fn take_data(&mut self) -> Option<ScheduleOutput> {
        self.data.take()
    }

    /// Waits for the `TaskOutput`, or returns `None` if the `Service` was dropped.
    pub async fn wait_or_closed(self) -> Option<TaskOutput> {
        self.output_recv.await.ok()
    }

    /// Waits for the `TaskOutput`, awaiting forever if the `Service` was dropped.
    pub async fn wait(self) -> TaskOutput {
        let Some(out) = self.wait_or_closed().await else {
            std::future::pending::<()>().await;
            unreachable!()
        };
        out
    }
}
