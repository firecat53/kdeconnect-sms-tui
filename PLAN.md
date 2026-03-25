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

| Interface | Object Path |
|---|---|
| `org.kde.kdeconnect.daemon` | `/modules/kdeconnect` |
| `org.kde.kdeconnect.device` | `/modules/kdeconnect/devices/{deviceId}` |
| `org.kde.kdeconnect.device.sms` | `/modules/kdeconnect/devices/{deviceId}/sms` |
| `org.kde.kdeconnect.device.conversations` | `/modules/kdeconnect/devices/{deviceId}` (adaptor on device object) |
| `org.kde.kdeconnect.device.telephony` | `/modules/kdeconnect/devices/{deviceId}/telephony` |

### Daemon Interface (org.kde.kdeconnect.daemon)

```
devices(onlyReachable: bool, onlyPaired: bool) -> StringList
deviceNames(onlyReachable: bool, onlyPaired: bool) -> Map<String,String>
deviceIdByName(name: String) -> String
selfId() -> String
```

**Signals:**
```
deviceAdded(id: String)
deviceRemoved(id: String)
deviceVisibilityChanged(id: String, isVisible: bool)
deviceListChanged()
```

### SMS Plugin Interface (org.kde.kdeconnect.device.sms)

```
sendSms(addresses: List, textMessage: String, attachmentUrls: List, subID: i64 = -1)
requestAllConversations()
requestConversation(conversationID: i64, rangeStartTimestamp: i64 = -1, numberToRequest: i64 = -1)
requestAttachment(partID: i64, uniqueIdentifier: String)
getAttachment(partID: i64, uniqueIdentifier: String)  # checks cache first
launchApp()  # opens messaging app on phone
```

Note: `subID` parameter enables **dual-SIM selection**.

### Conversations Interface (org.kde.kdeconnect.device.conversations)

```
activeConversations() -> QVariantList
requestConversation(conversationID: i64, start: i32, end: i32)
replyToConversation(conversationID: i64, message: String, attachmentUrls: List)
sendWithoutConversation(addressList: List, message: String, attachmentUrls: List)
requestAllConversationThreads()
requestAttachmentFile(partID: i64, uniqueIdentifier: String)
```

**Signals:**
```
conversationCreated(msg: Variant)
conversationRemoved(conversationID: i64)
conversationUpdated(msg: Variant)
conversationLoaded(conversationID: i64, messageCount: u64)
attachmentReceived(filePath: String, fileName: String)
```

### ConversationMessage Data Structure

Messages are serialized over D-Bus as structs with these fields:

| Field | Type | Description |
|---|---|---|
| `eventField` | `i32` | Bitwise: `0x1` = text message, `0x2` = multi-target (group) |
| `body` | `String` | Message text body |
| `addresses` | `List<{address: String}>` | Sender/recipient addresses |
| `date` | `i64` | Unix epoch milliseconds |
| `type` | `i32` | 1=Inbox, 2=Sent, 3=Draft, 4=Outbox, 5=Failed, 6=Queued |
| `read` | `i32` | Read status |
| `threadID` | `i64` | Conversation thread ID |
| `uID` | `i32` | Unique message identifier |
| `subID` | `i64` | SIM card subscriber ID |
| `attachments` | `List<{partID: i64, mimeType: String, base64EncodedFile: String, uniqueIdentifier: String}>` | Attachment metadata |

**Helpers:** `isIncoming()` = type==1, `isMultitarget()` = eventField & 0x2, `containsAttachment()` = attachments non-empty

### Incoming Message Notification Paths

1. **SMS Plugin (primary):** Phone sends `kdeconnect.sms.messages` packets → `conversationCreated`/`conversationUpdated` D-Bus signals
2. **Notifications Plugin (supplementary):** Android SMS notifications arrive as `kdeconnect.notification` packets with `replyId`, `isConversation`, `isGroupConversation`, `groupName` fields. Supports inline reply via `sendReply(replyId, message)`

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
dirs = "6"                    # XDG paths for state/cache
toml = "0.8"                  # State file format
clap = { version = "4", features = ["derive"] }  # CLI args
color-eyre = "0.6"            # Error handling
tracing = "0.1"               # Logging
tracing-subscriber = "0.3"
vcard_parser = "0.2"           # Parse vCard contacts from kdeconnect
```

---

## Module Structure

```
src/
├── main.rs                   # Entry point, tokio runtime, arg parsing
├── app.rs                    # App state machine, event loop
├── state.rs                  # App state (~/.local/state/kdeconnect-sms-tui/state.toml)
├── dbus/
│   ├── mod.rs
│   ├── daemon.rs             # Device discovery, pairing status
│   ├── conversations.rs      # Conversation list, message fetching, send/reply
│   ├── signals.rs            # D-Bus signal listeners for real-time updates
│   └── types.rs              # D-Bus type mappings (Message, Conversation, Address)
├── contacts.rs                   # Parse vCards from ~/.local/share/kpeoplevcard/, phone→name map
├── ui/
│   ├── mod.rs
│   ├── device_bar.rs         # Device selector (top bar)
│   ├── conversation_list.rs  # Left panel: conversation list with previews
│   ├── message_view.rs       # Right panel: message thread with images
│   ├── compose.rs            # Message input
│   ├── device_popup.rs       # Device selector popup
│   ├── folder_popup.rs       # Archive/spam folder viewer
│   ├── group_info_popup.rs   # Group rename dialog
│   ├── help_popup.rs         # Keybinding help overlay (?)
│   ├── test_helpers.rs       # Shared test utilities for UI tests
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
- [x] Connect to session bus, discover paired/reachable devices
- [x] Basic `kdeconnect-cli -l` equivalent via D-Bus
- [x] Minimal TUI that shows device list
- [x] **Tests**: D-Bus proxy mock, device parsing

