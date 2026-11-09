use crate::audio::processing::mixer::TrackCallback;
use futures_util::future::BoxFuture;
use std::ops::Deref;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{Notify, watch};

pub struct Completed(BoxFuture<'static, ()>);

impl Deref for Completed {
    type Target = BoxFuture<'static, ()>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl IntoFuture for Completed {
    type Output = ();
    type IntoFuture = BoxFuture<'static, ()>;

    fn into_future(self) -> Self::IntoFuture {
        self.0
    }
}

impl From<BoxFuture<'static, ()>> for Completed {
    fn from(future: BoxFuture<'static, ()>) -> Self {
        Self(future)
    }
}

impl Completed {
    pub fn at(instant: Instant) -> Self {
        Self(Box::pin(tokio::time::sleep_until(
            tokio::time::Instant::from_std(instant),
        )))
    }

    pub fn when(future: impl Future<Output = ()> + Send + 'static) -> Self {
        Self(Box::pin(future))
    }

    pub fn never() -> Self {
        Self(Box::pin(std::future::pending()))
    }
}

#[derive(Debug, Clone)]
pub struct CompletedSignal(watch::Sender<bool>);

impl Default for CompletedSignal {
    fn default() -> Self {
        Self(watch::channel(false).0)
    }
}

impl CompletedSignal {
    pub fn is_completed(&self) -> bool {
        *self.0.borrow()
    }

    pub fn completed(&self) -> Completed {
        let mut rx = self.0.subscribe();
        Completed::when(async move {
            let _ = rx.wait_for(|done| *done).await;
        })
    }

    pub fn complete(&self) {
        self.0.send_replace(true);
    }
}

impl Drop for CompletedSignal {
    fn drop(&mut self) {
        self.complete();
    }
}

#[derive(Debug, Clone, Default)]
pub struct ChangedSignal(Arc<Notify>);

impl ChangedSignal {
    pub fn changed(&self) -> Completed {
        let notify = self.0.clone();
        Completed::when(async move { notify.notified().await })
    }

    pub fn notify(&self) {
        self.0.notify_one();
    }
}

impl<T> From<CompletedSignal> for TrackCallback<T> {
    fn from(signal: CompletedSignal) -> Self {
        TrackCallback::new(move |_| signal.complete())
    }
}
