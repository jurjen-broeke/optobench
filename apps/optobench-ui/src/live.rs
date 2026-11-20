//! The window's live link (M12): a sidecar traced against the design open in SolidCraft,
//! again after each change, on a background thread (`optobench_bridge::live::Live`).
//! The window takes in each update as it takes in a trace, and reads the scene from the
//! update's STEP export so the view shows the parts where SolidCraft has them.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use serde_json::Value;

/// What the link thread sends.
#[derive(Debug)]
pub enum Message {
    /// A trace of the design as it is (the `live-trace` result).
    Update(Value),
    /// The link failed or ended, and why.
    Ended(String),
}

/// A running link.
pub struct Link {
    receiver: Receiver<Message>,
    stop: Arc<AtomicBool>,
    /// The revision traced last, for the status.
    pub revision: Option<u64>,
}

impl Link {
    /// Starts a link for the sidecar of `parameters` (`Live::start`'s); `repaint` is
    /// called whenever a message is sent.
    pub fn start(parameters: Value, repaint: crate::document::Repaint) -> Self {
        let (sender, receiver) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        std::thread::spawn(move || {
            let send = |m: Message| {
                let _ = sender.send(m);
                repaint();
            };
            let Some(parameters) = parameters.as_object() else {
                return send(Message::Ended("no parameters".into()));
            };
            let (mut live, first) = match optobench_bridge::live::Live::start(parameters) {
                Ok(started) => started,
                Err(e) => return send(Message::Ended(e.message)),
            };
            send(Message::Update(first));
            while !stopping.load(Ordering::Relaxed) {
                match live.next(Duration::from_millis(300)) {
                    Ok(Some(update)) => send(Message::Update(update)),
                    Ok(None) => {}
                    Err(e) => return send(Message::Ended(e.message)),
                }
            }
        });
        Self {
            receiver,
            stop,
            revision: None,
        }
    }

    /// The messages that arrived.
    pub fn poll(&mut self) -> Vec<Message> {
        self.receiver.try_iter().collect()
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