### Phase 2: Conversation List
- [x] Call `requestAllConversationThreads()` and `activeConversations()`
- [x] Parse conversation data from QDBusVariant format
- [x] Display conversation list with contact name/number, last message preview, timestamp
- [x] Listen for `conversationCreated`/`conversationUpdated`/`conversationRemoved` signals
- [x] Sort by most recent
- [x] **Tests**: Conversation parsing, sorting, signal handling

### Phase 3: Message View & Sending
- [x] `requestConversation(id, start, end)` to fetch message history
- [x] Render message bubbles (sent vs received, timestamps)
- [x] Compose input box with text editing (multi-line)
- [x] `replyToConversation()` for sending in existing threads
- [x] `sendWithoutConversation()` for new messages
- [x] Scroll through message history (auto-scroll to newest)
- [x] Lazy-load older messages on scroll
- [x] **Tests**: Message rendering, send/reply flow

### Phase 4: Images & Attachments
- [x] Detect terminal graphics capability via `ratatui-image` picker (Sixel/Kitty/halfblock)
- [x] Auto-download image attachments when viewing a conversation
- [x] Render inline images in message view via `StatefulImage`
- [x] Show `[Downloading...]` placeholder while fetching, `[Image failed: ...]` on error
- [x] `requestAttachmentFile()` + `attachmentReceived` signal flow
- [x] Uses kdeconnect's built-in cache (`~/.cache/kdeconnect/{device_name}/`)
- [x] Non-image attachments: open via `xdg-open` on keypress
- [ ] Allow selecting and downloading/saving images to a chosen path
- [x] Attachment picker for sending (file browser dialog or path input)
- [ ] **Tests**: attachment download/cache

### Phase 5: Group Messages & Replies
- [x] Handle multi-address conversations (group detection via address count)
- [x] Display group member list
- [x] Group rename functionality (stored in state file)
- [ ] Reply context (quote/reference previous message if supported)
- [x] **Tests**: Group detection

### Phase 6: Device Switching
- [x] Top bar showing current device with selection indicator
- [x] Switch device → reload conversations (Tab key)
- [ ] Handle device going offline (signal monitoring — `deviceListChanged` not yet listened for)
- [x] Auto-select first available device on startup
- [x] **Tests**: Device switch state transitions

### Phase 7: Polish & UX
- [x] Keyboard shortcuts help overlay (?)
- [ ] Search/filter conversations
- [ ] Notification indicator for new messages (unread count)
- [x] State file for group names, archive/spam lists (`~/.local/state/kdeconnect-sms-tui/state.toml`)
- [x] Proper emoji rendering with grapheme cluster awareness
- [x] Clipboard support for copying message text/attachments (`c` key)
- [x] Message-level selection with highlighted scrolling (j/k navigates messages and attachments)
- [x] Open attachments with xdg-open (`Enter` on attachment)
- [x] Archive/spam folders with auto-restore on new incoming messages
- [x] Panic hook to restore terminal on crash
- [x] Safe string truncation (char-boundary-aware)
- [x] **Tests**: State serialization

---

## Testing Strategy

### Unit Tests
- **D-Bus type parsing**: Verify QVariant → Rust struct conversions for all message types
- **Model logic**: Conversation sorting, group detection, attachment path resolution
- **State**: Parse/write state toml, handle missing/malformed files
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
└── state_test.rs           # State serialization tests
```

Each phase includes its own test requirements marked above. Tests should be written alongside implementation, not after.

---

## Necessities You Might Be Missing

1. **Contact name resolution** — kdeconnect's contacts plugin syncs phone contacts as vCard files to `~/.local/share/kpeoplevcard/` (one-way, Android → Desktop, auto-synced). We parse these `.vcf`/`.vcard` files to build a phone number → display name mapping. No external contacts app needed.

2. **Message persistence/caching** — kdeconnect doesn't store messages on the desktop side permanently. Consider a local SQLite cache so the app doesn't need to re-fetch everything on startup.

3. **Read receipts / typing indicators** — Not currently supported by kdeconnect's protocol.

4. **Notification integration** — Desktop notifications for incoming messages when the TUI isn't focused (via `notify-rust` or D-Bus `org.freedesktop.Notifications`).

5. **Phone number formatting** — Display and input normalization (international format, country codes).

6. **Rate limiting** — kdeconnect can be slow to respond when fetching all conversations from the phone. Need loading indicators and graceful timeout handling.

7. **Error handling for disconnected devices** — Device goes out of range mid-conversation.

8. **Dual-SIM selection** — `sendSms()` accepts a `subID` for SIM card selection. Should expose this in the UI for dual-SIM phones.

9. **Prior art** — [GideonWolfe/kdeconnect-sms-tui](https://github.com/GideonWolfe/kdeconnect-sms-tui) is an existing (unmaintained) Go TUI for kdeconnect SMS. Worth studying for UX ideas.

---

## Key References

- [KDE Connect D-Bus conversations interface](https://github.com/KDE/kdeconnect-kde/blob/master/plugins/sms/conversationsdbusinterface.h)
- [KDE Connect CLI source](https://github.com/KDE/kdeconnect-kde/blob/master/cli/kdeconnect-cli.cpp)
- [RCS not supported — Bug 464654](https://bugs.kde.org/show_bug.cgi?id=464654)
- [ratatui-image — inline terminal images](https://github.com/benjajaja/ratatui-image)
- [zbus — Rust D-Bus library](https://github.com/dbus2/zbus)
- [Shell SMS sending guide](https://doronbehar.com/articles/using-kdeconnect-to-comfortably-send-sms-messages-from-the-shell/)
- [kdeconnect-cli usage examples](https://commandmasters.com/commands/kdeconnect-cli-common/)
