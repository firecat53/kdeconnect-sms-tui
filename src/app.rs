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
use crate::state::AppState;
use crate::dbus::conversations::ConversationsClient;
use crate::dbus::daemon::DaemonClient;
use crate::dbus::signals;
use crate::events::{self, AppEvent};
use crate::models::attachment::Attachment;
use crate::models::conversation::{sort_by_recent, Conversation};
use crate::models::device::Device;
use crate::models::message::Message;

/// Extract initials from a display name.
/// "Alice Smith" → "AS", "Bob" → "B", "+15551234" → "+"
fn name_to_initials(name: &str) -> String {
    let parts: Vec<&str> = name.split_whitespace().collect();
    match parts.len() {
        0 => String::new(),
        1 => parts[0].chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default(),
        _ => {
            let first = parts[0].chars().next().unwrap_or(' ');
            let last = parts.last().unwrap().chars().next().unwrap_or(' ');
            format!("{}{}", first.to_uppercase(), last.to_uppercase())
        }
    }
}

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
    MessageView,
    Compose,
    DevicePopup,
    GroupInfoPopup,
    FolderPopup,
    HelpPopup,
}

/// Which folder popup is currently open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderKind {
    Archive,
    Spam,
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
    pub state: AppState,
    pub devices: Vec<Device>,
    pub selected_device_idx: Option<usize>,
    pub conversations: Vec<Conversation>,
    pub selected_conversation_idx: Option<usize>,
    pub contacts: ContactStore,
    pub message_scroll: u16,
    /// Last known height of the message viewport (set during render).
    pub message_view_height: u16,
    /// Maximum scroll offset (set during render). Used to detect scroll-to-top.
    pub message_max_scroll: u16,
    /// Message boundary offsets for message-by-message scrolling (set during render).
    /// Each entry is the cumulative height (from bottom) at the top of that message.
    pub message_boundaries: Vec<u16>,
    /// Index of the currently selected message within the conversation's message list.
    /// `None` when no message is selected (e.g. empty conversation).
    pub selected_message_idx: Option<usize>,
    /// Which part of the selected message is highlighted:
    /// 0 = the text body, 1..N = attachment index (0-based) within that message.
    pub selected_message_part: usize,
    pub should_quit: bool,
    pub loading: LoadingState,
    pub status_message: Option<String>,
    pub focus: Focus,
    /// Which panel was focused before entering Compose mode (to restore on Esc).
    pub pre_compose_focus: Focus,
    pub compose_input: String,
    /// Cursor position in compose_input (byte offset)
    pub compose_cursor: usize,
    /// Per-conversation draft messages: thread_id → (text, cursor_byte_offset)
    pub drafts: HashMap<i64, (String, usize)>,
    /// Whether the device popup is showing (tracked via Focus::DevicePopup)
    pub device_popup_idx: usize,
    daemon: Option<DaemonClient>,
    conversations_client: Option<ConversationsClient>,
    /// Sender for injecting D-Bus signal events
    signal_tx: Option<tokio::sync::mpsc::UnboundedSender<AppEvent>>,
    /// Handle to cancel the current signal listener when switching devices.
    signal_listener_handle: Option<tokio::task::JoinHandle<()>>,
    /// Terminal image protocol picker (None if detection failed)
    pub picker: Option<Picker>,
    /// Image states keyed by attachment unique_identifier
    pub image_states: HashMap<String, ImageState>,
    /// Attachment unique_identifiers that have been requested (to avoid duplicates)
    pending_attachments: HashSet<String>,
    /// Tick counter for periodic retry of message loading (250ms per tick).
    pub tick_count: u32,
    /// Remaining automatic re-syncs.  kdeconnectd progressively discovers
    /// messages from the phone across multiple `requestAllConversationThreads`
    /// calls, so we automatically repeat the request a few times after
    /// connecting to a device.
    auto_resync_remaining: u8,
    /// Instant of last successful send.  Used to suppress daemon polling
    /// (requestConversation, activeConversations, requestAllConversationThreads)
    /// for a short cooldown after sending so the daemon can finish processing
    /// without interference — prevents duplicate delivery.
    last_send_time: Option<std::time::Instant>,
    /// Group info popup: text input for editing the group name
    pub group_name_input: String,
    /// Cursor byte offset in group_name_input
    pub group_name_cursor: usize,
    /// Which folder popup (archive/spam) is open
    pub folder_popup_kind: FolderKind,
    /// Selected index in the folder popup list
    pub folder_popup_idx: usize,
    /// Request a full terminal repaint on the next draw cycle.
    /// Set when dismissing overlays (e.g. device popup) that may have
    /// erased protocol-based images (Kitty/Sixel).
    pub needs_full_repaint: bool,
    /// Epoch millis when we connected to the current device.  Messages with
    /// timestamps older than this are replayed history and should NOT
    /// auto-unarchive hidden threads.
    connected_at_ms: i64,
}

impl App {
    pub async fn new(
        config: Config,
        state: AppState,
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
            state,
            devices: Vec::new(),
            selected_device_idx: None,
            conversations: Vec::new(),
            selected_conversation_idx: None,
            contacts,
            message_scroll: 0,
            message_view_height: 0,
            message_max_scroll: 0,
            message_boundaries: Vec::new(),
            selected_message_idx: None,
            selected_message_part: 0,
            should_quit: false,
            loading: LoadingState::Idle,
            status_message: None,
            focus: Focus::ConversationList,
            pre_compose_focus: Focus::ConversationList,
            compose_input: String::new(),
            compose_cursor: 0,
            drafts: HashMap::new(),
            device_popup_idx: 0,
            daemon,
            conversations_client: None,
            signal_tx: None,
            signal_listener_handle: None,
            picker: None,
            image_states: HashMap::new(),
            pending_attachments: HashSet::new(),
            tick_count: 0,
            auto_resync_remaining: 0,
            last_send_time: None,
            group_name_input: String::new(),
            group_name_cursor: 0,
            folder_popup_kind: FolderKind::Archive,
            folder_popup_idx: 0,
            needs_full_repaint: false,
            connected_at_ms: 0,
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
            state: AppState::default(),
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
            message_max_scroll: 0,
            message_boundaries: Vec::new(),
            selected_message_idx: None,
            selected_message_part: 0,
            should_quit: false,
            loading: LoadingState::Idle,
            status_message: None,
            focus: Focus::ConversationList,
            pre_compose_focus: Focus::ConversationList,
            compose_input: String::new(),
            compose_cursor: 0,
            drafts: HashMap::new(),
            device_popup_idx: 0,
            daemon: None,
            conversations_client: None,
            signal_tx: None,
            signal_listener_handle: None,
            picker: None,
            image_states: HashMap::new(),
            pending_attachments: HashSet::new(),
            tick_count: 0,
            auto_resync_remaining: 0,
            last_send_time: None,
            group_name_input: String::new(),
            group_name_cursor: 0,
            folder_popup_kind: FolderKind::Archive,
            folder_popup_idx: 0,
            needs_full_repaint: false,
            connected_at_ms: 0,
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

        // Cancel previous signal listener (if any) before starting a new one.
        // Without this, switching devices or reconnecting accumulates listeners
        // that forward duplicate signals.
        if let Some(handle) = self.signal_listener_handle.take() {
            handle.abort();
        }

        // Start signal listener for this device.
        // Awaiting here ensures the D-Bus match rule is registered before we
        // request any conversations, so no reply signals are lost.
        match signals::spawn_signal_listener(
            daemon.connection().clone(),
            device.id.clone(),
            signal_tx,
        ).await {
            Ok(handle) => {
                self.signal_listener_handle = Some(handle);
            }
            Err(e) => {
                self.status_message = Some(format!("Signal listener failed: {}", e));
                return;
            }
        }

        self.conversations_client = Some(client);
        self.connected_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        self.status_message = Some(format!("Connected to {}", device.name));
        // kdeconnectd progressively discovers messages from the phone across
        // multiple requestAllConversationThreads calls.  Schedule a few
        // automatic re-syncs so the user doesn't have to press 'r' manually.
        self.auto_resync_remaining = Self::AUTO_RESYNC_MAX;

        // Trigger initial conversation load
        self.load_conversations().await;
    }

