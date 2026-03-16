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
use crate::dbus::conversations::ConversationsClient;
use crate::dbus::daemon::DaemonClient;
use crate::dbus::signals;
use crate::events::{self, AppEvent};
use crate::models::conversation::{sort_by_recent, Conversation};
use crate::models::device::Device;
use crate::models::message::Message;

/// Loading state for async operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadingState {
    Idle,
    Loading,
    Error(String),
}

pub struct App {
    pub config: Config,
    pub devices: Vec<Device>,
    pub selected_device_idx: Option<usize>,
    pub conversations: Vec<Conversation>,
    pub selected_conversation_idx: Option<usize>,
    pub contacts: ContactStore,
    pub message_scroll: u16,
    pub should_quit: bool,
    pub loading: LoadingState,
    pub status_message: Option<String>,
    daemon: Option<DaemonClient>,
    conversations_client: Option<ConversationsClient>,
    /// Sender for injecting D-Bus signal events
    signal_tx: Option<tokio::sync::mpsc::UnboundedSender<AppEvent>>,
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
            loading: LoadingState::Idle,
            status_message: None,
            daemon,
            conversations_client: None,
            signal_tx: None,
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
            loading: LoadingState::Idle,
            status_message: None,
            daemon: None,
            conversations_client: None,
            signal_tx: None,
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

    /// Set up conversations client and signal listener for the selected device.
    async fn connect_to_device(&mut self, signal_tx: tokio::sync::mpsc::UnboundedSender<AppEvent>) {
        let device = self.selected_device().cloned();
        let Some(device) = device else {
            self.status_message = Some("No device selected".into());
            return;
        };

        if !device.is_available() {
            self.status_message = Some(format!("{} is not reachable", device.name));
            return;
        }

        let Some(ref daemon) = self.daemon else {
            return;
        };

        let client = ConversationsClient::new(
            daemon.connection().clone(),
            device.id.clone(),
        );

        // Start signal listener for this device
        signals::spawn_signal_listener(
            daemon.connection().clone(),
            device.id.clone(),
            signal_tx,
        );

        self.conversations_client = Some(client);
        self.status_message = Some(format!("Connected to {}", device.name));

        // Trigger initial conversation load
        self.load_conversations().await;
    }

    /// Load conversations from the currently connected device.
    async fn load_conversations(&mut self) {
        let Some(ref client) = self.conversations_client else {
            return;
        };

        self.loading = LoadingState::Loading;
        self.status_message = Some("Loading conversations...".into());

        // First request the phone to send all threads
        if let Err(e) = client.request_all_conversation_threads().await {
            self.loading = LoadingState::Error(format!("Failed to request threads: {}", e));
            self.status_message = Some(format!("Error: {}", e));
            return;
        }

        // Then fetch what's cached
        match client.active_conversations().await {
            Ok(convos) => {
                self.conversations = convos;
                self.loading = LoadingState::Idle;
                let count = self.conversations.len();
                self.status_message = Some(format!("{} conversations loaded", count));

                // Auto-select first if none selected
                if self.selected_conversation_idx.is_none() && !self.conversations.is_empty() {
                    self.selected_conversation_idx = Some(0);
                }
            }
            Err(e) => {
                self.loading = LoadingState::Error(format!("Failed to load: {}", e));
                self.status_message = Some(format!("Error: {}", e));
            }
        }
    }

    pub fn selected_device(&self) -> Option<&Device> {
        self.selected_device_idx.and_then(|i| self.devices.get(i))
    }

