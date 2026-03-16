use std::time::Duration;

use crossterm::event::{self, Event as CrosstermEvent, KeyEvent};
use tokio::sync::mpsc;

use crate::models::message::Message;

/// Application events.
#[derive(Debug)]
pub enum AppEvent {
    /// Terminal key press
    Key(KeyEvent),
    /// Terminal resize
    Resize(u16, u16),
    /// Periodic tick for UI refresh
    Tick,
    /// D-Bus: device list changed
    DevicesChanged,
    /// D-Bus: new conversation appeared
    ConversationCreated(Message),
    /// D-Bus: existing conversation updated (new message)
    ConversationUpdated(Message),
    /// D-Bus: conversation removed
    ConversationRemoved(i64),
    /// Conversations finished loading from device
    ConversationsLoaded,
}

/// Spawns an event loop that listens for terminal events and periodic ticks.
///
/// Uses `spawn_blocking` because crossterm's `poll`/`read` are blocking I/O
/// that must not run on the async executor.
pub fn spawn_event_loop(tick_rate: Duration) -> mpsc::UnboundedReceiver<AppEvent> {
    let (tx, rx) = mpsc::unbounded_channel();

    tokio::task::spawn_blocking(move || {
        loop {
            let timeout = tick_rate;
            if event::poll(timeout).unwrap_or(false) {
                match event::read() {
                    Ok(CrosstermEvent::Key(key)) => {
                        if tx.send(AppEvent::Key(key)).is_err() {
                            break;
                        }
                    }
                    Ok(CrosstermEvent::Resize(w, h)) => {
                        if tx.send(AppEvent::Resize(w, h)).is_err() {
                            break;
                        }
                    }
                    _ => {}
                }
            } else if tx.send(AppEvent::Tick).is_err() {
                break;
            }
        }
    });

    rx
}

/// Returns the sender half for injecting D-Bus signal events into the event loop.
pub fn create_event_channel() -> (mpsc::UnboundedSender<AppEvent>, mpsc::UnboundedReceiver<AppEvent>) {
    mpsc::unbounded_channel()
}
