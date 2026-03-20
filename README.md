# kdeconnect-sms-tui

Terminal UI for sending and receiving SMS/MMS via KDE Connect.

## Quick start

```
cargo install --path .
kdeconnect-sms-tui
```

Requires a paired Android device running the KDE Connect app, and `kdeconnectd` running on your machine.

## Usage

```
kdeconnect-sms-tui [OPTIONS]

Options:
  -d, --device <ID>      Device ID to connect to (default: first available)
  -n, --name <NAME>      Device name to connect to
      --log-file <PATH>  Log file path (logs suppressed if not set)
```

### Keybindings

**Conversation list** (default focus):

| Key | Action |
|-----|--------|
| `j` / `Down` | Next conversation |
| `k` / `Up` | Previous conversation |
| `Enter` / `i` | Open conversation / focus compose |
| `Tab` | Cycle connected device |
| `r` | Refresh conversations / reconnect |
| `PageUp` / `PageDown` | Scroll messages |
| `q` | Quit |

**Compose** (after pressing Enter/i):

| Key | Action |
|-----|--------|
| `Enter` | Send message |
| `Shift+Enter` / `Alt+Enter` | Newline |
| `Esc` | Back to conversation list |
| `Up` / `Down` | Scroll messages (1 line) |
| `PageUp` / `PageDown` | Scroll messages (1 page) |
| `Left` / `Right` / `Home` / `End` | Cursor movement |

`Ctrl+C` quits from any screen.

## Installation

### Requirements

- Rust 1.70+
- `kdeconnectd` (the KDE Connect daemon)
- KDE Connect app on your Android device, paired with your machine
- D-Bus session bus

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

## Configuration

Config file: `~/.config/kdeconnect-sms-tui/config.toml`

```toml
default_device = "device_id_here"

[group_names]
"12345" = "Family Chat"
```

## Contacts

Contact names are read from KDE Connect's synced vCards at `~/.local/share/kpeoplevcard/`. Enable the Contacts plugin in the KDE Connect app to sync them.

## Features

- Browse and search conversations
- Send/receive SMS and MMS
- Inline image display (Kitty, Sixel, iTerm2, halfblocks)
- Contact name resolution from synced vCards
- Group conversation support
- Multiple device switching

## License

MIT
