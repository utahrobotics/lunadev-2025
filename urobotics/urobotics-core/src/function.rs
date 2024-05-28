use std::future::Future;

use serde::de::DeserializeOwned;
use tokio::task::AbortHandle;

use crate::runtime::RuntimeContext;

pub trait FunctionConfig: DeserializeOwned {
    type Output;
    const PERSISTENT: bool;
    const NAME: &'static str;
    const DESCRIPTION: &'static str = "";

    fn standalone(self, _value: bool) -> Self {
        self
    }

    fn run(self, context: &RuntimeContext) -> Self::Output;
    fn spawn(self, context: RuntimeContext) where Self: Send + 'static {
        if Self::PERSISTENT {
            context.clone().spawn_persistent_sync(move || {
                self.run(&context);
            });
        } else {
            std::thread::spawn(move || {
                self.run(&context);
            });
        }
    }
}

pub trait AsyncFunctionConfig: DeserializeOwned {
    type Output;
    const NAME: &'static str;
    const DESCRIPTION: &'static str = "";

    fn standalone(self, _value: bool) -> Self {
        self
    }

    fn run(self, context: &RuntimeContext) -> impl Future<Output = Self::Output>;
}


pub trait SendAsyncFunctionConfig: AsyncFunctionConfig {
    const PERSISTENT: bool;

    fn run_send(self, context: &RuntimeContext) -> impl Future<Output = Self::Output> + Send;
    fn spawn(self, context: RuntimeContext) -> AbortHandle where Self: Send + 'static {
        if Self::PERSISTENT {
            context.clone().spawn_persistent_async(async move {
                self.run_send(&context).await;
            })
        } else {
            context.clone().spawn_async(async move {
                self.run_send(&context).await;
            }).abort_handle()
        }
    }
}