//! Bounded interception control shared by UI and proxy. No socket work on the UI thread.
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::{mpsc, oneshot, watch};

#[derive(Debug)]
pub enum Decision {
    Forward,
    Edit(Vec<u8>),
    Drop,
}
#[derive(Debug)]
pub struct Pending {
    pub id: u64,
    pub authority: String,
    pub destination: String,
    pub response: bool,
    pub request: Vec<u8>,
    pub decision: oneshot::Sender<Decision>,
    pub deadline: std::time::Instant,
}
#[derive(Clone, Debug)]
pub struct Interception {
    enabled: watch::Sender<bool>,
    pending: mpsc::Sender<Pending>,
    next_id: Arc<AtomicU64>,
    pub hold_timeout: Duration,
}
impl Interception {
    pub fn channel(capacity: usize, hold_timeout: Duration) -> (Self, mpsc::Receiver<Pending>) {
        let (pending, rx) = mpsc::channel(capacity);
        let (enabled, _) = watch::channel(false);
        (
            Self {
                enabled,
                pending,
                next_id: Arc::new(AtomicU64::new(1)),
                hold_timeout,
            },
            rx,
        )
    }
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.send_replace(enabled);
    }
    pub fn enabled(&self) -> bool {
        *self.enabled.borrow()
    }
    pub async fn pause(
        &self,
        authority: String,
        request: Vec<u8>,
    ) -> Result<Decision, &'static str> {
        let destination = format!("http://{authority}");
        self.pause_stage(authority, destination, request, false)
            .await
    }
    pub async fn pause_stage(
        &self,
        authority: String,
        destination: String,
        request: Vec<u8>,
        response: bool,
    ) -> Result<Decision, &'static str> {
        let mut enabled = self.enabled.subscribe();
        if !*enabled.borrow() {
            return Ok(Decision::Forward);
        }
        if request.len() > 128 * 1024 {
            return Err("intercept request exceeds 128 KiB editing limit");
        }
        let (decision, rx) = oneshot::channel();
        let pending = Pending {
            id: self.next_id.fetch_add(1, Ordering::Relaxed),
            authority,
            destination,
            response,
            request,
            decision,
            deadline: std::time::Instant::now() + self.hold_timeout,
        };
        let work = async {
            self.pending
                .send(pending)
                .await
                .map_err(|_| "intercept UI closed")?;
            rx.await.map_err(|_| "intercept request dropped by UI")
        };
        tokio::select! {
            biased;
            _ = enabled.changed() => Ok(Decision::Forward),
            result = tokio::time::timeout(self.hold_timeout, work) => result.map_err(|_| "intercept hold timeout")?,
        }
    }
}
