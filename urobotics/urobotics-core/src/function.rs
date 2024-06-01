use std::future::Future;

use tokio::task::AbortHandle;

use crate::runtime::RuntimeContext;

pub trait SyncFunctionConfig {
    type Output: Loggable;
    const NAME: &'static str;
    const PERSISTENT: bool = false;

    fn run(self, context: &RuntimeContext) -> Self::Output;
    fn spawn_persistent(self, context: RuntimeContext)
    where
        Self: Sized + Send + 'static,
    {
        context.clone().spawn_persistent_sync(move || {
            self.run(&context).log(Self::NAME);
        });
    }
    fn spawn(self, context: RuntimeContext)
    where
        Self: Sized + Send + 'static,
    {
        if Self::PERSISTENT {
            self.spawn_persistent(context);
        } else {
            std::thread::spawn(move || {
                self.run(&context).log(Self::NAME);
            });
        }
    }
}

pub trait AsyncFunctionConfig {
    type Output: Loggable;
    const NAME: &'static str;
    const PERSISTENT: bool = false;

    fn run(self, context: &RuntimeContext) -> impl Future<Output = Self::Output> + Send;
    fn spawn_persistent(self, context: RuntimeContext)
    where
        Self: Sized + Send + 'static,
    {
        context.clone().spawn_persistent_async(async move {
            self.run(&context).await.log(Self::NAME);
        });
    }
    fn spawn(self, context: RuntimeContext) -> Option<AbortHandle>
    where
        Self: Sized + Send + 'static,
    {
        if Self::PERSISTENT {
            self.spawn_persistent(context);
            None
        } else {
            Some(
                context
                    .clone()
                    .spawn_async(async move {
                        self.run(&context).await.log(Self::NAME);
                    })
                    .abort_handle(),
            )
        }
    }
}

pub trait Loggable {
    fn log(self, target: &str);
}

impl Loggable for () {
    fn log(self, _target: &str) {}
}

impl Loggable for ! {
    fn log(self, _target: &str) {}
}

impl<T: Loggable, E: std::error::Error> Loggable for Result<T, E> {
    fn log(self, target: &str) {
        match self {
            Ok(t) => t.log(target),
            Err(e) => log::error!(target: target, "{}", e),
        }
    }
}
