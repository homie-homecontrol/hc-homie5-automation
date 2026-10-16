//! Containment of panics raised while running automation logic.
//!
//! Rules, Lua scripts, virtual devices, and configuration come from users, so a defect they
//! trigger must not take the controller down. Code wrapped in [`contain`] or [`contain_sync`]
//! turns a panic into a [`PanicError`]. While such code runs, [`panic_is_contained`] is true on
//! the running thread; the process panic hook uses it to log the panic instead of exiting.

use std::{
    any::Any,
    cell::Cell,
    fmt,
    future::Future,
    panic::{catch_unwind, AssertUnwindSafe},
    pin::Pin,
    task::{Context, Poll},
};

thread_local! {
    static CONTAINED_DEPTH: Cell<usize> = const { Cell::new(0) };
}

/// Returns true while the current thread executes code inside [`contain`] or [`contain_sync`].
pub fn panic_is_contained() -> bool {
    CONTAINED_DEPTH.with(|depth| depth.get() > 0)
}

struct ContainedScope;

impl ContainedScope {
    fn enter() -> Self {
        CONTAINED_DEPTH.with(|depth| depth.set(depth.get() + 1));
        ContainedScope
    }
}

impl Drop for ContainedScope {
    fn drop(&mut self) {
        CONTAINED_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

/// A panic caught by [`contain`] or [`contain_sync`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanicError {
    pub message: String,
}

impl fmt::Display for PanicError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "panic: {}", self.message)
    }
}

impl std::error::Error for PanicError {}

fn panic_message(payload: Box<dyn Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "unknown panic payload".to_owned()
    }
}

/// Runs `f` and converts a panic into a [`PanicError`].
pub fn contain_sync<T>(f: impl FnOnce() -> T) -> Result<T, PanicError> {
    let _scope = ContainedScope::enter();
    catch_unwind(AssertUnwindSafe(f)).map_err(|payload| PanicError {
        message: panic_message(payload),
    })
}

/// Future returned by [`contain`].
pub struct Contained<F> {
    inner: Option<Pin<Box<F>>>,
}

/// Wraps `future` so that a panic while polling it resolves to `Err(PanicError)`.
///
/// After a panic the inner future is dropped and never polled again.
pub fn contain<F: Future>(future: F) -> Contained<F> {
    Contained {
        inner: Some(Box::pin(future)),
    }
}

impl<F: Future> Future for Contained<F> {
    type Output = Result<F::Output, PanicError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let Some(inner) = self.inner.as_mut() else {
            return Poll::Ready(Err(PanicError {
                message: "contained future polled after it panicked".to_owned(),
            }));
        };
        match contain_sync(|| inner.as_mut().poll(cx)) {
            Ok(Poll::Ready(value)) => Poll::Ready(Ok(value)),
            Ok(Poll::Pending) => Poll::Pending,
            Err(err) => {
                self.inner = None;
                Poll::Ready(Err(err))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contain_sync_returns_value() {
        assert_eq!(contain_sync(|| 41 + 1), Ok(42));
    }

    #[test]
    fn contain_sync_converts_panic_with_message() {
        let err = contain_sync(|| -> u32 { panic!("boom {}", 7) }).unwrap_err();
        assert_eq!(err.message, "boom 7");
    }

    #[test]
    fn contained_flag_is_scoped() {
        assert!(!panic_is_contained());
        contain_sync(|| {
            assert!(panic_is_contained());
            contain_sync(|| assert!(panic_is_contained())).unwrap();
            assert!(panic_is_contained());
        })
        .unwrap();
        assert!(!panic_is_contained());
        let _ = contain_sync(|| panic!("unwinds through the scope"));
        assert!(!panic_is_contained());
    }

    #[tokio::test]
    async fn contain_converts_panic_across_await_points() {
        let err = contain(async {
            tokio::task::yield_now().await;
            assert!(panic_is_contained());
            if true {
                panic!("async boom");
            }
            1
        })
        .await
        .unwrap_err();
        assert_eq!(err.message, "async boom");
        assert!(!panic_is_contained());
    }

    #[tokio::test]
    async fn contain_passes_through_output() {
        let value = contain(async {
            tokio::task::yield_now().await;
            "done"
        })
        .await;
        assert_eq!(value, Ok("done"));
    }

    #[tokio::test]
    async fn contain_is_send_for_spawned_tasks() {
        let handle = tokio::spawn(contain(async { panic!("in a spawned task") }));
        let result: Result<(), PanicError> = handle.await.expect("task itself must not panic");
        assert_eq!(result.unwrap_err().message, "in a spawned task");
    }
}
