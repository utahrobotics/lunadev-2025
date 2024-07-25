use std::future::Future;

use tokio::runtime::Handle;

use crate::get_tokio_handle;

pub trait SyncTask: Send + Sized + 'static {
    type Output;

    fn run(self) -> Self::Output;
    fn spawn(self)
    where
        Self::Output: Loggable,
    {
        self.spawn_with(|x| x.log())
    }
    fn spawn_with(self, f: impl FnOnce(Self::Output) + Send + 'static) {
        std::thread::spawn(move || {
            f(self.run());
        });
    }
}

pub trait AsyncTask: Sized {
    type Output;

    fn run(self) -> impl Future<Output = Self::Output> + Send + 'static;
    fn spawn(self)
    where
        Self::Output: Loggable,
    {
        self.spawn_with(|x| x.log())
    }
    fn spawn_with(self, f: impl FnOnce(Self::Output) + Send + 'static) {
        let fut = self.run();
        Handle::try_current()
            .unwrap_or_else(|_| get_tokio_handle())
            .spawn(async move {
                let output = fut.await;
                f(output);
            });
    }
}

// pub trait BlockingAsyncTask: Send + Sized + 'static {
//     type Output: Loggable;

//     fn parallelism(&self) -> NonZeroUsize {
//         NonZeroUsize::new(1).unwrap()
//     }
//     fn run(self) -> impl Future<Output = Self::Output> + Send + 'static;
//     fn spawn(self) {
//         std::thread::spawn(move || {
//             let parallelism = self.parallelism().get();
//             let mut builder = if parallelism == 1 {
//                 tokio::runtime::Builder::new_current_thread()
//             } else {
//                 let mut builder = tokio::runtime::Builder::new_multi_thread();
//                 builder.worker_threads(parallelism);
//                 builder
//             };

//             let runtime = builder.enable_all().build().unwrap();
//             runtime.block_on(self.run()).log();
//         });
//     }
// }

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

impl<T: Loggable, F: FnOnce() -> T + Send + 'static> SyncTask for F {
    type Output = T;

    fn run(self) -> T {
        self()
    }
}

impl<F: FnOnce() -> Fut, Fut: Future<Output: Loggable> + Send + 'static> AsyncTask for F {
    type Output = Fut::Output;

    fn run(self) -> impl Future<Output = Self::Output> + Send + 'static {
        self()
    }
}
