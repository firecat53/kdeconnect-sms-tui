use std::collections::{HashMap, HashSet};
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use color_eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::execute;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;
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

/// Which panel has keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    ConversationList,
    Compose,
}

/// State of an image attachment being fetched/decoded.
pub enum ImageState {
    /// Request sent, waiting for file
    Downloading,
    /// Image decoded and ready for rendering
    Loaded(Box<StatefulProtocol>),
    /// Failed to load
    Failed(String),
}

pub struct App {
    pub config: Config,
    pub devices: Vec<Device>,
    pub selected_device_idx: Option<usize>,
    pub conversations: Vec<Conversation>,
    pub selected_conversation_idx: Option<usize>,
    pub contacts: ContactStore,
    pub message_scroll: u16,
    /// Last known height of the message viewport (set during render).
    pub message_view_height: u16,
    pub should_quit: bool,
    pub loading: LoadingState,
    pub status_message: Option<String>,
    pub focus: Focus,
    pub compose_input: String,
    /// Cursor position in compose_input (byte offset)
    pub compose_cursor: usize,
    daemon: Option<DaemonClient>,
    conversations_client: Option<ConversationsClient>,
    /// Sender for injecting D-Bus signal events
    signal_tx: Option<tokio::sync::mpsc::UnboundedSender<AppEvent>>,
    /// Terminal image protocol picker (None if detection failed)
    pub picker: Option<Picker>,
    /// Image states keyed by attachment unique_identifier
    pub image_states: HashMap<String, ImageState>,
    /// Attachment unique_identifiers that have been requested (to avoid duplicates)
    pending_attachments: HashSet<String>,
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
            message_view_height: 0,
            should_quit: false,
            loading: LoadingState::Idle,
            status_message: None,
            focus: Focus::ConversationList,
            compose_input: String::new(),
            compose_cursor: 0,
            daemon,
            conversations_client: None,
            signal_tx: None,
            picker: None,
            image_states: HashMap::new(),
            pending_attachments: HashSet::new(),
        };

        app.refresh_devices().await;

        // Resolve initial device (non-fatal if kdeconnect is unresponsive)
        if let Some(ref daemon) = app.daemon {
            match daemon
                .resolve_device(device_id.as_deref(), device_name.as_deref())
                .await
            {
                Ok(Some(dev)) => {
                    app.selected_device_idx =
                        app.devices.iter().position(|d| d.id == dev.id);
                }
                Ok(None) => {
                    app.status_message = Some("No reachable device found".into());
                }
                Err(e) => {
                    tracing::warn!("Failed to resolve device: {}", e);
                    app.status_message =
                        Some("Could not reach kdeconnect daemon (press 'r' to retry)".into());
                }
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
            message_view_height: 0,
            should_quit: false,
            loading: LoadingState::Idle,
            status_message: None,
            focus: Focus::ConversationList,
            compose_input: String::new(),
            compose_cursor: 0,
            daemon: None,
            conversations_client: None,
            signal_tx: None,
            picker: None,
            image_states: HashMap::new(),
            pending_attachments: HashSet::new(),
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

        // Then fetch what's cached, preserving any loaded messages
        match client.active_conversations().await {
            Ok(convos) => {
                // Merge new data, preserving messages from existing conversations
                let mut old_map: HashMap<i64, _> = self
                    .conversations
                    .drain(..)
                    .map(|c| (c.thread_id, c))
                    .collect();
                for mut new_conv in convos {
                    if let Some(old) = old_map.remove(&new_conv.thread_id) {
                        // Preserve loaded messages
                        new_conv.messages = old.messages;
                    }
                    self.conversations.push(new_conv);
                }
                sort_by_recent(&mut self.conversations);
                self.loading = LoadingState::Idle;
                let count = self.conversations.len();
                self.status_message = Some(format!("{} conversations loaded", count));

                // Auto-select first if none selected, and request its messages
                if self.selected_conversation_idx.is_none() && !self.conversations.is_empty() {
                    self.selected_conversation_idx = Some(0);
                    self.request_selected_conversation_messages();
                }
            }
            Err(e) => {
                self.loading = LoadingState::Error(format!("Failed to load: {}", e));
                self.status_message = Some(format!("Error: {}", e));
            }
        }
    }

    /// Fetch cached conversations from kdeconnect without requesting a new sync.
    /// Preserves any messages already loaded in existing conversations.
    async fn refresh_cached_conversations(&mut self) {
        let Some(ref client) = self.conversations_client else {
            return;
        };

        match client.active_conversations().await {
            Ok(new_convos) => {
                // Merge: preserve messages already loaded in existing conversations
                for new_conv in new_convos {
                    if let Some(existing) = self.conversations.iter_mut().find(|c| c.thread_id == new_conv.thread_id) {
                        // Update metadata but keep loaded messages
                        existing.is_group = new_conv.is_group;
                        if let Some(ref new_latest) = new_conv.latest_message {
                            let dominated = existing
                                .latest_message
                                .as_ref()
                                .is_none_or(|e| new_latest.date > e.date);
                            if dominated {
                                existing.latest_message = new_conv.latest_message;
                            }
                        }
                    } else {
                        self.conversations.push(new_conv);
                    }
                }
                sort_by_recent(&mut self.conversations);
                self.loading = LoadingState::Idle;
                let count = self.conversations.len();
                self.status_message = Some(format!("{} conversations loaded", count));

                if self.selected_conversation_idx.is_none() && !self.conversations.is_empty() {
                    self.selected_conversation_idx = Some(0);
                    self.request_selected_conversation_messages();
                }
            }
            Err(e) => {
                self.status_message = Some(format!("Refresh error: {}", e));
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

        // Detect terminal image protocol (must be after entering alternate screen)
        self.picker = match Picker::from_query_stdio() {
            Ok(p) => {
                info!("Detected image protocol: {:?}", p.protocol_type());
                Some(p)
            }
            Err(e) => {
                info!("Image protocol detection failed, using halfblocks: {}", e);
                Some(Picker::halfblocks())
            }
        };

        let result = self.run_inner(&mut terminal).await;

        // Always restore terminal, even on error
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;

        result
    }

    async fn run_inner(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) -> Result<()> {
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
                let thread_id = msg.thread_id;
                self.handle_conversation_created(msg);
                self.auto_request_attachments_for(thread_id);
            }
            AppEvent::ConversationUpdated(msg) => {
                let thread_id = msg.thread_id;
                self.handle_conversation_updated(msg);
                self.auto_request_attachments_for(thread_id);
            }
            AppEvent::ConversationRemoved(thread_id) => {
                self.handle_conversation_removed(thread_id);
            }
            AppEvent::ConversationsLoaded => {
                // Phone finished sending data — only fetch cached results.
                // Do NOT call request_all_conversation_threads() here or it
                // creates an infinite loop (request → signal → request → …).
                self.refresh_cached_conversations().await;
            }
            AppEvent::AttachmentReceived(file_path, file_name) => {
                self.handle_attachment_received(&file_path, &file_name);
            }
        }
    }

    /// Handle a new conversation appearing via D-Bus signal.
    fn handle_conversation_created(&mut self, msg: Message) {
        let thread_id = msg.thread_id;

        // Check if we already have this thread
        if let Some(conv) = self.conversations.iter_mut().find(|c| c.thread_id == thread_id) {
            conv.is_group = conv.is_group || msg.addresses.len() > 2;
            let is_newer = conv
                .latest_message
                .as_ref()
                .is_none_or(|existing| msg.date > existing.date);
            if is_newer {
                conv.latest_message = Some(msg.clone());
            }
            insert_message_sorted(&mut conv.messages, msg);
        } else {
            let mut conv = Conversation::new(thread_id);
            conv.is_group = msg.addresses.len() > 2;
            conv.latest_message = Some(msg.clone());
            conv.messages.push(msg);
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
                conv.latest_message = Some(msg.clone());
            }
            insert_message_sorted(&mut conv.messages, msg);
        } else {
            // New thread we didn't know about
            self.handle_conversation_created(msg);
            return;
        }

        sort_by_recent(&mut self.conversations);
    }

    /// Request message history for the currently selected conversation.
    /// Spawns the D-Bus call as a background task so it doesn't block the UI.
    fn request_selected_conversation_messages(&mut self) {
        let Some(idx) = self.selected_conversation_idx else {
            return;
        };
        let Some(conv) = self.conversations.get(idx) else {
            return;
        };
        let Some(ref client) = self.conversations_client else {
            return;
        };

        let thread_id = conv.thread_id;
        let connection = client.connection().clone();
        let device_id = client.device_id().to_owned();

        // Fire-and-forget: the phone will send messages back via D-Bus signals
        tokio::spawn(async move {
            let client = ConversationsClient::new(connection, device_id);
            if let Err(e) = client.request_conversation(thread_id, 0, 50).await {
                tracing::warn!("Failed to request conversation {}: {}", thread_id, e);
            }
        });

        // Also request any image attachments
        self.request_conversation_attachments();
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

    /// Request attachments if the given thread is the currently selected conversation.
    fn auto_request_attachments_for(&mut self, thread_id: i64) {
        if let Some(idx) = self.selected_conversation_idx {
            if let Some(conv) = self.conversations.get(idx) {
                if conv.thread_id == thread_id {
                    self.request_conversation_attachments();
                }
            }
        }
    }

    /// Handle an attachment file arriving from kdeconnect.
    fn handle_attachment_received(&mut self, file_path: &str, _file_name: &str) {
        let path = PathBuf::from(file_path);
        if !path.exists() {
            tracing::warn!("Attachment file not found: {}", file_path);
            return;
        }

        // Find which attachment(s) match this file path.
        // kdeconnect uses uniqueIdentifier as filename in its cache,
        // so we match by checking if the path ends with the unique_identifier.
        let file_stem = path.file_name().and_then(|f| f.to_str()).unwrap_or("");

        // Update cached_path on matching attachments in all conversations
        for conv in &mut self.conversations {
            for msg in &mut conv.messages {
                for att in &mut msg.attachments {
                    if att.unique_identifier == file_stem
                        || file_path.contains(&att.unique_identifier)
                    {
                        att.cached_path = Some(path.clone());
                    }
                }
            }
            if let Some(ref mut msg) = conv.latest_message {
                for att in &mut msg.attachments {
                    if att.unique_identifier == file_stem
                        || file_path.contains(&att.unique_identifier)
                    {
                        att.cached_path = Some(path.clone());
                    }
                }
            }
        }

        // Decode image if applicable
        if let Some(ref picker) = self.picker {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            let is_image = matches!(
                ext.to_lowercase().as_str(),
                "jpg" | "jpeg" | "png" | "gif" | "bmp" | "webp"
            ) || file_stem
                .split('.')
                .next()
                .is_some_and(|_| {
                    // Try to detect from the attachment metadata
                    self.conversations.iter().any(|c| {
                        c.messages.iter().any(|m| {
                            m.attachments.iter().any(|a| {
                                (a.unique_identifier == file_stem
                                    || file_path.contains(&a.unique_identifier))
                                    && a.is_image()
                            })
                        })
                    })
                });

            if is_image {
                // Use content-based format detection instead of relying on file
                // extension.  KDE Connect attachment filenames often lack a proper
                // extension, causing `image::open()` to fail with
                // "The image format could not be determined".
                let load_result = image::ImageReader::open(&path)
                    .and_then(|r| r.with_guessed_format())
                    .map_err(image::ImageError::IoError)
                    .and_then(|r| r.decode());
                let load_result = match load_result {
                    Ok(img) => Ok(img),
                    Err(_) => {
                        // Fallback: try converting via ImageMagick or heif-convert.
                        // Handles HEIC and other formats the image crate can't decode.
                        Self::convert_image_external(&path)
                    }
                };
                match load_result {
                    Ok(dyn_img) => {
                        let protocol = picker.new_resize_protocol(dyn_img);
                        self.image_states.insert(
                            file_stem.to_string(),
                            ImageState::Loaded(Box::new(protocol)),
                        );
                    }
                    Err(e) => {
                        tracing::warn!("Failed to decode image {}: {}", file_path, e);
                        self.image_states.insert(
                            file_stem.to_string(),
                            ImageState::Failed(e.to_string()),
                        );
                    }
                }
            }
        }

        self.pending_attachments.remove(file_stem);
    }

    /// Try to convert an image file to PNG using an external tool (ImageMagick or
    /// heif-convert).  This handles HEIC and other formats the `image` crate
    /// cannot decode natively.
    fn convert_image_external(path: impl AsRef<std::path::Path>) -> Result<image::DynamicImage, image::ImageError> {
        use std::process::Command;

        let path = path.as_ref();
        let tmp = std::env::temp_dir().join(format!(
            "kdeconnect-sms-tui-conv-{}.png",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));

        // Try ImageMagick first, then heif-convert.
        let attempts: &[(&str, &[&str])] = &[
            ("magick", &["convert"]),
            ("heif-convert", &[]),
        ];

        for (bin, pre_args) in attempts {
            let mut cmd = Command::new(bin);
            cmd.args(*pre_args).arg(path).arg(&tmp);
            if let Ok(output) = cmd.output() {
                if output.status.success() && tmp.exists() {
                    let result = image::open(&tmp);
                    let _ = std::fs::remove_file(&tmp);
                    return result;
                }
            }
        }

        Err(image::ImageError::Unsupported(
            image::error::UnsupportedError::from_format_and_kind(
                image::error::ImageFormatHint::Unknown,
                image::error::UnsupportedErrorKind::Format(
                    image::error::ImageFormatHint::Unknown,
                ),
            ),
        ))
    }

    /// Request downloads for all image attachments in the currently selected conversation.
    fn request_conversation_attachments(&mut self) {
        let Some(idx) = self.selected_conversation_idx else {
            return;
        };
        let Some(conv) = self.conversations.get(idx) else {
            return;
        };
        let Some(ref client) = self.conversations_client else {
            return;
        };

        let connection = client.connection().clone();
        let device_id = client.device_id().to_owned();

        // Collect attachments that need downloading
        let mut to_request: Vec<(i64, String)> = Vec::new();
        for msg in &conv.messages {
            for att in &msg.attachments {
                if att.is_image()
                    && !att.is_cached()
                    && !self.pending_attachments.contains(&att.unique_identifier)
                    && !self.image_states.contains_key(&att.unique_identifier)
                {
                    to_request.push((att.part_id, att.unique_identifier.clone()));
                    self.pending_attachments.insert(att.unique_identifier.clone());
                    self.image_states.insert(
                        att.unique_identifier.clone(),
                        ImageState::Downloading,
                    );
                }
            }
        }

        // Also check latest_message (may not be in messages vec yet)
        if let Some(ref msg) = conv.latest_message {
            for att in &msg.attachments {
                if att.is_image()
                    && !att.is_cached()
                    && !self.pending_attachments.contains(&att.unique_identifier)
                    && !self.image_states.contains_key(&att.unique_identifier)
                {
                    to_request.push((att.part_id, att.unique_identifier.clone()));
                    self.pending_attachments.insert(att.unique_identifier.clone());
                    self.image_states.insert(
                        att.unique_identifier.clone(),
                        ImageState::Downloading,
                    );
                }
            }
        }

        // Also load any already-cached images that haven't been decoded yet
        if let Some(ref picker) = self.picker {
            for msg in &conv.messages {
                for att in &msg.attachments {
                    if att.is_image()
                        && att.is_cached()
                        && !self.image_states.contains_key(&att.unique_identifier)
                    {
                        if let Some(ref path) = att.cached_path {
                            match image::open(path) {
                                Ok(dyn_img) => {
                                    let protocol = picker.new_resize_protocol(dyn_img);
                                    self.image_states.insert(
                                        att.unique_identifier.clone(),
                                        ImageState::Loaded(Box::new(protocol)),
                                    );
                                }
                                Err(e) => {
                                    self.image_states.insert(
                                        att.unique_identifier.clone(),
                                        ImageState::Failed(e.to_string()),
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        if !to_request.is_empty() {
            tracing::info!("Requesting {} attachments", to_request.len());
            tokio::spawn(async move {
                let client = ConversationsClient::new(connection, device_id);
                for (part_id, uid) in to_request {
                    if let Err(e) = client.request_attachment_file(part_id, &uid).await {
                        tracing::warn!("Failed to request attachment {}: {}", uid, e);
                    }
                }
            });
        }
    }

    async fn handle_key(
        &mut self,
        key: KeyEvent,
        signal_tx: tokio::sync::mpsc::UnboundedSender<AppEvent>,
    ) {
        // Ctrl-C always quits
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.should_quit = true;
            return;
        }

        match self.focus {
            Focus::ConversationList => self.handle_key_normal(key, signal_tx).await,
            Focus::Compose => self.handle_key_compose(key).await,
        }
    }

    async fn handle_key_normal(
        &mut self,
        key: KeyEvent,
        signal_tx: tokio::sync::mpsc::UnboundedSender<AppEvent>,
    ) {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,

            // Device switching
            KeyCode::Tab => {
                self.cycle_device();
                self.connect_to_device(signal_tx).await;
            }

            // Conversation navigation
            KeyCode::Up | KeyCode::Char('k') => {
                self.select_prev_conversation();
                self.request_selected_conversation_messages();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.select_next_conversation();
                self.request_selected_conversation_messages();
            }

            // Enter conversation / focus compose
            KeyCode::Enter | KeyCode::Char('i') => {
                if self.selected_conversation_idx.is_some() {
                    self.focus = Focus::Compose;
                    self.request_selected_conversation_messages();
                }
            }

            // Refresh: re-discover devices if none connected, else reload conversations
            KeyCode::Char('r') => {
                if self.conversations_client.is_none() {
                    self.refresh_devices().await;
                    if self.selected_device_idx.is_some() {
                        self.connect_to_device(signal_tx).await;
                    }
                } else {
                    self.load_conversations().await;
                }
            }

            // Message scrolling (scroll is offset from bottom: 0 = newest)
            KeyCode::PageUp => {
                let page = self.message_view_height.max(1);
                self.message_scroll = self.message_scroll.saturating_add(page);
            }
            KeyCode::PageDown => {
                let page = self.message_view_height.max(1);
                self.message_scroll = self.message_scroll.saturating_sub(page);
            }

            _ => {}
        }
    }

    async fn handle_key_compose(&mut self, key: KeyEvent) {
        match key.code {
            // Escape returns to conversation list
            KeyCode::Esc => {
                self.focus = Focus::ConversationList;
            }

            // Enter sends the message
            KeyCode::Enter => {
                if key.modifiers.contains(KeyModifiers::ALT)
                    || key.modifiers.contains(KeyModifiers::SHIFT)
                {
                    // Alt+Enter or Shift+Enter: newline
                    self.compose_input.insert(self.compose_cursor, '\n');
                    self.compose_cursor += 1;
                } else {
                    self.send_message().await;
                }
            }

            // Backspace
            KeyCode::Backspace => {
                if self.compose_cursor > 0 {
                    // Find previous char boundary
                    let prev = self.compose_input[..self.compose_cursor]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    self.compose_input.drain(prev..self.compose_cursor);
                    self.compose_cursor = prev;
                }
            }

            // Delete
            KeyCode::Delete => {
                if self.compose_cursor < self.compose_input.len() {
                    let next = self.compose_input[self.compose_cursor..]
                        .char_indices()
                        .nth(1)
                        .map(|(i, _)| self.compose_cursor + i)
                        .unwrap_or(self.compose_input.len());
                    self.compose_input.drain(self.compose_cursor..next);
                }
            }

            // Cursor movement
            KeyCode::Left => {
                if self.compose_cursor > 0 {
                    self.compose_cursor = self.compose_input[..self.compose_cursor]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                }
            }
            KeyCode::Right => {
                if self.compose_cursor < self.compose_input.len() {
                    self.compose_cursor = self.compose_input[self.compose_cursor..]
                        .char_indices()
                        .nth(1)
                        .map(|(i, _)| self.compose_cursor + i)
                        .unwrap_or(self.compose_input.len());
                }
            }
            KeyCode::Home => {
                self.compose_cursor = 0;
            }
            KeyCode::End => {
                self.compose_cursor = self.compose_input.len();
            }

            // Text input
            KeyCode::Char(c) => {
                self.compose_input.insert(self.compose_cursor, c);
                self.compose_cursor += c.len_utf8();
            }

            // Message scrolling while composing (scroll is offset from bottom)
            KeyCode::Up => {
                self.message_scroll = self.message_scroll.saturating_add(1);
            }
            KeyCode::Down => {
                self.message_scroll = self.message_scroll.saturating_sub(1);
            }
            KeyCode::PageUp => {
                let page = self.message_view_height.max(1);
                self.message_scroll = self.message_scroll.saturating_add(page);
            }
            KeyCode::PageDown => {
                let page = self.message_view_height.max(1);
                self.message_scroll = self.message_scroll.saturating_sub(page);
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
        self.focus = Focus::ConversationList;
        self.compose_input.clear();
        self.compose_cursor = 0;
    }

    /// Send the current compose input as a reply.
    async fn send_message(&mut self) {
        let text = self.compose_input.trim().to_string();
        if text.is_empty() {
            return;
        }

        let Some(idx) = self.selected_conversation_idx else {
            self.status_message = Some("No conversation selected".into());
            return;
        };
        let Some(conv) = self.conversations.get(idx) else {
            return;
        };
        let Some(ref client) = self.conversations_client else {
            self.status_message = Some("Not connected to device".into());
            return;
        };

        let thread_id = conv.thread_id;

        match client.reply_to_conversation(thread_id, &text).await {
            Ok(()) => {
                self.compose_input.clear();
                self.compose_cursor = 0;
                self.status_message = Some("Message sent".into());
            }
            Err(e) => {
                self.status_message = Some(format!("Send failed: {}", e));
            }
        }
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

/// Insert a message into a sorted (by date ascending) list, avoiding duplicates by uid.
fn insert_message_sorted(messages: &mut Vec<Message>, msg: Message) {
    // Avoid duplicates
    if messages.iter().any(|m| m.uid == msg.uid && m.date == msg.date) {
        return;
    }
    let pos = messages.partition_point(|m| m.date <= msg.date);
    messages.insert(pos, msg);
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

    #[test]
    fn test_insert_message_sorted() {
        let mut messages = Vec::new();

        insert_message_sorted(&mut messages, make_test_message(1, 3000, "third"));
        insert_message_sorted(&mut messages, make_test_message(1, 1000, "first"));
        insert_message_sorted(&mut messages, make_test_message(1, 2000, "second"));

        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].body, "first");
        assert_eq!(messages[1].body, "second");
        assert_eq!(messages[2].body, "third");
    }

    #[test]
    fn test_insert_message_deduplication() {
        let mut messages = Vec::new();

        let msg = make_test_message(1, 1000, "hello");
        insert_message_sorted(&mut messages, msg.clone());
        insert_message_sorted(&mut messages, msg); // duplicate

        assert_eq!(messages.len(), 1);
    }

    #[test]
    fn test_conversation_signals_populate_messages() {
        let mut app = App::new_test();

        app.handle_conversation_created(make_test_message(1, 1000, "first"));
        app.handle_conversation_updated(make_test_message(1, 2000, "second"));
        app.handle_conversation_updated(make_test_message(1, 3000, "third"));

        assert_eq!(app.conversations[0].messages.len(), 3);
        assert_eq!(app.conversations[0].messages[0].body, "first");
        assert_eq!(app.conversations[0].messages[2].body, "third");
    }

    #[test]
    fn test_focus_transitions() {
        let mut app = App::new_test();
        assert_eq!(app.focus, Focus::ConversationList);

        // Can't enter compose without a conversation selected
        app.focus = Focus::Compose;
        assert_eq!(app.focus, Focus::Compose);

        // Esc goes back
        app.focus = Focus::ConversationList;
        assert_eq!(app.focus, Focus::ConversationList);
    }

    #[test]
    fn test_compose_input_basic() {
        let mut app = App::new_test();
        app.compose_input = "hello".into();
        app.compose_cursor = 5;

        assert_eq!(app.compose_input, "hello");
        assert_eq!(app.compose_cursor, 5);

        // Simulate backspace
        let prev = app.compose_input[..app.compose_cursor]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(0);
        app.compose_input.drain(prev..app.compose_cursor);
        app.compose_cursor = prev;

        assert_eq!(app.compose_input, "hell");
        assert_eq!(app.compose_cursor, 4);
    }

    #[test]
    fn test_cycle_device_resets_compose() {
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
        app.focus = Focus::Compose;
        app.compose_input = "draft message".into();
        app.compose_cursor = 13;

        app.cycle_device();

        assert_eq!(app.focus, Focus::ConversationList);
        assert!(app.compose_input.is_empty());
        assert_eq!(app.compose_cursor, 0);
    }
}
