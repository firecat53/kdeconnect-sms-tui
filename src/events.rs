use crossterm::event::{Event as CrosstermEvent, EventStream, KeyEvent, KeyEventKind};
use futures_lite::StreamExt;
use tokio::sync::mpsc;

use crate::models::message::Message;

/// Application events.
#[derive(Debug)]
pub enum AppEvent {
    /// Terminal key press
    Key(KeyEvent),
    /// Bracketed paste (multiline text pasted in one go)
    Paste(String),
    /// Terminal resize
    Resize,
    /// Periodic tick for UI refresh
    Tick,
    /// D-Bus: new conversation appeared
    ConversationCreated(Message),
    /// D-Bus: existing conversation updated (new message)
    ConversationUpdated(Message),
    /// D-Bus: conversation removed
    ConversationRemoved(i64),
    /// D-Bus: conversation messages finished loading (thread_id, message_count)
    ConversationLoaded(i64, u64),
    /// D-Bus: attachment file received (file_path, file_name)
    AttachmentReceived(String, String),
}

/// Spawns an async event loop using crossterm's EventStream.
///
/// Uses crossterm's `event-stream` feature for proper async integration
/// with tokio, which works correctly across terminal multiplexers like tmux.
pub fn spawn_event_loop(tick_rate: std::time::Duration) -> mpsc::UnboundedReceiver<AppEvent> {
    let (tx, rx) = mpsc::unbounded_channel();

    tokio::spawn(async move {
        let mut reader = EventStream::new();
        let mut tick_interval = tokio::time::interval(tick_rate);

        loop {
            tokio::select! {
                maybe_event = reader.next() => {
                    match maybe_event {
                        Some(Ok(CrosstermEvent::Key(key))) if key.kind == KeyEventKind::Press => {
                            if tx.send(AppEvent::Key(key)).is_err() {
                                break;
                            }
                        }
                        Some(Ok(CrosstermEvent::Resize(_, _))) => {
                            if tx.send(AppEvent::Resize).is_err() {
                                break;
                            }
                        }
                        Some(Ok(CrosstermEvent::Paste(text))) => {
                            if tx.send(AppEvent::Paste(text)).is_err() {
                                break;
                            }
                        }
                        Some(Ok(_)) => {} // ignore other events
                        Some(Err(_)) => break,
                        None => break, // stream ended
                    }
                }
                _ = tick_interval.tick() => {
                    if tx.send(AppEvent::Tick).is_err() {
                        break;
                    }
                }
            }
        }
    });

    rx
}

/// Returns the sender half for injecting D-Bus signal events into the event loop.
pub fn create_event_channel() -> (
    mpsc::UnboundedSender<AppEvent>,
    mpsc::UnboundedReceiver<AppEvent>,
) {
    mpsc::unbounded_channel()
}