    pub async fn run(&mut self) -> Result<()> {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        // Create event channels
        let mut term_events = events::spawn_event_loop(Duration::from_millis(250));
        let (signal_tx, mut signal_events) = events::create_event_channel();
        self.signal_tx = Some(signal_tx.clone());

        // Connect to initially selected device
        self.connect_to_device(signal_tx.clone()).await;

        // Main loop
        while !self.should_quit {
            terminal.draw(|f| {
                crate::ui::draw(f, self);
            })?;

            // Wait for either terminal or D-Bus signal events
            tokio::select! {
                Some(event) = term_events.recv() => {
                    self.handle_event(event, signal_tx.clone()).await;
                }
                Some(event) = signal_events.recv() => {
                    self.handle_event(event, signal_tx.clone()).await;
                }
            }
        }

        // Restore terminal
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;

        Ok(())
    }

    async fn handle_event(
        &mut self,
        event: AppEvent,
        signal_tx: tokio::sync::mpsc::UnboundedSender<AppEvent>,
    ) {
        match event {
            AppEvent::Key(key) => self.handle_key(key, signal_tx).await,
            AppEvent::Resize(_, _) => {}
            AppEvent::Tick => {}
            AppEvent::DevicesChanged => {
                self.refresh_devices().await;
            }
            AppEvent::ConversationCreated(msg) => {
                self.handle_conversation_created(msg);
            }
            AppEvent::ConversationUpdated(msg) => {
                self.handle_conversation_updated(msg);
            }
            AppEvent::ConversationRemoved(thread_id) => {
                self.handle_conversation_removed(thread_id);
            }
            AppEvent::ConversationsLoaded => {
                // Re-fetch conversations after phone finishes sending data
                self.load_conversations().await;
            }
        }
    }

    /// Handle a new conversation appearing via D-Bus signal.
    fn handle_conversation_created(&mut self, msg: Message) {
        let thread_id = msg.thread_id;

        // Check if we already have this thread
        if let Some(conv) = self.conversations.iter_mut().find(|c| c.thread_id == thread_id) {
            conv.is_group = msg.is_group();
            let is_newer = conv
                .latest_message
                .as_ref()
                .is_none_or(|existing| msg.date > existing.date);
            if is_newer {
                conv.latest_message = Some(msg);
            }
        } else {
            let mut conv = Conversation::new(thread_id);
            conv.is_group = msg.is_group();
            conv.latest_message = Some(msg);
            self.conversations.push(conv);
        }

        sort_by_recent(&mut self.conversations);
    }

    /// Handle a conversation update (new message in existing thread).
    fn handle_conversation_updated(&mut self, msg: Message) {
        let thread_id = msg.thread_id;

        if let Some(conv) = self.conversations.iter_mut().find(|c| c.thread_id == thread_id) {
            let is_newer = conv
                .latest_message
                .as_ref()
                .is_none_or(|existing| msg.date > existing.date);
            if is_newer {
                conv.latest_message = Some(msg);
            }
        } else {
            // New thread we didn't know about
            self.handle_conversation_created(msg);
            return;
        }

        sort_by_recent(&mut self.conversations);
    }

    /// Handle a conversation being removed.
    fn handle_conversation_removed(&mut self, thread_id: i64) {
        let prev_len = self.conversations.len();
        self.conversations.retain(|c| c.thread_id != thread_id);

        if self.conversations.len() != prev_len {
            // Adjust selection
            if let Some(idx) = self.selected_conversation_idx {
                if idx >= self.conversations.len() {
                    self.selected_conversation_idx = if self.conversations.is_empty() {
                        None
                    } else {
                        Some(self.conversations.len() - 1)
                    };
                }
            }
        }
    }

