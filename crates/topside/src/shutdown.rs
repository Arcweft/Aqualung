use std::io;

use tokio::sync::watch;

#[derive(Clone)]
pub struct Shutdown {
    receiver: watch::Receiver<bool>,
}

impl Shutdown {
    pub fn manual() -> (Self, impl Fn() + Clone + Send + Sync + 'static) {
        let (sender, receiver) = watch::channel(false);
        let stop = move || {
            sender.send_replace(true);
        };
        (Self { receiver }, stop)
    }

    pub fn on_signals() -> io::Result<Self> {
        let mut interrupt =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        let (sender, receiver) = watch::channel(false);
        tokio::spawn(async move {
            tokio::select! {
                _ = interrupt.recv() => {}
                _ = terminate.recv() => {}
            }
            sender.send_replace(true);
        });
        Ok(Self { receiver })
    }

    pub(crate) fn is_set(&self) -> bool {
        *self.receiver.borrow()
    }

    pub(crate) async fn wait(&mut self) {
        while !self.is_set() {
            if self.receiver.changed().await.is_err() {
                return;
            }
        }
    }
}
