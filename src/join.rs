#[cfg(feature = "driver-core")]
use crate::error::ConnectionResult;
use crate::{
    error::{JoinError, JoinResult},
    ConnectionInfo,
};
use core::{
    convert,
    future::Future,
    marker::Unpin,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use flume::r#async::RecvFut;
use pin_project::pin_project;
use tokio::time::{self, Timeout};

#[cfg(feature = "driver-core")]
/// TODO
#[pin_project]
pub struct Join {
    #[pin]
    inner: JoinClass<ConnectionResult<()>>,
}

#[cfg(feature = "driver-core")]
impl Join {
    pub(crate) fn new(
        recv: RecvFut<'static, ConnectionResult<()>>,
        timeout: Option<Duration>,
    ) -> Self {
        Self {
            inner: JoinClass::new(recv, timeout),
        }
    }
}

#[cfg(feature = "driver-core")]
impl Future for Join {
    type Output = JoinResult<()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.project()
            .inner
            .poll(cx)
            .map_ok(|inner_res| inner_res.map_err(JoinError::Driver))
            .map(|m| m.and_then(convert::identity))
    }
}

/// TODO
#[pin_project]
pub struct JoinGateway {
    #[pin]
    inner: JoinClass<ConnectionInfo>,
}

impl JoinGateway {
    pub(crate) fn new(recv: RecvFut<'static, ConnectionInfo>, timeout: Option<Duration>) -> Self {
        Self {
            inner: JoinClass::new(recv, timeout),
        }
    }
}

impl Future for JoinGateway {
    type Output = JoinResult<ConnectionInfo>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.project().inner.poll(cx)
    }
}

#[pin_project(project = JoinClassProj)]
enum JoinClass<T: 'static> {
    WithTimeout(#[pin] Timeout<RecvFut<'static, T>>),
    Vanilla(RecvFut<'static, T>),
}

impl<T: 'static> JoinClass<T> {
    pub(crate) fn new(recv: RecvFut<'static, T>, timeout: Option<Duration>) -> Self {
        match timeout {
            Some(t) => JoinClass::WithTimeout(time::timeout(t, recv)),
            None => JoinClass::Vanilla(recv),
        }
    }
}

impl<T> Future for JoinClass<T>
where
    T: Unpin,
{
    type Output = JoinResult<T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project() {
            JoinClassProj::WithTimeout(t) => t
                .poll(cx)
                .map_err(|_| JoinError::TimedOut)
                .map_ok(|res| res.map_err(|_| JoinError::Dropped))
                .map(|m| m.and_then(convert::identity)),
            JoinClassProj::Vanilla(t) => Pin::new(t).poll(cx).map_err(|_| JoinError::Dropped),
        }
    }
}