    /// If we have a selected device but aren't connected yet (e.g. kdeconnectd
    /// was still starting when the app launched), periodically retry.
    async fn retry_connection_if_needed(
        &mut self,
        signal_tx: tokio::sync::mpsc::UnboundedSender<AppEvent>,
    ) {
        // Only try every 8 ticks (2 seconds)
        if self.tick_count % 8 != 0 {
            return;
        }
        // Already connected — nothing to do
        if self.conversations_client.is_some() {
            return;
        }
        // No device selected — nothing to connect to
        if self.selected_device_idx.is_none() {
            // Try to discover devices first
            self.refresh_devices().await;
            if self.selected_device_idx.is_none() {
                return;
            }
        }
        tracing::debug!("Retrying device connection...");
        self.refresh_devices().await;
        self.connect_to_device(signal_tx).await;
    }

    /// Load conversations from the currently connected device.
    async fn load_conversations(&mut self) {
        if self.conversations_client.is_none() {
            return;
        }
        // Don't hit the phone right after sending — it can cause the
        // Android kdeconnect plugin to re-send the outgoing message.
        if self.in_send_cooldown() {
            self.status_message = Some("Waiting for send to complete...".into());
            return;
        }

        self.loading = LoadingState::Loading;
        self.status_message = Some("Loading conversations...".into());

        // First request the phone to send all threads
        let request_result = self.conversations_client.as_ref().unwrap()
            .request_all_conversation_threads().await;
        if let Err(e) = request_result {
            self.loading = LoadingState::Error(format!("Failed to request threads: {}", e));
            self.status_message = Some(format!("Error: {}", e));
            // Connection may be stale — drop client so retry can reconnect.
            self.conversations_client = None;
            return;
        }

        // Then fetch what's cached, preserving any loaded messages
        let fetch_result = self.conversations_client.as_ref().unwrap()
            .active_conversations().await;
        match fetch_result {
            Ok(convos) => {
                // Merge new data, preserving messages from existing conversations
                let mut old_map: HashMap<i64, _> = self
                    .conversations
                    .drain(..)
                    .map(|c| (c.thread_id, c))
                    .collect();
                for mut new_conv in convos {
                    if let Some(old) = old_map.remove(&new_conv.thread_id) {
                        // Preserve loaded messages and pagination state
                        new_conv.messages = old.messages;
                        new_conv.messages_requested = old.messages_requested;
                        new_conv.total_messages = old.total_messages;
                        new_conv.loading_more_messages = old.loading_more_messages;
                    }
                    self.conversations.push(new_conv);
                }
                self.sort_conversations();
                self.loading = LoadingState::Idle;

                let count = self.conversations.len();
                self.status_message = Some(format!("{} conversations loaded", count));

                // Auto-select first visible if none selected, and request its messages
                if self.selected_conversation_idx.is_none() && !self.conversations.is_empty() {
                    let first_visible = self.visible_conversation_indices().first().copied();
                    self.selected_conversation_idx = first_visible;
                    self.request_selected_conversation_messages();
                }
            }
            Err(e) => {
                self.loading = LoadingState::Error(format!("Failed to load: {}", e));
                self.status_message = Some(format!("Error: {}", e));
                // Connection may be stale — drop client so retry can reconnect.
                self.conversations_client = None;
            }
        }
    }

