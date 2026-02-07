use crate::kernel::client::KernelMessage;
use crossterm::event::{Event as CrosstermEvent, EventStream, KeyEvent};
use futures::StreamExt;
use tokio::sync::mpsc;

/// Unified application event type.
///
/// Merges terminal input events with kernel messages
/// into a single stream for the main event loop.
#[derive(Debug)]
pub enum AppEvent {
    /// A key was pressed
    Key(KeyEvent),
    /// Terminal was resized
    Resize(u16, u16),
    /// A message arrived from the kernel
    Kernel(KernelMessage),
    /// Render tick (for periodic redraws if needed)
    Tick,
}

/// Runs the event collection loop, forwarding all events
/// to the provided sender.
pub async fn run_event_loop(
    tx: mpsc::UnboundedSender<AppEvent>,
    mut kernel_rx: mpsc::UnboundedReceiver<KernelMessage>,
) {
    let mut reader = EventStream::new();
    let mut tick_interval = tokio::time::interval(std::time::Duration::from_millis(100));

    loop {
        tokio::select! {
            // Terminal events (keyboard, resize, etc.)
            maybe_event = reader.next() => {
                match maybe_event {
                    Some(Ok(event)) => {
                        match event {
                            CrosstermEvent::Key(key) => {
                                if tx.send(AppEvent::Key(key)).is_err() {
                                    break;
                                }
                            }
                            CrosstermEvent::Resize(w, h) => {
                                if tx.send(AppEvent::Resize(w, h)).is_err() {
                                    break;
                                }
                            }
                            _ => {}
                        }
                    }
                    Some(Err(_)) => break,
                    None => break,
                }
            }
            // Kernel messages
            Some(kernel_msg) = kernel_rx.recv() => {
                if tx.send(AppEvent::Kernel(kernel_msg)).is_err() {
                    break;
                }
            }
            // Periodic tick for redraws
            _ = tick_interval.tick() => {
                if tx.send(AppEvent::Tick).is_err() {
                    break;
                }
            }
        }
    }
}
