use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time::{sleep_until, Instant};

/// Trailing-edge debouncer with a maximum wait.
///
/// Delivers the latest event once no new event arrived for `debounce`, but at the latest
/// `max_wait` after the first event of a burst, so continuous updates cannot starve delivery.
#[derive(Debug, Clone)]
pub struct MaxWaitDebouncer<T>
where
    T: Send + 'static,
{
    tx: mpsc::Sender<T>,
}

impl<T> MaxWaitDebouncer<T>
where
    T: Send + 'static,
{
    pub fn new(debounce: Duration, max_wait: Duration, target: mpsc::Sender<T>) -> Self {
        let (tx, mut rx) = mpsc::channel::<T>(100);
        tokio::spawn(async move {
            while let Some(first) = rx.recv().await {
                let mut pending = first;
                let burst_deadline = Instant::now() + max_wait;
                let mut deadline = (Instant::now() + debounce).min(burst_deadline);
                loop {
                    tokio::select! {
                        next = rx.recv() => match next {
                            Some(event) => {
                                pending = event;
                                deadline = (Instant::now() + debounce).min(burst_deadline);
                            }
                            None => return,
                        },
                        _ = sleep_until(deadline) => break,
                    }
                }
                if target.send(pending).await.is_err() {
                    tracing::debug!("cannot send debounced event; receiver closed");
                }
            }
        });
        Self { tx }
    }

    pub async fn send(&self, event: T) {
        let _ = self.tx.send(event).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn delivers_after_quiet_period() {
        let (target, mut rx) = mpsc::channel(10);
        let debouncer = MaxWaitDebouncer::new(Duration::from_millis(200), Duration::from_secs(1), target);

        debouncer.send(1).await;
        debouncer.send(2).await;
        tokio::time::sleep(Duration::from_millis(199)).await;
        assert!(rx.try_recv().is_err());
        tokio::time::sleep(Duration::from_millis(2)).await;
        assert_eq!(rx.try_recv().ok(), Some(2));
    }

    #[tokio::test(start_paused = true)]
    async fn continuous_updates_deliver_at_least_once_per_max_wait() {
        let (target, mut rx) = mpsc::channel(10);
        let debouncer = MaxWaitDebouncer::new(Duration::from_millis(200), Duration::from_secs(1), target);

        let mut delivered = Vec::new();
        // a member updating every 100 ms for 2.5 s never leaves a 200 ms quiet period
        for i in 0..25 {
            debouncer.send(i).await;
            tokio::time::sleep(Duration::from_millis(100)).await;
            while let Ok(v) = rx.try_recv() {
                delivered.push(v);
            }
        }

        assert!(delivered.len() >= 2, "{delivered:?}");
    }
}
