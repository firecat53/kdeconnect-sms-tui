# kdeconnect-sms-tui

Terminal UI for sending and receiving SMS/MMS via KDE Connect.

## Quick start

```
cargo install --path .
kdeconnect-sms-tui
```

Requires a paired Android device running the KDE Connect app, and `kdeconnectd` running on your machine.

### Optional dependencies

For clipboard support (`c` key to copy messages/attachments):

| Platform | Tool |
|----------|------|
| Linux (Wayland) | `wl-copy` (from `wl-clipboard`) |
| Linux (X11) | `xclip` or `xsel` |
| macOS | `pbcopy` (built-in) |

## Usage

```
kdeconnect-sms-tui [OPTIONS]

Options:
  -d, --device <ID>      Device ID to connect to (default: first available)
  -n, --name <NAME>      Device name to connect to
      --log-file <PATH>  Log file path (logs suppressed if not set)
```

### Keybindings

`Tab` switches focus between the **conversations** and **messages** panels.
The active panel is highlighted with a distinct border color. Draft messages
are saved per-conversation when you switch away.

**Conversations panel**:

| Key | Action |
|-----|--------|
| `j` / `Down` | Next conversation |
| `k` / `Up` | Previous item |
| `J` / `K` | Page down / page up |
| `PageDown` / `PageUp` | Page down / page up |
| `l` / `Tab` | Switch focus to messages panel |
| `Enter` / `i` | Focus compose input for the selected conversation |
| `d` | Open device selector |
| `g` | Edit group name (conversations panel) |
| `a` / `s` | Archive / spam selected conversation |
| `A` / `S` | View archived / spam folder |
| `r` | Refresh conversations / reconnect |
| `?` | Show help popup |
| `q` | Quit |

**Messages panel**:

| Key | Action |
|-----|--------|
| `j` / `Down` | Next message or attachment |
| `k` / `Up` | Previous message or attachment |
| `J` / `K` | Page down / page up |
| `PageDown` / `PageUp` | Page down / page up |
| `h` / `Tab` | Switch focus to conversations panel |
| `i` | Focus compose input |
| `Enter` | Open selected attachment (xdg-open) |
| `D` | Download selected image to downloads folder |
| `c` | Copy message text or attachment to clipboard |
| `d` | Open device selector |
| `g` | Edit group name |
| `r` | Refresh conversations / reconnect |
| `?` | Show help popup |
| `q` | Quit |

**Compose**:

| Key | Action |
|-----|--------|
| `Enter` | Send message |
| `Shift+Enter` / `Alt+Enter` / `Ctrl+j` | Newline |
| `Esc` | Back to previous panel |
| `Left` / `Right` / `Home` / `End` | Cursor movement |
| `Alt+A` | Attach an image |
| `Alt+X` | Remove the pending attachment |

**Device selector** (after pressing d):

| Key | Action |
|-----|--------|
| `j` / `Down` | Next device |
| `k` / `Up` | Previous device |
| `Enter` | Select device |
| `Esc` / `d` / `q` | Close |

**File picker** (after `Alt+A` in compose):

| Key | Action |
|-----|--------|
| `j` / `Down` | Next entry |
| `k` / `Up` | Previous entry |
| `Enter` | Open directory or select image |
| `Backspace` | Go to parent directory |
| `Esc` / `q` | Cancel |

Message scrolling moves message-by-message, keeping the bottom of the
current message aligned to the viewport bottom. Messages taller than the
viewport scroll line-by-line within the message.

`Ctrl+C` quits from any screen.

## Installation

### Requirements

- Rust 1.70+
- `kdeconnectd` (the KDE Connect daemon)
- KDE Connect app on your Android device, paired with your machine
- D-Bus session bus
- `libheif` development headers (optional, for HEIC/HEIF image support)
  - Debian/Ubuntu: `apt install libheif-dev`
  - Fedora: `dnf install libheif-devel`
  - Arch: `pacman -S libheif`
  - Nix: included in the flake

Without `libheif`, the app still compiles and runs but HEIC/HEIF images
will show a placeholder instead of being rendered inline. To build
without HEIF support: `cargo build --no-default-features`

### From source

```
git clone https://github.com/firecat53/kdeconnect-sms-tui
cd kdeconnect-sms-tui
cargo build --release
cp target/release/kdeconnect-sms-tui ~/.local/bin/
```

### Verify KDE Connect is running

```
# Check daemon
qdbus org.kde.kdeconnect /modules/kdeconnect org.kde.kdeconnect.daemon.devices

# List paired devices
kdeconnect-cli -l
```

## State

Application state (group names, archived/spam thread lists) is stored
at `~/.local/state/kdeconnect-sms-tui/state.toml` (`$XDG_STATE_HOME`).
This file is managed automatically by the app.

## Contacts

Contact names are read from KDE Connect's synced vCards at `~/.local/share/kpeoplevcard/`. Enable the Contacts plugin in the KDE Connect app to sync them.

## Features

- Browse conversations
- Search conversations (placeholder; not implemented yet)
- Send/receive SMS and MMS
- Inline image display (Kitty, Sixel, iTerm2, halfblocks)
- Contact name resolution from synced vCards
- Group conversation support with custom naming
- Multiple device switching with popup selector
- Per-conversation draft messages
- Archive and spam folders for hiding conversations
- Auto-restore archived/spam threads on new incoming messages

## License

MIT
