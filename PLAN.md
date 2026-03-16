# kdeconnect-sms-tui — Implementation Plan

## Language Choice: Rust

**Recommendation: Rust** over Go, for these reasons:

| Factor | Rust | Go |
|--------|------|-----|
| **D-Bus** | `zbus` — pure Rust, async-native, excellent ergonomics, auto-generates proxies from XML introspection | `godbus` — works but callback-based, no async, less ergonomic |
| **TUI** | `ratatui` — the dominant TUI framework, massive ecosystem, very active | `bubbletea` — good but Elm-architecture can be verbose for complex state |
| **Inline images** | `ratatui-image` (v10+) — first-class ratatui widget, supports Sixel (foot), Kitty protocol, iTerm2, unicode halfblocks fallback | `rasterm` exists but no bubbletea integration; would need manual escape codes |
| **Async** | `tokio` — handles D-Bus signals, user input, and rendering concurrently | goroutines work but D-Bus signal handling is less clean |
| **Emoji** | Unicode works natively in ratatui with proper font support | Same |

The killer feature for Rust is `ratatui-image` which solves inline image display out of the box across foot, kitty, ghostty, wezterm, iTerm2, and xterm.

---

## KDE Connect D-Bus API Summary

### Protocol: SMS/MMS only (no RCS)

