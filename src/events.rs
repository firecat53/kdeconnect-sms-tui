use std::time::Duration;

use crossterm::event::{self, Event as CrosstermEvent, KeyEvent};
use tokio::sync::mpsc;

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
}

/// Spawns an event loop that listens for terminal events and periodic ticks.
pub fn spawn_event_loop(tick_rate: Duration) -> mpsc::UnboundedReceiver<AppEvent> {
    let (tx, rx) = mpsc::unbounded_channel();

    tokio::spawn(async move {
        loop {
            // Poll for crossterm events with timeout
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
            } else {
                // Tick on timeout
                if tx.send(AppEvent::Tick).is_err() {
                    break;
                }
            }
        }
    });

    rx
}
