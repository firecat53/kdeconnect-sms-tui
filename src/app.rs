use std::io;
use std::time::Duration;

use color_eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::execute;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tracing::info;

use crate::config::Config;
use crate::contacts::ContactStore;
use crate::dbus::daemon::DaemonClient;
use crate::events::{self, AppEvent};
use crate::models::conversation::Conversation;
use crate::models::device::Device;

pub struct App {
    pub config: Config,
    pub devices: Vec<Device>,
    pub selected_device_idx: Option<usize>,
    pub conversations: Vec<Conversation>,
    pub selected_conversation_idx: Option<usize>,
    pub contacts: ContactStore,
    pub message_scroll: u16,
    pub should_quit: bool,
    daemon: Option<DaemonClient>,
}

impl App {
    pub async fn new(
        config: Config,
        device_id: Option<String>,
        device_name: Option<String>,
    ) -> Result<Self> {
        let contacts = ContactStore::load().unwrap_or_else(|e| {
            tracing::warn!("Failed to load contacts: {}", e);
            ContactStore::load_from_dir(std::path::Path::new("/dev/null"))
                .unwrap_or_else(|_| ContactStore::load_from_dir(std::path::Path::new(".")).unwrap())
        });
        info!("Loaded {} contacts", contacts.len());

        let daemon = match DaemonClient::new().await {
            Ok(d) => Some(d),
            Err(e) => {
                tracing::warn!("Failed to connect to kdeconnect daemon: {}", e);
                None
            }
        };

        let mut app = Self {
            config,
            devices: Vec::new(),
            selected_device_idx: None,
            conversations: Vec::new(),
            selected_conversation_idx: None,
            contacts,
            message_scroll: 0,
            should_quit: false,
            daemon,
        };

        app.refresh_devices().await;

        // Resolve initial device
        if let Some(ref daemon) = app.daemon {
            let resolved = daemon
                .resolve_device(device_id.as_deref(), device_name.as_deref())
                .await?;
            if let Some(dev) = resolved {
                app.selected_device_idx = app.devices.iter().position(|d| d.id == dev.id);
            }
        }

        Ok(app)
    }

    /// Create a minimal App for testing (no D-Bus).
    #[cfg(test)]
    pub fn new_test() -> Self {
        Self {
            config: Config::default(),
            devices: Vec::new(),
            selected_device_idx: None,
            conversations: Vec::new(),
            selected_conversation_idx: None,
            contacts: ContactStore::load_from_dir(std::path::Path::new("/nonexistent"))
                .unwrap_or_else(|_| {
                    ContactStore::load_from_dir(&std::env::temp_dir()).unwrap()
                }),
            message_scroll: 0,
            should_quit: false,
            daemon: None,
        }
    }