KDE Connect **does not support RCS** — this is [blocked by Android not exposing RCS APIs to third-party apps](https://bugs.kde.org/show_bug.cgi?id=464654). The app works with SMS and has basic MMS support (attachments, group messages).

### D-Bus Service

- **Bus**: Session bus
- **Service**: `org.kde.kdeconnect`
- **Daemon path**: `/modules/kdeconnect`
- **Device path**: `/modules/kdeconnect/devices/<deviceId>`
- **Conversations interface**: `org.kde.kdeconnect.device.conversations`

### Key Methods (org.kde.kdeconnect.device.conversations)

```
activeConversations() -> QVariantList
requestConversation(conversationID: i64, start: i32, end: i32)
replyToConversation(conversationID: i64, message: String, attachmentUrls: List)
sendWithoutConversation(addressList: List, message: String, attachmentUrls: List)
requestAllConversationThreads()
requestAttachmentFile(partID: i64, uniqueIdentifier: String)
```

### Signals

```
conversationCreated(msg: Variant)
conversationRemoved(conversationID: i64)
conversationUpdated(msg: Variant)
conversationLoaded(conversationID: i64, messageCount: u64)
attachmentReceived(filePath: String, fileName: String)
```

### Device Management (org.kde.kdeconnect.daemon)

```
devices(onlyReachable: bool, onlyPaired: bool) -> StringList
deviceNames(onlyReachable: bool, onlyPaired: bool) -> Map<String,String>
```

### CLI Reference

```bash
kdeconnect-cli -l                          # list devices
kdeconnect-cli -a --id-only                # available device IDs
kdeconnect-cli -d <id> --send-sms "msg" --destination "+1234567890"
kdeconnect-cli --name "Phone" --send-sms "msg" --destination "+1234567890" --attachment /path/to/file
```

---

## Architecture

```
┌─────────────────────────────────────────────────┐
│                   TUI Layer                      │
│  ┌──────────┐  ┌──────────────┐  ┌───────────┐ │
│  │ Device    │  │ Conversation │  │  Message   │ │
│  │ Selector  │  │ List (left)  │  │  View      │ │
│  │ (top bar) │  │              │  │  (right)   │ │
│  └──────────┘  └──────────────┘  ├───────────┤ │
│                                   │  Compose   │ │
│                                   │  Input     │ │
│                                   └───────────┘ │
└───────────────────┬─────────────────────────────┘
                    │
┌───────────────────▼─────────────────────────────┐
│              Application State                   │
│  devices, conversations, messages, attachments   │
└───────────────────┬─────────────────────────────┘
                    │
┌───────────────────▼─────────────────────────────┐
│              D-Bus Client Layer                   │
│  zbus async proxies for kdeconnect interfaces    │
│  Signal listeners for real-time updates          │
└─────────────────────────────────────────────────┘
```

---

## Core Dependencies

```toml
[dependencies]
ratatui = "0.29"              # TUI framework
crossterm = "0.28"            # Terminal backend
ratatui-image = "10"          # Inline image display (sixel/kitty/iterm2)
image = "0.25"                # Image decoding
zbus = "5"                    # D-Bus client (async)
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
unicode-segmentation = "1"    # Proper emoji/grapheme handling
unicode-width = "0.2"         # Display width calculation
dirs = "6"                    # XDG paths for config/cache
toml = "0.8"                  # Config file format
clap = { version = "4", features = ["derive"] }  # CLI args
color-eyre = "0.6"            # Error handling
tracing = "0.1"               # Logging
tracing-subscriber = "0.3"
```

---

## Module Structure

```
src/
├── main.rs                   # Entry point, tokio runtime, arg parsing
├── app.rs                    # App state machine, event loop
├── config.rs                 # Config file (~/.config/kdeconnect-sms-tui/config.toml)
├── dbus/
│   ├── mod.rs
│   ├── daemon.rs             # Device discovery, pairing status
│   ├── conversations.rs      # Conversation list, message fetching
│   ├── messages.rs           # Send/reply, attachments
│   └── types.rs              # D-Bus type mappings (Message, Conversation, Address)
├── ui/
│   ├── mod.rs
│   ├── device_bar.rs         # Device selector (top bar)
│   ├── conversation_list.rs  # Left panel: conversation list with previews
│   ├── message_view.rs       # Right panel: message thread with images
│   ├── compose.rs            # Message input with attachment picker
│   ├── image_display.rs      # Inline image rendering via ratatui-image
│   ├── group_rename.rs       # Group rename dialog
│   └── theme.rs              # Colors, styling
├── models/
│   ├── mod.rs
│   ├── conversation.rs       # Conversation struct
│   ├── message.rs            # Message struct (text, attachments, timestamps)
│   ├── device.rs             # Device struct
│   └── attachment.rs         # Attachment handling (download, cache)
└── events.rs                 # Event enum (key input, D-Bus signals, resize, tick)
```

---

## Implementation Phases

### Phase 1: Project Skeleton & D-Bus Connection
- [x] Cargo project setup with all dependencies
- [ ] Connect to session bus, discover paired/reachable devices
- [ ] Basic `kdeconnect-cli -l` equivalent via D-Bus
- [ ] Minimal TUI that shows device list
- [ ] **Tests**: D-Bus proxy mock, device parsing

### Phase 2: Conversation List
- [ ] Call `requestAllConversationThreads()` and `activeConversations()`
- [ ] Parse conversation data from QDBusVariant format
- [ ] Display conversation list with contact name/number, last message preview, timestamp
- [ ] Listen for `conversationCreated`/`conversationUpdated`/`conversationRemoved` signals
- [ ] Sort by most recent
- [ ] **Tests**: Conversation parsing, sorting, signal handling

### Phase 3: Message View & Sending
- [ ] `requestConversation(id, start, end)` to fetch message history
- [ ] Render message bubbles (sent vs received, timestamps)
- [ ] Compose input box with text editing (multi-line)
- [ ] `replyToConversation()` for sending in existing threads
- [ ] `sendWithoutConversation()` for new messages
- [ ] Scroll through message history, lazy-load older messages
- [ ] **Tests**: Message rendering, send/reply flow

### Phase 4: Images & Attachments
- [ ] Detect terminal graphics capability via `ratatui-image` picker
- [ ] Render inline images in message view (MMS pictures)
- [ ] `requestAttachmentFile()` + `attachmentReceived` signal flow
- [ ] Cache downloaded attachments in XDG cache dir
- [ ] Attachment picker for sending (file browser dialog or path input)
- [ ] **Tests**: Image protocol detection, attachment download/cache

### Phase 5: Group Messages & Replies
- [ ] Handle multi-address conversations (group detection)
- [ ] Display group member list
- [ ] Group rename functionality (stored locally in config)
- [ ] Reply context (quote/reference previous message if supported)
- [ ] **Tests**: Group detection, rename persistence

### Phase 6: Device Switching
- [ ] Top bar showing current device with dropdown/selector
- [ ] Switch device → reload conversations
- [ ] Handle device going offline (signal monitoring)
- [ ] Auto-select first available device on startup
- [ ] **Tests**: Device switch state transitions

### Phase 7: Polish & UX
- [ ] Keyboard shortcuts help overlay (?)
- [ ] Search/filter conversations
- [ ] Notification indicator for new messages (unread count)
- [ ] Config file for: default device, theme, keybindings, image display preference
- [ ] Proper emoji rendering with grapheme cluster awareness
- [ ] Clipboard support for copying message text
- [ ] **Tests**: Config parsing, keybinding handling

---

## Testing Strategy

### Unit Tests
- **D-Bus type parsing**: Verify QVariant → Rust struct conversions for all message types
- **Model logic**: Conversation sorting, group detection, attachment path resolution
- **Config**: Parse/write config toml, handle missing/malformed files
- **UI state**: State machine transitions (selecting device → loading conversations → viewing messages)

### Integration Tests (require mock D-Bus or running kdeconnectd)
- **D-Bus proxy tests**: Use `zbus`'s test helpers or a mock D-Bus session bus
- **Signal handling**: Simulate incoming message signals, verify state updates
- **Attachment flow**: Mock attachment download and verify caching

### Snapshot/Rendering Tests
- Use `ratatui`'s `TestBackend` to assert rendered output for:
  - Conversation list layout
  - Message bubble rendering
  - Device bar states
  - Empty states (no devices, no conversations)

### Test Infrastructure
```
tests/
├── common/
│   └── mod.rs              # Shared test helpers, mock D-Bus setup
├── dbus_integration.rs     # Tests against mock/real D-Bus
├── ui_snapshot.rs          # ratatui TestBackend rendering tests
└── config_test.rs          # Config parsing tests
```

Each phase includes its own test requirements marked above. Tests should be written alongside implementation, not after.

---

## Necessities You Might Be Missing

1. **Contact name resolution** — kdeconnect provides phone numbers but not contact names from the phone's address book. May need to integrate with local contacts (e.g. `khard`, `abook`, or a simple local mapping file).

2. **Message persistence/caching** — kdeconnect doesn't store messages on the desktop side permanently. Consider a local SQLite cache so the app doesn't need to re-fetch everything on startup.

3. **Read receipts / typing indicators** — Not currently supported by kdeconnect's protocol.

4. **Notification integration** — Desktop notifications for incoming messages when the TUI isn't focused (via `notify-rust` or D-Bus `org.freedesktop.Notifications`).

5. **Phone number formatting** — Display and input normalization (international format, country codes).

6. **Rate limiting** — kdeconnect can be slow to respond when fetching all conversations from the phone. Need loading indicators and graceful timeout handling.

7. **Error handling for disconnected devices** — Device goes out of range mid-conversation.

---

## Key References

- [KDE Connect D-Bus conversations interface](https://github.com/KDE/kdeconnect-kde/blob/master/plugins/sms/conversationsdbusinterface.h)
- [KDE Connect CLI source](https://github.com/KDE/kdeconnect-kde/blob/master/cli/kdeconnect-cli.cpp)
- [RCS not supported — Bug 464654](https://bugs.kde.org/show_bug.cgi?id=464654)
- [ratatui-image — inline terminal images](https://github.com/benjajaja/ratatui-image)
- [zbus — Rust D-Bus library](https://github.com/dbus2/zbus)
- [Shell SMS sending guide](https://doronbehar.com/articles/using-kdeconnect-to-comfortably-send-sms-messages-from-the-shell/)
- [kdeconnect-cli usage examples](https://commandmasters.com/commands/kdeconnect-cli-common/)