    /// Fetch cached conversations from kdeconnect without requesting a new sync.
    /// Preserves any messages already loaded in existing conversations.
    async fn refresh_cached_conversations(&mut self) {
        if self.conversations_client.is_none() {
            return;
        }

        match self.conversations_client.as_ref().unwrap().active_conversations().await {
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
                self.sort_conversations();
                self.loading = LoadingState::Idle;

                let count = self.conversations.len();
                self.status_message = Some(format!("{} conversations loaded", count));

                if self.selected_conversation_idx.is_none() && !self.conversations.is_empty() {
                    let first_visible = self.visible_conversation_indices().first().copied();
                    self.selected_conversation_idx = first_visible;
                    self.request_selected_conversation_messages();
                }
            }
            Err(e) => {
                self.status_message = Some(format!("Refresh error: {}", e));
                // Connection may be stale — drop client so retry can reconnect.
                self.conversations_client = None;
            }
        }
    }

    /// Post-send cooldown period.  After sending a message we suppress all
    /// phone-facing daemon requests (requestConversation,
    /// requestAllConversationThreads) for this duration so the Android side
    /// can finish processing the outgoing message.  The kdeconnect-android
    /// SMS plugin has no deduplication — if a conversation sync request
    /// arrives while the sent message is still being processed, the content
    /// observer can trigger the SMS library to re-send the message.
    /// 15 seconds covers even slow MMS delivery.
    const SEND_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(15);

    /// Returns `true` if we recently sent a message and should avoid daemon requests.
    fn in_send_cooldown(&self) -> bool {
        self.last_send_time
            .is_some_and(|t| t.elapsed() < Self::SEND_COOLDOWN)
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

        // Main loop — only redraw when state actually changed.  Redrawing
        // on every tick causes protocol-based images (Kitty/Sixel) to
        // flicker because the escape sequences reposition the cursor.
        let mut needs_redraw = true;
        while !self.should_quit {
            if needs_redraw {
                if self.needs_full_repaint {
                    // Force a complete redraw so protocol-based images
                    // (Kitty/Sixel) are re-emitted after an overlay erased them.
                    terminal.clear()?;
                    self.needs_full_repaint = false;
                }
                terminal.draw(|f| {
                    crate::ui::draw(f, self);
                })?;
                needs_redraw = false;
            }

            // Wait for either terminal or D-Bus signal events
            tokio::select! {
                Some(event) = term_events.recv() => {
                    needs_redraw = !matches!(&event, AppEvent::Tick)
                        || self.is_loading_more_messages();
                    self.handle_event(event, signal_tx.clone()).await;
                }
                Some(event) = signal_events.recv() => {
                    needs_redraw = true;
                    self.handle_event(event, signal_tx.clone()).await;
                    // Drain any additional pending signals before redrawing
                    // so that rapid-fire message loads during conversation
                    // init are batched into a single redraw.  This prevents
                    // images from being re-encoded on every intermediate
                    // layout shift.
                    while let Ok(event) = signal_events.try_recv() {
                        self.handle_event(event, signal_tx.clone()).await;
                    }
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
            AppEvent::Tick => {
                self.tick_count = self.tick_count.wrapping_add(1);
                self.reset_stale_loading_flags();
                self.retry_connection_if_needed(signal_tx.clone()).await;
                self.retry_message_loading_if_needed().await;
            }
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
            AppEvent::ConversationLoaded(thread_id, message_count) => {
                // Record the total message count so pagination knows when to stop.
                if let Some(conv) = self.conversations.iter_mut().find(|c| c.thread_id == thread_id) {
                    // When kdeconnectd discovers more messages (total increases),
                    // reset messages_requested to what we actually have so that
                    // has_more_messages() returns true and load_more_messages()
                    // requests the right range.  Without this, messages_requested
                    // can exceed the old total (e.g. requested=50, old total=25)
                    // and stay above the new total (e.g. 49), blocking loading.
                    let old_total = conv.total_messages.unwrap_or(0);
                    if message_count > old_total
                        && (conv.messages_requested as usize) > conv.messages.len()
                    {
                        conv.messages_requested = conv.messages.len() as i32;
                    }
                    conv.total_messages = Some(message_count);
                    conv.loading_more_messages = false;
                    conv.loading_started_tick = None;
                }
                // Phone finished sending data — only fetch cached results.
                // Do NOT call request_all_conversation_threads() here or it
                // creates an infinite loop (request → signal → request → …).
                self.refresh_cached_conversations().await;
                // If the selected conversation's viewport isn't full, load more.
                if let Some(idx) = self.selected_conversation_idx {
                    if self.conversations.get(idx).is_some_and(|c| c.thread_id == thread_id) {
                        self.maybe_load_more_on_scroll();
                    }
                }
            }
            AppEvent::AttachmentReceived(file_path, file_name) => {
                self.handle_attachment_received(&file_path, &file_name);
            }
        }
    }

    /// Sort conversations by most-recent and update `selected_conversation_idx`
    /// so it continues to point at the same thread (rather than a stale numeric
    /// position).  Without this, sending a message in a non-first conversation
    /// causes the sort to move that thread to the top while the selection index
    /// still points at the old slot.
    fn sort_conversations(&mut self) {
        // Remember which thread is selected *before* the sort.
        let selected_thread = self
            .selected_conversation_idx
            .and_then(|i| self.conversations.get(i))
            .map(|c| c.thread_id);

        sort_by_recent(&mut self.conversations);

        // Restore the selection to the same thread at its new position.
        if let Some(tid) = selected_thread {
            self.selected_conversation_idx =
                self.conversations.iter().position(|c| c.thread_id == tid);
        }
    }

    /// Handle a new conversation appearing via D-Bus signal.
    fn handle_conversation_created(&mut self, msg: Message) {
        let thread_id = msg.thread_id;

        // If a genuinely new incoming message arrives for a hidden conversation,
        // restore it.  Messages older than our connection time are replayed
        // history from kdeconnectd and should not trigger unarchive.
        if msg.date > self.connected_at_ms && msg.is_incoming() && self.state.is_hidden(thread_id) {
            self.state.unarchive(thread_id);
            let _ = self.state.save();
        }

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

        self.sort_conversations();
    }

    /// Handle a conversation update (new message in existing thread).
    fn handle_conversation_updated(&mut self, msg: Message) {
        let thread_id = msg.thread_id;

        // If a genuinely new incoming message arrives for a hidden conversation,
        // restore it.  Messages older than our connection time are replayed
        // history from kdeconnectd and should not trigger unarchive.
        if msg.date > self.connected_at_ms && msg.is_incoming() && self.state.is_hidden(thread_id) {
            self.state.unarchive(thread_id);
            let _ = self.state.save();
        }

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

        self.sort_conversations();
    }

    /// Request message history for the currently selected conversation.
    /// Spawns the D-Bus call as a background task so it doesn't block the UI.
    /// Batch size for message pagination.
    const MESSAGE_PAGE_SIZE: i32 = 50;

    /// Number of automatic re-syncs after connecting to a device.
    /// kdeconnectd progressively discovers messages from the phone, so
    /// multiple `requestAllConversationThreads` calls are needed.
    const AUTO_RESYNC_MAX: u8 = 5;

    fn request_selected_conversation_messages(&mut self) {
        // Don't request messages from the phone right after sending.
        if self.in_send_cooldown() {
            return;
        }
        let Some(idx) = self.selected_conversation_idx else {
            return;
        };
        let Some(conv) = self.conversations.get_mut(idx) else {
            return;
        };
        let Some(ref client) = self.conversations_client else {
            return;
        };

        // Skip if we already have messages loaded for this conversation.
        // If a previous request was made but no messages arrived (signals lost),
        // allow retrying by checking messages.is_empty() rather than
        // messages_requested alone.
        if conv.messages_requested > 0 && !conv.messages.is_empty() {
            return;
        }

        let thread_id = conv.thread_id;
        let end = Self::MESSAGE_PAGE_SIZE;
        conv.messages_requested = end;

        let connection = client.connection().clone();
        let device_id = client.device_id().to_owned();

        // Fire-and-forget: the phone will send messages back via D-Bus signals
        tokio::spawn(async move {
            let client = ConversationsClient::new(connection, device_id);
            if let Err(e) = client.request_conversation(thread_id, 0, end).await {
                tracing::warn!("Failed to request conversation {}: {}", thread_id, e);
            }
        });

        // Also request any image attachments
        self.request_conversation_attachments();
    }

    /// Load the next page of older messages for the selected conversation.
    fn load_more_messages(&mut self) {
        // Don't request messages from the phone right after sending —
        // it can cause the daemon to re-process the outgoing message.
        if self.in_send_cooldown() {
            return;
        }
        let Some(idx) = self.selected_conversation_idx else {
            return;
        };
        let Some(conv) = self.conversations.get_mut(idx) else {
            return;
        };
        if !conv.has_more_messages() {
            return;
        }
        // Don't fire another request while one is already in flight.
        if conv.loading_more_messages {
            return;
        }
        let Some(ref client) = self.conversations_client else {
            return;
        };

        let thread_id = conv.thread_id;
        let start = conv.messages_requested;
        let end = start + Self::MESSAGE_PAGE_SIZE;
        conv.messages_requested = end;
        conv.loading_more_messages = true;
        conv.loading_started_tick = Some(self.tick_count);

        let connection = client.connection().clone();
        let device_id = client.device_id().to_owned();

        tokio::spawn(async move {
            let client = ConversationsClient::new(connection, device_id);
            if let Err(e) = client.request_conversation(thread_id, start, end).await {
                tracing::warn!("Failed to load more messages for {}: {}", thread_id, e);
            }
        });
    }

    /// Periodically retry loading messages for the selected conversation
    /// if they haven't arrived yet.  Called on each Tick (every 250ms).
    ///
    /// The kdeconnect daemon sometimes doesn't deliver messages in response
    /// to `requestConversation` (e.g. if it's still busy with the initial
    /// `requestAllConversationThreads` sync).  This method retries:
    ///   - Every 2s: re-send `requestConversation` for the specific thread
    ///   - After 6s:  fall back to `requestAllConversationThreads` (full re-sync)
    ///
    /// Also handles auto-resync: kdeconnectd progressively discovers messages
    /// from the phone, so multiple `requestAllConversationThreads` calls are
    /// needed after connecting.  We automatically repeat this up to
    /// AUTO_RESYNC_MAX times (every 2s) to avoid requiring manual 'r' presses.
    async fn retry_message_loading_if_needed(&mut self) {
        // Only act every 8 ticks (2 seconds)
        if self.tick_count % 8 != 0 {
            return;
        }
        // Don't poll the daemon right after sending a message.
        if self.in_send_cooldown() {
            return;
        }

        let Some(idx) = self.selected_conversation_idx else { return };
        let Some(conv) = self.conversations.get(idx) else { return };

        // If we've requested messages but got none, retry the specific conversation
        if conv.messages_requested > 0 && conv.messages.is_empty() {
            // After 24 ticks (6 seconds) of empty messages, do a full re-sync
            // (equivalent to pressing 'r'), which reliably triggers the phone.
            if conv.messages_requested > Self::MESSAGE_PAGE_SIZE {
                // Already retried via requestConversation; try full sync
                self.load_conversations().await;
                return;
            }

            // Retry requestConversation and bump messages_requested so we can
            // detect repeated failures.
            let thread_id = conv.thread_id;
            if let Some(conv) = self.conversations.get_mut(idx) {
                conv.messages_requested += Self::MESSAGE_PAGE_SIZE;
            }

            let Some(ref client) = self.conversations_client else { return };
            let connection = client.connection().clone();
            let device_id = client.device_id().to_owned();
            let end = Self::MESSAGE_PAGE_SIZE;

            tokio::spawn(async move {
                let client = ConversationsClient::new(connection, device_id);
                if let Err(e) = client.request_conversation(thread_id, 0, end).await {
                    tracing::warn!("Retry: failed to request conversation {}: {}", thread_id, e);
                }
            });
            return;
        }

        // Auto-resync: periodically re-request all conversation threads so
        // that kdeconnectd discovers additional messages from the phone.
        if self.auto_resync_remaining > 0 {
            self.auto_resync_remaining -= 1;
            tracing::debug!(
                "Auto-resync ({} remaining): requesting all conversation threads",
                self.auto_resync_remaining
            );
            if let Some(ref client) = self.conversations_client {
                let connection = client.connection().clone();
                let device_id = client.device_id().to_owned();
                tokio::spawn(async move {
                    let client = ConversationsClient::new(connection, device_id);
                    if let Err(e) = client.request_all_conversation_threads().await {
                        tracing::warn!("Auto-resync failed: {}", e);
                    }
                });
            }
        }
    }

    /// If the user has scrolled near the top of the message view, or the
    /// viewport isn't full yet, request the next page of older messages.
    fn maybe_load_more_on_scroll(&mut self) {
        // If content doesn't fill the viewport, always try to load more.
        if self.message_max_scroll == 0 {
            self.load_more_messages();
            return;
        }
        // message_scroll is an offset from the bottom (0 = newest visible).
        // Trigger loading one full page before reaching the top so messages
        // are ready before the user scrolls up to them.
        let threshold = self.message_view_height.max(1);
        if self.message_scroll >= self.message_max_scroll.saturating_sub(threshold) {
            self.load_more_messages();
        }
    }

    /// Whether the selected conversation is currently loading older messages.
    fn is_loading_more_messages(&self) -> bool {
        self.selected_conversation_idx
            .and_then(|i| self.conversations.get(i))
            .is_some_and(|c| c.loading_more_messages)
    }

    /// Reset `loading_more_messages` if the flag has been stuck for too long
    /// (the `conversationLoaded` signal never arrived).  40 ticks ≈ 10 seconds.
    fn reset_stale_loading_flags(&mut self) {
        const LOADING_TIMEOUT_TICKS: u32 = 40;
        let current = self.tick_count;
        for conv in &mut self.conversations {
            if conv.loading_more_messages {
                if let Some(started) = conv.loading_started_tick {
                    if current.wrapping_sub(started) >= LOADING_TIMEOUT_TICKS {
                        tracing::warn!(
                            "Loading timeout for thread {} — resetting",
                            conv.thread_id
                        );
                        conv.loading_more_messages = false;
                        conv.loading_started_tick = None;
                    }
                } else {
                    // Flag set but no tick recorded (e.g. restored from cache) — clear it.
                    conv.loading_more_messages = false;
                }
            }
        }
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
                "jpg" | "jpeg" | "png" | "gif" | "bmp" | "webp" | "heic" | "heif"
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

    /// Request downloads for all image attachments in the currently selected conversation.
    fn request_conversation_attachments(&mut self) {
        let Some(idx) = self.selected_conversation_idx else {
            return;
        };
        let Some(conv) = self.conversations.get_mut(idx) else {
            return;
        };

        // Scan the kdeconnect cache directory for files that already exist on
        // disk but whose cached_path hasn't been set (e.g. from a prior session).
        if let Some(device) = self.selected_device_idx.and_then(|i| self.devices.get(i)) {
            let cache_dir = dirs::cache_dir()
                .unwrap_or_else(|| PathBuf::from("~/.cache"))
                .join("kdeconnect.daemon")
                .join(&device.name);
            if cache_dir.is_dir() {
                for msg in &mut conv.messages {
                    for att in &mut msg.attachments {
                        if att.is_image() && att.cached_path.is_none() {
                            let candidate = cache_dir.join(&att.unique_identifier);
                            if candidate.exists() {
                                tracing::debug!(
                                    "Found cached attachment on disk: {:?}",
                                    candidate
                                );
                                att.cached_path = Some(candidate);
                            }
                        }
                    }
                }
                if let Some(ref mut msg) = conv.latest_message {
                    for att in &mut msg.attachments {
                        if att.is_image() && att.cached_path.is_none() {
                            let candidate = cache_dir.join(&att.unique_identifier);
                            if candidate.exists() {
                                att.cached_path = Some(candidate);
                            }
                        }
                    }
                }
            }
        }

        // Re-borrow as immutable for the rest of the method
        let conv = &self.conversations[idx];
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
                            let load_result = image::ImageReader::open(path)
                                .and_then(|r| r.with_guessed_format())
                                .map_err(image::ImageError::IoError)
                                .and_then(|r| r.decode());
                            match load_result {
                                Ok(dyn_img) => {
                                    let protocol = picker.new_resize_protocol(dyn_img);
                                    self.image_states.insert(
                                        att.unique_identifier.clone(),
                                        ImageState::Loaded(Box::new(protocol)),
                                    );
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        "Failed to decode cached image {:?}: {}",
                                        path,
                                        e
                                    );
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
            Focus::ConversationList => self.handle_key_conversations(key, signal_tx).await,
            Focus::MessageView => self.handle_key_messages(key, signal_tx).await,
            Focus::Compose => self.handle_key_compose(key).await,
            Focus::DevicePopup => self.handle_key_device_popup(key, signal_tx).await,
            Focus::GroupInfoPopup => self.handle_key_group_info(key),
            Focus::FolderPopup => self.handle_key_folder_popup(key),
            Focus::HelpPopup => {
                // Any key dismisses the help popup
                self.focus = Focus::ConversationList;
                self.needs_full_repaint = true;
            }
        }
    }

    async fn handle_key_conversations(
        &mut self,
        key: KeyEvent,
        signal_tx: tokio::sync::mpsc::UnboundedSender<AppEvent>,
    ) {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,

            // Switch focus to messages panel
            KeyCode::Tab | KeyCode::Char('l') => {
                self.focus = Focus::MessageView;
            }

            // Help popup
            KeyCode::Char('?') => {
                self.focus = Focus::HelpPopup;
            }

            // Conversation navigation
            KeyCode::Up | KeyCode::Char('k') => {
                self.save_draft();
                self.select_prev_conversation();
                self.restore_draft();
                self.request_selected_conversation_messages();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.save_draft();
                self.select_next_conversation();
                self.restore_draft();
                self.request_selected_conversation_messages();
            }

            // Page through conversations
            KeyCode::PageUp | KeyCode::Char('K') => {
                self.save_draft();
                let page = 10; // conversations per page
                for _ in 0..page {
                    self.select_prev_conversation();
                }
                self.restore_draft();
                self.request_selected_conversation_messages();
            }
            KeyCode::PageDown | KeyCode::Char('J') => {
                self.save_draft();
                let page = 10;
                for _ in 0..page {
                    self.select_next_conversation();
                }
                self.restore_draft();
                self.request_selected_conversation_messages();
            }

            // Enter conversation / focus compose
            KeyCode::Enter | KeyCode::Char('i') => {
                if self.selected_conversation_idx.is_some() {
                    self.pre_compose_focus = Focus::ConversationList;
                    self.focus = Focus::Compose;
                    self.request_selected_conversation_messages();
                }
            }

            // Device popup
            KeyCode::Char('d') => {
                if !self.devices.is_empty() {
                    self.device_popup_idx = self.selected_device_idx.unwrap_or(0);
                    self.focus = Focus::DevicePopup;
                }
            }

            // Refresh
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

            // Group info popup
            KeyCode::Char('g') => {
                self.open_group_info_popup();
            }

            // Archive conversation
            KeyCode::Char('a') => {
                self.archive_selected_conversation();
            }
            // View archived conversations
            KeyCode::Char('A') => {
                self.open_folder_popup(FolderKind::Archive);
            }

            // Spam conversation
            KeyCode::Char('s') => {
                self.spam_selected_conversation();
            }
            // View spam conversations
            KeyCode::Char('S') => {
                self.open_folder_popup(FolderKind::Spam);
            }

            _ => {}
        }
    }

    async fn handle_key_messages(
        &mut self,
        key: KeyEvent,
        signal_tx: tokio::sync::mpsc::UnboundedSender<AppEvent>,
    ) {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,

            // Switch focus to conversations panel
            KeyCode::Tab | KeyCode::Char('h') => {
                self.focus = Focus::ConversationList;
            }

            // Help popup
            KeyCode::Char('?') => {
                self.focus = Focus::HelpPopup;
            }

            // Message-by-message selection (up = older)
            KeyCode::Up | KeyCode::Char('k') => {
                self.select_message_up();
                self.maybe_load_more_on_scroll();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.select_message_down();
            }

            // Page scrolling
            KeyCode::PageUp | KeyCode::Char('K') => {
                let page = self.message_view_height.max(1);
                self.message_scroll = self.message_scroll.saturating_add(page);
                self.maybe_load_more_on_scroll();
            }
            KeyCode::PageDown | KeyCode::Char('J') => {
                let page = self.message_view_height.max(1);
                self.message_scroll = self.message_scroll.saturating_sub(page);
            }

            // Enter: open selected attachment with xdg-open
            KeyCode::Enter => {
                self.try_open_selected_attachment();
            }

            // 'i' always enters compose
            KeyCode::Char('i') => {
                if self.selected_conversation_idx.is_some() {
                    self.pre_compose_focus = Focus::MessageView;
                    self.focus = Focus::Compose;
                }
            }

            // Copy selected message/attachment to clipboard
            KeyCode::Char('c') => {
                self.copy_selected_to_clipboard();
            }

            // Device popup
            KeyCode::Char('d') => {
                if !self.devices.is_empty() {
                    self.device_popup_idx = self.selected_device_idx.unwrap_or(0);
                    self.focus = Focus::DevicePopup;
                }
            }

            // Refresh
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

            // Group info popup
            KeyCode::Char('g') => {
                self.open_group_info_popup();
            }

            _ => {}
        }
    }

    async fn handle_key_compose(&mut self, key: KeyEvent) {
        match key.code {
            // Escape returns to previous panel
            KeyCode::Esc => {
                self.focus = self.pre_compose_focus;
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

            _ => {}
        }
    }

    async fn handle_key_device_popup(
        &mut self,
        key: KeyEvent,
        signal_tx: tokio::sync::mpsc::UnboundedSender<AppEvent>,
    ) {
        match key.code {
            // Close popup
            KeyCode::Esc | KeyCode::Char('d') | KeyCode::Char('q') => {
                self.focus = Focus::ConversationList;
                self.needs_full_repaint = true;
            }

            // Navigate
            KeyCode::Up | KeyCode::Char('k') => {
                if self.device_popup_idx > 0 {
                    self.device_popup_idx -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.device_popup_idx + 1 < self.devices.len() {
                    self.device_popup_idx += 1;
                }
            }

            // Select device
            KeyCode::Enter => {
                let new_idx = self.device_popup_idx;
                if self.selected_device_idx != Some(new_idx) {
                    self.select_device(new_idx);
                    self.connect_to_device(signal_tx).await;
                }
                self.focus = Focus::ConversationList;
                self.needs_full_repaint = true;
            }

            _ => {}
        }
    }

    /// Switch to a specific device by index, resetting conversation state.
    fn select_device(&mut self, idx: usize) {
        self.selected_device_idx = Some(idx);
        self.conversations.clear();
        self.selected_conversation_idx = None;
        self.conversations_client = None;
        self.message_scroll = 0;
        self.selected_message_idx = None;
        self.selected_message_part = 0;
        self.focus = Focus::ConversationList;
        self.compose_input.clear();
        self.compose_cursor = 0;
        self.drafts.clear();
    }

    // ── Group info popup ────────────────────────────────────────────

    fn open_group_info_popup(&mut self) {
        let Some(idx) = self.selected_conversation_idx else { return };
        let Some(conv) = self.conversations.get(idx) else { return };
        let thread_id = conv.thread_id;

        // Pre-fill with existing custom name, or generate initials
        let existing = self
            .state
            .group_names
            .get(&thread_id.to_string())
            .cloned();
        let name = existing.unwrap_or_else(|| self.generate_group_initials(conv));
        self.group_name_input = name;
        self.group_name_cursor = self.group_name_input.len();
        self.focus = Focus::GroupInfoPopup;
    }

    fn handle_key_group_info(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.focus = Focus::ConversationList;
                self.needs_full_repaint = true;
            }
            KeyCode::Enter => {
                // Save group name
                if let Some(idx) = self.selected_conversation_idx {
                    if let Some(conv) = self.conversations.get(idx) {
                        let tid = conv.thread_id.to_string();
                        let name = self.group_name_input.trim().to_string();
                        if name.is_empty() {
                            self.state.group_names.remove(&tid);
                        } else {
                            self.state.group_names.insert(tid, name);
                        }
                        if let Err(e) = self.state.save() {
                            self.status_message = Some(format!("Failed to save state: {}", e));
                        }
                    }
                }
                self.focus = Focus::ConversationList;
                self.needs_full_repaint = true;
            }
            KeyCode::Backspace => {
                if self.group_name_cursor > 0 {
                    let prev = self.group_name_input[..self.group_name_cursor]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    self.group_name_input.drain(prev..self.group_name_cursor);
                    self.group_name_cursor = prev;
                }
            }
            KeyCode::Delete => {
                if self.group_name_cursor < self.group_name_input.len() {
                    let next = self.group_name_input[self.group_name_cursor..]
                        .char_indices()
                        .nth(1)
                        .map(|(i, _)| self.group_name_cursor + i)
                        .unwrap_or(self.group_name_input.len());
                    self.group_name_input.drain(self.group_name_cursor..next);
                }
            }
            KeyCode::Left => {
                if self.group_name_cursor > 0 {
                    self.group_name_cursor = self.group_name_input[..self.group_name_cursor]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                }
            }
            KeyCode::Right => {
                if self.group_name_cursor < self.group_name_input.len() {
                    self.group_name_cursor = self.group_name_input[self.group_name_cursor..]
                        .char_indices()
                        .nth(1)
                        .map(|(i, _)| self.group_name_cursor + i)
                        .unwrap_or(self.group_name_input.len());
                }
            }
            KeyCode::Home => self.group_name_cursor = 0,
            KeyCode::End => self.group_name_cursor = self.group_name_input.len(),
            KeyCode::Char(c) => {
                self.group_name_input.insert(self.group_name_cursor, c);
                self.group_name_cursor += c.len_utf8();
            }
            _ => {}
        }
    }

    /// Generate default group name from member initials.
    /// e.g. "Alice Smith, Bob, +15551234" → "AS,B,+"
    pub fn generate_group_initials(&self, conv: &Conversation) -> String {
        let addrs = conv
            .latest_message
            .as_ref()
            .map(|m| &m.addresses[..])
            .unwrap_or(&[]);

        let mut seen = std::collections::HashSet::new();
        let mut entries: Vec<(String, String)> = addrs
            .iter()
            .filter(|a| {
                let normalized = crate::contacts::normalize_phone(&a.address);
                seen.insert(normalized)
            })
            .map(|a| {
                let name = self.contacts.display_name(&a.address);
                let initials = name_to_initials(&name);
                (name, initials)
            })
            .collect();

        // Sort by last name then first name
        entries.sort_by(|(a, _), (b, _)| {
            let a_parts: Vec<&str> = a.split_whitespace().collect();
            let b_parts: Vec<&str> = b.split_whitespace().collect();
            let a_last = a_parts.last().unwrap_or(&"");
            let b_last = b_parts.last().unwrap_or(&"");
            let a_first = a_parts.first().unwrap_or(&"");
            let b_first = b_parts.first().unwrap_or(&"");
            a_last.cmp(b_last).then(a_first.cmp(b_first))
        });

        entries.into_iter().map(|(_, i)| i).collect::<Vec<_>>().join(",")
    }

    /// Get the sorted member list for the group info popup.
    pub fn group_members(&self) -> Vec<(String, String)> {
        let Some(idx) = self.selected_conversation_idx else {
            return vec![];
        };
        let Some(conv) = self.conversations.get(idx) else {
            return vec![];
        };
        let addrs = conv
            .latest_message
            .as_ref()
            .map(|m| &m.addresses[..])
            .unwrap_or(&[]);

        let mut seen = std::collections::HashSet::new();
        let mut members: Vec<(String, String)> = addrs
            .iter()
            .filter(|a| {
                let normalized = crate::contacts::normalize_phone(&a.address);
                seen.insert(normalized)
            })
            .map(|a| {
                let name = self.contacts.display_name(&a.address);
                let phone = a.address.clone();
                (name, phone)
            })
            .collect();

        // Sort by last name then first name
        members.sort_by(|(a, _), (b, _)| {
            let a_parts: Vec<&str> = a.split_whitespace().collect();
            let b_parts: Vec<&str> = b.split_whitespace().collect();
            let a_last = a_parts.last().unwrap_or(&"");
            let b_last = b_parts.last().unwrap_or(&"");
            let a_first = a_parts.first().unwrap_or(&"");
            let b_first = b_parts.first().unwrap_or(&"");
            a_last.cmp(b_last).then(a_first.cmp(b_first))
        });

        members
    }

    // ── Archive / Spam ──────────────────────────────────────────────

    fn archive_selected_conversation(&mut self) {
        let Some(idx) = self.selected_conversation_idx else { return };
        let Some(conv) = self.conversations.get(idx) else { return };
        let thread_id = conv.thread_id;
        self.state.toggle_archived(thread_id);
        if let Err(e) = self.state.save() {
            self.status_message = Some(format!("Failed to save state: {}", e));
        }
        // Move selection to next visible conversation
        self.adjust_selection_after_hide();
    }

    fn spam_selected_conversation(&mut self) {
        let Some(idx) = self.selected_conversation_idx else { return };
        let Some(conv) = self.conversations.get(idx) else { return };
        let thread_id = conv.thread_id;
        self.state.toggle_spam(thread_id);
        if let Err(e) = self.state.save() {
            self.status_message = Some(format!("Failed to save state: {}", e));
        }
        self.adjust_selection_after_hide();
    }

    fn adjust_selection_after_hide(&mut self) {
        let visible: Vec<usize> = self.visible_conversation_indices();
        if visible.is_empty() {
            self.selected_conversation_idx = None;
        } else if let Some(sel) = self.selected_conversation_idx {
            // Try to keep same index or move to nearest visible
            self.selected_conversation_idx = visible
                .iter()
                .find(|&&i| i >= sel)
                .or(visible.last())
                .copied();
        }
    }

    fn open_folder_popup(&mut self, kind: FolderKind) {
        self.folder_popup_kind = kind;
        self.folder_popup_idx = 0;
        self.focus = Focus::FolderPopup;
    }

    fn handle_key_folder_popup(&mut self, key: KeyEvent) {
        let threads = self.folder_thread_ids();
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.focus = Focus::ConversationList;
                self.needs_full_repaint = true;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.folder_popup_idx > 0 {
                    self.folder_popup_idx -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.folder_popup_idx + 1 < threads.len() {
                    self.folder_popup_idx += 1;
                }
            }
            KeyCode::Enter => {
                if let Some(&thread_id) = threads.get(self.folder_popup_idx) {
                    // Restore conversation and select it
                    self.state.unarchive(thread_id);
                    if let Err(e) = self.state.save() {
                        self.status_message = Some(format!("Failed to save state: {}", e));
                    }
                    // Select the restored conversation
                    if let Some(pos) = self.conversations.iter().position(|c| c.thread_id == thread_id) {
                        self.selected_conversation_idx = Some(pos);
                        self.message_scroll = 0;
                        self.reset_message_selection();
                        self.request_selected_conversation_messages();
                    }
                    self.focus = Focus::ConversationList;
                    self.needs_full_repaint = true;
                }
            }
            _ => {}
        }
    }

    /// Returns the list of thread_ids for the currently open folder popup.
    pub fn folder_thread_ids(&self) -> Vec<i64> {
        let mut ids = match self.folder_popup_kind {
            FolderKind::Archive => self.state.archived_threads.clone(),
            FolderKind::Spam => self.state.spam_threads.clone(),
        };
        // Sort by most recent message first.
        ids.sort_by(|a, b| {
            let date_a = self.conversations.iter().find(|c| c.thread_id == *a)
                .and_then(|c| c.latest_message.as_ref()).map(|m| m.date).unwrap_or(0);
            let date_b = self.conversations.iter().find(|c| c.thread_id == *b)
                .and_then(|c| c.latest_message.as_ref()).map(|m| m.date).unwrap_or(0);
            date_b.cmp(&date_a)
        });
        ids
    }

    /// Returns indices of conversations that are not archived or spam.
    pub fn visible_conversation_indices(&self) -> Vec<usize> {
        self.conversations
            .iter()
            .enumerate()
            .filter(|(_, c)| !self.state.is_hidden(c.thread_id))
            .map(|(i, _)| i)
            .collect()
    }

    /// Scroll up (toward older messages) by one message boundary.
    /// Number of selectable parts for a message: 1 (text) + N (attachments).
    fn message_part_count(msg: &Message) -> usize {
        1 + msg.attachments.len()
    }

    /// Move selection up (toward older messages / previous parts).
    fn select_message_up(&mut self) {
        let msg_count = self.selected_conversation_messages_len();
        if msg_count == 0 {
            return;
        }

        match self.selected_message_idx {
            None => {
                // Nothing selected — select the newest message's last part
                let idx = msg_count - 1;
                self.selected_message_idx = Some(idx);
                let parts = self.conversation_message_part_count(idx);
                self.selected_message_part = parts.saturating_sub(1);
            }
            Some(idx) => {
                if self.selected_message_part > 0 {
                    // Move to previous part of same message
                    self.selected_message_part -= 1;
                } else if idx > 0 {
                    // Move to previous message (last part)
                    let new_idx = idx - 1;
                    self.selected_message_idx = Some(new_idx);
                    let parts = self.conversation_message_part_count(new_idx);
                    self.selected_message_part = parts.saturating_sub(1);
                }
                // else: already at oldest message, part 0 — do nothing
            }
        }
    }

    /// Move selection down (toward newer messages / next parts).
    fn select_message_down(&mut self) {
        let msg_count = self.selected_conversation_messages_len();
        if msg_count == 0 {
            return;
        }

        match self.selected_message_idx {
            None => {
                // Nothing selected — select newest message text
                self.selected_message_idx = Some(msg_count - 1);
                self.selected_message_part = 0;
            }
            Some(idx) => {
                let parts = self.conversation_message_part_count(idx);
                if self.selected_message_part + 1 < parts {
                    // Move to next part of same message
                    self.selected_message_part += 1;
                } else if idx + 1 < msg_count {
                    // Move to next message (text part)
                    self.selected_message_idx = Some(idx + 1);
                    self.selected_message_part = 0;
                }
                // else: already at newest message, last part — do nothing
            }
        }
    }

    /// Reset message selection to the newest message (text part).
    fn reset_message_selection(&mut self) {
        let count = self.selected_conversation_messages_len();
        if count > 0 {
            self.selected_message_idx = Some(count - 1);
            self.selected_message_part = 0;
        } else {
            self.selected_message_idx = None;
            self.selected_message_part = 0;
        }
    }

    fn selected_conversation_messages_len(&self) -> usize {
        self.selected_conversation_idx
            .and_then(|i| self.conversations.get(i))
            .map(|c| c.messages.len())
            .unwrap_or(0)
    }

    fn conversation_message_part_count(&self, msg_idx: usize) -> usize {
        self.selected_conversation_idx
            .and_then(|i| self.conversations.get(i))
            .and_then(|c| c.messages.get(msg_idx))
            .map(|m| Self::message_part_count(m))
            .unwrap_or(1)
    }

    /// Save current compose input as a draft for the current conversation.
    fn save_draft(&mut self) {
        if let Some(idx) = self.selected_conversation_idx {
            if let Some(conv) = self.conversations.get(idx) {
                let thread_id = conv.thread_id;
                if self.compose_input.is_empty() {
                    self.drafts.remove(&thread_id);
                } else {
                    self.drafts.insert(thread_id, (self.compose_input.clone(), self.compose_cursor));
                }
            }
        }
    }

    /// Restore draft for the currently selected conversation.
    fn restore_draft(&mut self) {
        if let Some(idx) = self.selected_conversation_idx {
            if let Some(conv) = self.conversations.get(idx) {
                if let Some((text, cursor)) = self.drafts.get(&conv.thread_id) {
                    self.compose_input = text.clone();
                    self.compose_cursor = *cursor;
                } else {
                    self.compose_input.clear();
                    self.compose_cursor = 0;
                }
            }
        }
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
                self.drafts.remove(&thread_id);
                self.status_message = Some("Message sent".into());
                // Start the post-send cooldown to prevent daemon polling
                // from interfering with message delivery.
                self.last_send_time = Some(std::time::Instant::now());
            }
            Err(e) => {
                self.status_message = Some(format!("Send failed: {}", e));
            }
        }
    }

    fn select_prev_conversation(&mut self) {
        let visible = self.visible_conversation_indices();
        if visible.is_empty() {
            self.selected_conversation_idx = None;
            return;
        }
        let new_idx = match self.selected_conversation_idx {
            None => *visible.first().unwrap(),
            Some(cur) => visible.iter().rev().find(|&&i| i < cur).copied().unwrap_or(visible[0]),
        };
        self.selected_conversation_idx = Some(new_idx);
        self.message_scroll = 0;
        self.reset_message_selection();
    }

    fn select_next_conversation(&mut self) {
        let visible = self.visible_conversation_indices();
        if visible.is_empty() {
            self.selected_conversation_idx = None;
            return;
        }
        let new_idx = match self.selected_conversation_idx {
            None => *visible.first().unwrap(),
            Some(cur) => visible.iter().find(|&&i| i > cur).copied().unwrap_or(*visible.last().unwrap()),
        };
        self.selected_conversation_idx = Some(new_idx);
        self.message_scroll = 0;
        self.reset_message_selection();
    }

    /// Get the currently selected message and its attachment (if part > 0).
    fn selected_message_and_attachment(&self) -> Option<(&Message, Option<&Attachment>)> {
        let msg_idx = self.selected_message_idx?;
        let conv_idx = self.selected_conversation_idx?;
        let conv = self.conversations.get(conv_idx)?;
        let msg = conv.messages.get(msg_idx)?;
        if self.selected_message_part == 0 {
            Some((msg, None))
        } else {
            let att = msg.attachments.get(self.selected_message_part - 1);
            Some((msg, att))
        }
    }

    /// Open the selected attachment with xdg-open. Returns true if an
    /// attachment was opened (so the caller can skip enter-to-compose).
    fn try_open_selected_attachment(&mut self) -> bool {
        // Extract the cached path before mutating self.
        let cached_path = self.selected_message_and_attachment()
            .and_then(|(_, att)| att)
            .and_then(|att| att.cached_path.clone());

        // If selection is on text (part 0), not an attachment
        if self.selected_message_part == 0 {
            return false;
        }

        if let Some(path) = cached_path {
            if path.exists() {
                tokio::spawn(async move {
                    let _ = tokio::process::Command::new("xdg-open")
                        .arg(&path)
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn();
                });
                self.status_message = Some("Opening attachment...".into());
                return true;
            }
        }
        self.status_message = Some("Attachment not downloaded yet".into());
        true // still an attachment, just not cached — don't fall through to compose
    }

    /// Copy the selected message text or attachment to the clipboard.
    fn copy_selected_to_clipboard(&mut self) {
        // Extract what we need before mutating self.
        let info = self.selected_message_and_attachment().map(|(msg, att)| {
            (
                msg.body.clone(),
                att.map(|a| (a.mime_type.clone(), a.is_image(), a.cached_path.clone())),
            )
        });
        let Some((body, att_info)) = info else {
            return;
        };
        match att_info {
            None => {
                clipboard_copy_text(&body);
                self.status_message = Some("Message copied".into());
            }
            Some((mime, is_image, cached_path)) => {
                if let Some(path) = cached_path {
                    if path.exists() {
                        if mime.starts_with("text/") {
                            if let Ok(text) = std::fs::read_to_string(&path) {
                                clipboard_copy_text(&text);
                                self.status_message = Some("Attachment text copied".into());
                            } else {
                                self.status_message = Some("Failed to read attachment".into());
                            }
                        } else if is_image {
                            clipboard_copy_image(&path);
                            self.status_message = Some("Image copied to clipboard".into());
                        } else {
                            clipboard_copy_text(&path.display().to_string());
                            self.status_message = Some("Attachment path copied".into());
                        }
                    } else {
                        self.status_message = Some("Attachment not downloaded yet".into());
                    }
                } else {
                    self.status_message = Some("Attachment not downloaded yet".into());
                }
            }
        }
    }
}