    async fn refresh_devices(&mut self) {
        if let Some(ref daemon) = self.daemon {
            match daemon.discover_devices().await {
                Ok(devices) => {
                    self.devices = devices;
                    if self.selected_device_idx.is_none() && !self.devices.is_empty() {
                        self.selected_device_idx = Some(0);
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to discover devices: {}", e);
                }
            }
        }
    }

    pub async fn run(&mut self) -> Result<()> {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let mut events = events::spawn_event_loop(Duration::from_millis(250));

        // Main loop
        while !self.should_quit {
            terminal.draw(|f| {
                crate::ui::draw(f, self);
            })?;

            if let Some(event) = events.recv().await {
                self.handle_event(event).await;
            }
        }

        // Restore terminal
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;

        Ok(())
    }

    async fn handle_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::Key(key) => self.handle_key(key).await,
            AppEvent::Resize(_, _) => {} // ratatui handles resize automatically
            AppEvent::Tick => {}          // future: refresh stale data
            AppEvent::DevicesChanged => {
                self.refresh_devices().await;
            }
        }
    }

    async fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            // Quit
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }

            // Device switching
            KeyCode::Tab => self.cycle_device(),

            // Conversation navigation
            KeyCode::Up | KeyCode::Char('k') => self.select_prev_conversation(),
            KeyCode::Down | KeyCode::Char('j') => self.select_next_conversation(),

            // Message scrolling
            KeyCode::PageUp => {
                self.message_scroll = self.message_scroll.saturating_sub(10);
            }
            KeyCode::PageDown => {
                self.message_scroll = self.message_scroll.saturating_add(10);
            }

            _ => {}
        }
    }

    fn cycle_device(&mut self) {
        if self.devices.is_empty() {
            return;
        }
        let next = match self.selected_device_idx {
            Some(i) => (i + 1) % self.devices.len(),
            None => 0,
        };
        self.selected_device_idx = Some(next);
        // Reset conversation state when switching devices
        self.conversations.clear();
        self.selected_conversation_idx = None;
        self.message_scroll = 0;
    }

    fn select_prev_conversation(&mut self) {
        if self.conversations.is_empty() {
            return;
        }
        let new_idx = match self.selected_conversation_idx {
            Some(0) | None => 0,
            Some(i) => i - 1,
        };
        self.selected_conversation_idx = Some(new_idx);
        self.message_scroll = 0;
    }

    fn select_next_conversation(&mut self) {
        if self.conversations.is_empty() {
            return;
        }
        let max = self.conversations.len().saturating_sub(1);
        let new_idx = match self.selected_conversation_idx {
            None => 0,
            Some(i) => (i + 1).min(max),
        };
        self.selected_conversation_idx = Some(new_idx);
        self.message_scroll = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::device::Device;

    #[test]
    fn test_cycle_device_empty() {
        let mut app = App::new_test();
        app.cycle_device();
        assert_eq!(app.selected_device_idx, None);
    }

    #[test]
    fn test_cycle_device() {
        let mut app = App::new_test();
        app.devices = vec![
            Device {
                id: "a".into(),
                name: "Phone A".into(),
                reachable: true,
                paired: true,
            },
            Device {
                id: "b".into(),
                name: "Phone B".into(),
                reachable: true,
                paired: true,
            },
        ];
        app.selected_device_idx = Some(0);

        app.cycle_device();
        assert_eq!(app.selected_device_idx, Some(1));

        app.cycle_device();
        assert_eq!(app.selected_device_idx, Some(0));
    }

    #[test]
    fn test_conversation_navigation() {
        let mut app = App::new_test();
        app.conversations = vec![
            Conversation::new(1),
            Conversation::new(2),
            Conversation::new(3),
        ];

        // Start with nothing selected, go down
        app.select_next_conversation();
        assert_eq!(app.selected_conversation_idx, Some(0));

        app.select_next_conversation();
        assert_eq!(app.selected_conversation_idx, Some(1));

        app.select_next_conversation();
        assert_eq!(app.selected_conversation_idx, Some(2));

        // Can't go past the end
        app.select_next_conversation();
        assert_eq!(app.selected_conversation_idx, Some(2));

        // Go up
        app.select_prev_conversation();
        assert_eq!(app.selected_conversation_idx, Some(1));

        app.select_prev_conversation();
        assert_eq!(app.selected_conversation_idx, Some(0));

        // Can't go past the beginning
        app.select_prev_conversation();
        assert_eq!(app.selected_conversation_idx, Some(0));
    }

    #[test]
    fn test_cycle_device_clears_conversations() {
        let mut app = App::new_test();
        app.devices = vec![
            Device {
                id: "a".into(),
                name: "A".into(),
                reachable: true,
                paired: true,
            },
            Device {
                id: "b".into(),
                name: "B".into(),
                reachable: true,
                paired: true,
            },
        ];
        app.selected_device_idx = Some(0);
        app.conversations = vec![Conversation::new(1)];
        app.selected_conversation_idx = Some(0);

        app.cycle_device();
        assert!(app.conversations.is_empty());
        assert_eq!(app.selected_conversation_idx, None);
    }
}
