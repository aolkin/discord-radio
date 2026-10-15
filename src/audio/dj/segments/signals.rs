use crate::audio::processing::mixer::TrackCallback;
use futures_util::future::BoxFuture;
use std::ops::Deref;
use std::time::Instant;
use tokio::sync::watch;

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
        Box::pin(tokio::time::sleep_until(tokio::time::Instant::from_std(
            instant,
        )))
        .into()
    }

    pub fn when(future: impl Future<Output = ()>) -> Self {
        Box::pin(future).into()
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
            // An Err means the segment dropped its sender, so it is finished.
            let _ = rx.wait_for(|done| *done).await;
        })
    }

    pub fn complete(&self) {
        self.0.send_replace(true);
    }
}

impl<T> From<CompletedSignal> for TrackCallback<T> {
    fn from(signal: CompletedSignal) -> Self {
        TrackCallback::new(move |_| signal.complete())
    }
}