    async fn handle_key(
        &mut self,
        key: KeyEvent,
        signal_tx: tokio::sync::mpsc::UnboundedSender<AppEvent>,
    ) {
        match key.code {
            // Quit
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }

            // Device switching
            KeyCode::Tab => {
                self.cycle_device();
                self.connect_to_device(signal_tx).await;
            }

            // Conversation navigation
            KeyCode::Up | KeyCode::Char('k') => self.select_prev_conversation(),
            KeyCode::Down | KeyCode::Char('j') => self.select_next_conversation(),

            // Refresh conversations
            KeyCode::Char('r') => {
                self.load_conversations().await;
            }

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
        self.conversations_client = None;
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
    use crate::models::message::{Address, MessageType};

    fn make_test_message(thread_id: i64, date: i64, body: &str) -> Message {
        Message {
            event: 0x1,
            body: body.into(),
            addresses: vec![Address {
                address: "+15551234".into(),
            }],
            date,
            message_type: MessageType::Inbox,
            read: false,
            thread_id,
            uid: 1,
            sub_id: -1,
            attachments: vec![],
        }
    }

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
        assert!(app.conversations_client.is_none());
    }

    #[test]
    fn test_conversation_created_new_thread() {
        let mut app = App::new_test();
        assert!(app.conversations.is_empty());

        let msg = make_test_message(42, 1000, "Hello!");
        app.handle_conversation_created(msg);

        assert_eq!(app.conversations.len(), 1);
        assert_eq!(app.conversations[0].thread_id, 42);
        assert_eq!(app.conversations[0].preview_text(), "Hello!");
    }

    #[test]
    fn test_conversation_created_existing_thread_updates() {
        let mut app = App::new_test();

        app.handle_conversation_created(make_test_message(1, 1000, "old"));
        app.handle_conversation_created(make_test_message(1, 2000, "new"));

        assert_eq!(app.conversations.len(), 1);
        assert_eq!(app.conversations[0].preview_text(), "new");
    }

    #[test]
    fn test_conversation_updated() {
        let mut app = App::new_test();

        app.handle_conversation_created(make_test_message(1, 1000, "first"));
        app.handle_conversation_updated(make_test_message(1, 2000, "updated"));

        assert_eq!(app.conversations.len(), 1);
        assert_eq!(app.conversations[0].preview_text(), "updated");
    }

    #[test]
    fn test_conversation_updated_unknown_thread() {
        let mut app = App::new_test();

        // Updated on a thread we don't know about should create it
        app.handle_conversation_updated(make_test_message(99, 1000, "surprise"));

        assert_eq!(app.conversations.len(), 1);
        assert_eq!(app.conversations[0].thread_id, 99);
    }

    #[test]
    fn test_conversation_removed() {
        let mut app = App::new_test();

        app.handle_conversation_created(make_test_message(1, 1000, "a"));
        app.handle_conversation_created(make_test_message(2, 2000, "b"));
        app.handle_conversation_created(make_test_message(3, 3000, "c"));
        app.selected_conversation_idx = Some(2);

        app.handle_conversation_removed(3);

        assert_eq!(app.conversations.len(), 2);
        // Selection should adjust since index 2 no longer exists
        assert_eq!(app.selected_conversation_idx, Some(1));
    }

    #[test]
    fn test_conversation_removed_all() {
        let mut app = App::new_test();

        app.handle_conversation_created(make_test_message(1, 1000, "only one"));
        app.selected_conversation_idx = Some(0);

        app.handle_conversation_removed(1);

        assert!(app.conversations.is_empty());
        assert_eq!(app.selected_conversation_idx, None);
    }

    #[test]
    fn test_conversations_sorted_after_signal() {
        let mut app = App::new_test();

        app.handle_conversation_created(make_test_message(1, 1000, "old"));
        app.handle_conversation_created(make_test_message(2, 3000, "newest"));
        app.handle_conversation_created(make_test_message(3, 2000, "middle"));

        // Should be sorted newest first
        assert_eq!(app.conversations[0].thread_id, 2);
        assert_eq!(app.conversations[1].thread_id, 3);
        assert_eq!(app.conversations[2].thread_id, 1);

        // Now update thread 1 to be the newest
        app.handle_conversation_updated(make_test_message(1, 5000, "now newest"));

        assert_eq!(app.conversations[0].thread_id, 1);
        assert_eq!(app.conversations[0].preview_text(), "now newest");
    }
}