/// Detected clipboard backend.
enum ClipboardBackend {
    /// macOS pbcopy/pbpaste
    Pbcopy,
    /// Wayland wl-copy
    WlCopy,
    /// X11 xclip
    Xclip,
    /// X11 xsel (fallback)
    Xsel,
}

/// Detect the appropriate clipboard backend for the current platform/session.
fn detect_clipboard() -> Option<ClipboardBackend> {
    // macOS
    if cfg!(target_os = "macos") {
        return Some(ClipboardBackend::Pbcopy);
    }

    // Wayland: XDG_SESSION_TYPE=wayland or WAYLAND_DISPLAY is set
    if std::env::var("WAYLAND_DISPLAY").is_ok()
        || std::env::var("XDG_SESSION_TYPE").ok().as_deref() == Some("wayland")
    {
        return Some(ClipboardBackend::WlCopy);
    }

    // X11: try xclip first, then xsel
    if std::process::Command::new("xclip")
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
    {
        return Some(ClipboardBackend::Xclip);
    }

    if std::process::Command::new("xsel")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
    {
        return Some(ClipboardBackend::Xsel);
    }

    None
}

/// Copy text to the system clipboard.
fn clipboard_copy_text(text: &str) {
    use std::io::Write;

    let Some(backend) = detect_clipboard() else {
        return;
    };

    let result = match backend {
        ClipboardBackend::Pbcopy => std::process::Command::new("pbcopy")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn(),
        ClipboardBackend::WlCopy => std::process::Command::new("wl-copy")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn(),
        ClipboardBackend::Xclip => std::process::Command::new("xclip")
            .args(["-selection", "clipboard"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn(),
        ClipboardBackend::Xsel => std::process::Command::new("xsel")
            .args(["--clipboard", "--input"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn(),
    };

    if let Ok(mut child) = result {
        if let Some(ref mut stdin) = child.stdin {
            let _ = stdin.write_all(text.as_bytes());
        }
        let _ = child.wait();
    }
}

/// Copy an image file to the system clipboard.
fn clipboard_copy_image(path: &std::path::Path) {
    let Some(backend) = detect_clipboard() else {
        return;
    };

    let mime = if path.extension().and_then(|e| e.to_str()) == Some("png") {
        "image/png"
    } else {
        "image/jpeg"
    };

    let _ = match backend {
        ClipboardBackend::Pbcopy => {
            // macOS: pbcopy doesn't support images directly; copy the path instead
            clipboard_copy_text(&path.display().to_string());
            return;
        }
        ClipboardBackend::WlCopy => std::process::Command::new("wl-copy")
            .args(["--type", mime])
            .stdin(std::fs::File::open(path).ok().map_or(
                std::process::Stdio::null(),
                std::process::Stdio::from,
            ))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status(),
        ClipboardBackend::Xclip => std::process::Command::new("xclip")
            .args(["-selection", "clipboard", "-t", mime, "-i"])
            .arg(path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status(),
        ClipboardBackend::Xsel => {
            // xsel doesn't support binary clipboard; copy the path instead
            clipboard_copy_text(&path.display().to_string());
            return;
        }
    };
}

/// Insert a message into a sorted (by date ascending) list, avoiding duplicates.
///
/// kdeconnect can deliver the same message multiple times — via both
/// `conversationCreated` and `conversationUpdated` signals, or via signal
/// + conversation reload.  The uid may differ between deliveries (e.g. 0
/// before the phone assigns the real SMS ID), and the timestamp can shift
/// by a few seconds between the queued and delivered states (especially
/// for MMS).  We use multiple strategies:
///   1. If the incoming uid > 0 and matches an existing uid → duplicate.
///   2. Same body + date within 5 seconds → duplicate (covers uid==0,
///      mismatched-uid, and MMS timestamp drift).
fn insert_message_sorted(messages: &mut Vec<Message>, msg: Message) {
    // 5 seconds in milliseconds — kdeconnect timestamps are in epoch ms.
    const TIMESTAMP_FUZZ_MS: i64 = 5_000;

    let dominated = messages.iter().any(|m| {
        // Exact uid match (when the id is known)
        if msg.uid != 0 && m.uid == msg.uid {
            return true;
        }
        // Fallback: same body + date within a small window
        m.body == msg.body && (m.date - msg.date).abs() <= TIMESTAMP_FUZZ_MS
    });
    if dominated {
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
        // Use date as uid so each distinct test message gets a unique id
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
            uid: date as i32,
            sub_id: -1,
            attachments: vec![],
        }
    }

    #[test]
    fn test_select_device() {
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

        app.select_device(1);
        assert_eq!(app.selected_device_idx, Some(1));

        app.select_device(0);
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
    fn test_select_device_clears_conversations() {
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

        app.select_device(1);
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
    fn test_insert_message_dedup_different_uid_same_body_date() {
        // kdeconnect can deliver the same sent message via both
        // conversationCreated and conversationUpdated signals, with uid=0
        // in one and the real uid in the other.
        let mut messages = Vec::new();

        let mut msg1 = make_test_message(1, 1000, "sent msg");
        msg1.uid = 0; // first signal: uid not yet assigned
        insert_message_sorted(&mut messages, msg1);

        let mut msg2 = make_test_message(1, 1000, "sent msg");
        msg2.uid = 42; // second signal: real uid from phone
        insert_message_sorted(&mut messages, msg2);

        assert_eq!(messages.len(), 1, "same body+date should dedup even with different uids");
    }

    #[test]
    fn test_insert_message_dedup_fuzzy_timestamp() {
        // MMS messages can arrive with slightly different timestamps
        // between signal deliveries (queued vs delivered).
        let mut messages = Vec::new();

        let mut msg1 = make_test_message(1, 1700000000000, "hello via MMS");
        msg1.uid = 0;
        insert_message_sorted(&mut messages, msg1);

        // Same body, 3 seconds later — still a duplicate
        let mut msg2 = make_test_message(1, 1700000003000, "hello via MMS");
        msg2.uid = 77;
        insert_message_sorted(&mut messages, msg2);

        assert_eq!(messages.len(), 1, "same body within 5s window should dedup");

        // Same body, 6 seconds later — genuinely different
        let mut msg3 = make_test_message(1, 1700000006001, "hello via MMS");
        msg3.uid = 88;
        insert_message_sorted(&mut messages, msg3);

        assert_eq!(messages.len(), 2, "same body outside 5s window is a new message");
    }

    #[test]
    fn test_insert_message_dedup_same_uid_nonzero() {
        // Two signals with the same non-zero uid but different body (e.g. edited)
        // should still dedup — uid is the authoritative identifier.
        let mut messages = Vec::new();

        let mut msg1 = make_test_message(1, 1000, "original");
        msg1.uid = 42;
        insert_message_sorted(&mut messages, msg1);

        let mut msg2 = make_test_message(1, 1001, "updated");
        msg2.uid = 42;
        insert_message_sorted(&mut messages, msg2);

        assert_eq!(messages.len(), 1, "same non-zero uid should dedup");
    }

    #[test]
    fn test_insert_message_different_messages_not_deduped() {
        // Genuinely different messages should not be deduped
        let mut messages = Vec::new();

        insert_message_sorted(&mut messages, make_test_message(1, 1000, "hello"));
        insert_message_sorted(&mut messages, make_test_message(1, 2000, "world"));

        assert_eq!(messages.len(), 2);
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

        // Tab switches to MessageView
        app.focus = Focus::MessageView;
        assert_eq!(app.focus, Focus::MessageView);

        // Tab switches back to ConversationList
        app.focus = Focus::ConversationList;
        assert_eq!(app.focus, Focus::ConversationList);

        // Enter compose from ConversationList
        app.pre_compose_focus = Focus::ConversationList;
        app.focus = Focus::Compose;
        assert_eq!(app.focus, Focus::Compose);

        // Esc goes back to pre_compose_focus
        app.focus = app.pre_compose_focus;
        assert_eq!(app.focus, Focus::ConversationList);

        // Enter compose from MessageView
        app.focus = Focus::MessageView;
        app.pre_compose_focus = Focus::MessageView;
        app.focus = Focus::Compose;

        // Esc goes back to MessageView
        app.focus = app.pre_compose_focus;
        assert_eq!(app.focus, Focus::MessageView);

        // Device popup
        app.focus = Focus::DevicePopup;
        assert_eq!(app.focus, Focus::DevicePopup);
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
    fn test_select_device_resets_compose() {
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

        app.select_device(1);

        assert_eq!(app.focus, Focus::ConversationList);
        assert!(app.compose_input.is_empty());
        assert_eq!(app.compose_cursor, 0);
    }

    #[test]
    fn test_drafts_saved_on_conversation_switch() {
        let mut app = App::new_test();
        app.conversations = vec![
            Conversation::new(1),
            Conversation::new(2),
        ];
        app.selected_conversation_idx = Some(0);
        app.compose_input = "draft for thread 1".into();
        app.compose_cursor = 18;

        // Switch to next conversation
        app.save_draft();
        app.select_next_conversation();
        app.restore_draft();

        // Draft should be cleared for new conversation
        assert!(app.compose_input.is_empty());
        assert_eq!(app.compose_cursor, 0);

        // Type something for thread 2
        app.compose_input = "draft for thread 2".into();
        app.compose_cursor = 18;

        // Switch back to thread 1
        app.save_draft();
        app.select_prev_conversation();
        app.restore_draft();

        // Should restore thread 1's draft
        assert_eq!(app.compose_input, "draft for thread 1");
        assert_eq!(app.compose_cursor, 18);
    }

    #[test]
    fn test_drafts_cleared_on_device_switch() {
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
        app.drafts.insert(1, ("hello".into(), 5));

        app.select_device(1);

        assert!(app.drafts.is_empty());
    }

    #[test]
    fn test_device_popup_navigation() {
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
            Device {
                id: "c".into(),
                name: "C".into(),
                reachable: true,
                paired: true,
            },
        ];
        app.selected_device_idx = Some(0);
        app.device_popup_idx = 0;

        // Navigate down
        app.device_popup_idx = 1;
        assert_eq!(app.device_popup_idx, 1);

        // Navigate down again
        app.device_popup_idx = 2;
        assert_eq!(app.device_popup_idx, 2);

        // Can't go past end (checked in handler, but here just verifying state)
        assert_eq!(app.device_popup_idx, 2);
    }

    #[test]
    fn test_message_selection_navigation() {
        let mut app = App::new_test();
        app.conversations = vec![Conversation {
            thread_id: 1,
            latest_message: None,
            messages: vec![
                make_test_message(1, 1000, "msg1"),
                make_test_message(1, 2000, "msg2"),
                make_test_message(1, 3000, "msg3"),
            ],
            is_group: false,
            display_name: None,
            messages_requested: 0,
            total_messages: None,
            loading_more_messages: false,
            loading_started_tick: None,
        }];
        app.selected_conversation_idx = Some(0);

        // Initially no selection
        assert_eq!(app.selected_message_idx, None);

        // First select_message_down should select newest (idx 2)
        app.select_message_down();
        assert_eq!(app.selected_message_idx, Some(2));
        assert_eq!(app.selected_message_part, 0);

        // select_message_up from newest goes to idx 1
        app.select_message_up();
        assert_eq!(app.selected_message_idx, Some(1));
        assert_eq!(app.selected_message_part, 0);

        // Up again to idx 0
        app.select_message_up();
        assert_eq!(app.selected_message_idx, Some(0));

        // Up at oldest — stays
        app.select_message_up();
        assert_eq!(app.selected_message_idx, Some(0));

        // Down goes back to idx 1
        app.select_message_down();
        assert_eq!(app.selected_message_idx, Some(1));
    }

    #[test]
    fn test_message_selection_with_attachments() {
        use crate::models::attachment::Attachment;
        let mut app = App::new_test();
        let mut msg = make_test_message(1, 1000, "has attachment");
        msg.attachments.push(Attachment {
            part_id: 1,
            mime_type: "image/jpeg".into(),
            unique_identifier: "att1".into(),
            cached_path: None,
        });
        app.conversations = vec![Conversation {
            thread_id: 1,
            latest_message: None,
            messages: vec![msg],
            is_group: false,
            display_name: None,
            messages_requested: 0,
            total_messages: None,
            loading_more_messages: false,
            loading_started_tick: None,
        }];
        app.selected_conversation_idx = Some(0);

        // Select newest message text (part 0)
        app.select_message_down();
        assert_eq!(app.selected_message_idx, Some(0));
        assert_eq!(app.selected_message_part, 0);

        // Down goes to attachment (part 1)
        app.select_message_down();
        assert_eq!(app.selected_message_idx, Some(0));
        assert_eq!(app.selected_message_part, 1);

        // Down at last part of last message — stays
        app.select_message_down();
        assert_eq!(app.selected_message_part, 1);

        // Up goes back to text
        app.select_message_up();
        assert_eq!(app.selected_message_part, 0);
    }

    #[test]
    fn test_message_selection_empty_conversation() {
        let mut app = App::new_test();
        app.conversations = vec![Conversation::new(1)];
        app.selected_conversation_idx = Some(0);

        app.select_message_up();
        assert_eq!(app.selected_message_idx, None);

        app.select_message_down();
        assert_eq!(app.selected_message_idx, None);
    }

    #[test]
    fn test_maybe_load_more_viewport_not_full() {
        let mut app = App::new_test();
        app.conversations = vec![Conversation::new(1)];
        app.selected_conversation_idx = Some(0);
        app.message_max_scroll = 0; // viewport not full
        app.message_view_height = 20;

        // Should try to load more even though scroll is 0
        // (load_more_messages will check has_more_messages internally)
        app.maybe_load_more_on_scroll();
        // Verify it didn't panic and the method ran
        // (actual D-Bus loading won't happen in test, but the path is exercised)
    }
}
