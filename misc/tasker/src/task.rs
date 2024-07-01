use std::{future::Future, num::NonZeroUsize};

use crate::get_tokio_handle;

pub trait SyncTask: Send + Sized + 'static {
    type Output: Loggable;

    fn run(self) -> Self::Output;
    fn spawn(self) {
        std::thread::spawn(move || {
            self.run().log();
        });
    }
}

pub trait AsyncTask: Sized {
    type Output: Loggable;

    fn run(self) -> impl Future<Output = Self::Output> + Send + 'static;
    fn spawn(self) {
        let fut = self.run();
        get_tokio_handle().spawn(async move {
            fut.await.log();
        });
    }
}

pub trait BlockingAsyncTask: Send + Sized + 'static {
    type Output: Loggable;

    fn parallelism(&self) -> NonZeroUsize {
        NonZeroUsize::new(1).unwrap()
    }
    fn run(self) -> impl Future<Output = Self::Output> + Send + 'static;
}

impl<T: BlockingAsyncTask> SyncTask for T {
    type Output = <T as BlockingAsyncTask>::Output;

    fn run(self) -> Self::Output {
        let parallelism = self.parallelism().get();
        let mut builder = if parallelism == 1 {
            tokio::runtime::Builder::new_current_thread()
        } else {
            let mut builder = tokio::runtime::Builder::new_multi_thread();
            builder.worker_threads(parallelism);
            builder
        };

        let runtime = builder.enable_all().build().unwrap();
        runtime.block_on(async move { BlockingAsyncTask::run(self).await })
    }
}

pub trait Loggable {
    fn log(&self);
}

impl Loggable for () {
    fn log(&self) {}
}

impl Loggable for ! {
    fn log(&self) {}
}

impl<T: Loggable, E: std::error::Error> Loggable for Result<T, E> {
    fn log(&self) {
        match self {
            Ok(t) => t.log(),
            Err(e) => log::error!("{}", e),
        }
    }
}
