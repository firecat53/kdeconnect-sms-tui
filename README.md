# kdeconnect-sms-tui

Terminal UI for sending and receiving SMS/MMS via KDE Connect.

Disclaimer - project was coded using Claude and other AI tools

## Quick start

```
cargo install --path .
kdeconnect-sms-tui
```

OR

`nix run github:firecat53/kdeconnect-sms-tui`

Requires a paired Android device running the KDE Connect app, and `kdeconnectd` running on your machine.

### Optional dependencies

For clipboard support (`c` key to copy messages/attachments):

| Platform        | Tool                            |
|-----------------|---------------------------------|
| Linux (Wayland) | `wl-copy` (from `wl-clipboard`) |
| Linux (X11)     | `xclip` or `xsel`               |
| macOS           | `pbcopy` (built-in)             |

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

**Conversations/Messages**:

| Key                            | Action                                  |
|--------------------------------|-----------------------------------------|
| `j`/`k` or `Up`/`Down`         | Next/Previous                           |
| `J`/`K` or `PageDown`/`PageUp` | Page down / page up                     |
| `h`/`l` or `Tab`               | Switch focus to messages panel          |
| `i`                            | Compose message (existing conversation) |
| `d`                            | Open device selector                    |
| `g`                            | Edit group name                         |
| `a` / `s` / `t`                | Archive / spam / trash selected conversation |
| `A` / `S` / `T`                | View archived / spam / trash folder     |
| `r`                            | Refresh conversations / reconnect       |
| `/`                            | Search conversations by name or number  |
| `n` / `p`                      | Next / previous search match            |
| `Esc`                          | Clear search                            |
| `Ctrl+t`                       | Cycle themes                            |
| `?`                            | Show help popup                         |
| `q`                            | Quit                                    |

**Messages panel only**:

| Key                         | Action                                      |
|-----------------------------|---------------------------------------------|
| `Enter`                     | Open selected attachment (xdg-open)         |
| `D`                         | Download selected image to downloads folder |
| `c`                         | Copy message text or image to clipboard     |

**Compose**:

| Key                                    | Action                                                            |
|----------------------------------------|-------------------------------------------------------------------|
| `Enter`                                | Send message                                                      |
| `Shift+Enter` / `Alt+Enter` / `Ctrl+j` | Newline                                                           |
| `Esc`                                  | Exit compose (draft saved)                                        |
| Readline shortcuts                     | `C-a` `C-e` `C-f` `C-b` `M-f` `M-b` `C-d` `C-k` `C-u` `M-d` `C-w` |
| `Alt+a`                                | Attach an image                                                   |
| `Alt+x`                                | Remove attachment                                                 |

**Device selector** (after pressing d):

| Key                    | Action               |
|------------------------|----------------------|
| `j`/`k` or `Up`/`Down` | Next/Previous device |
| `Enter`                | Select device        |
| `Esc` / `d` / `q`      | Close                |

**File picker** (image attachment):

| Key                            | Action                          |
|--------------------------------|---------------------------------|
| `j`/`k` or `Up`/`Down`         | Next/Previous entry             |
| `h`/`l` or `Enter`/`Backspace` | Enter or go to parent directory |
| `Enter`                        | Enter directory or select image |
| `Esc` / `q`                    | Cancel                          |

Message scrolling moves message-by-message, keeping the bottom of the
current message aligned to the viewport bottom. Messages taller than the
viewport scroll line-by-line within the message.

`Ctrl+c` quits from any screen.

## Installation

### Pre-built binaries

Download the latest release from the [releases page](https://github.com/firecat53/kdeconnect-sms-tui/releases):

- `kdeconnect-sms-tui-x86_64-unknown-linux-gnu` — with HEIC/HEIF image support (requires `libheif` at runtime)
- `kdeconnect-sms-tui-x86_64-unknown-linux-gnu-no-heif` — without HEIF support, no native library required

```bash
chmod +x kdeconnect-sms-tui-*
mv kdeconnect-sms-tui-* ~/.local/bin/kdeconnect-sms-tui
```

### Requirements

- `kdeconnectd` (the KDE Connect daemon)
- KDE Connect app on your Android device, paired with your machine
- D-Bus session bus
- `libheif` (optional, runtime dep for the HEIF variant — for HEIC/HEIF image support)
  - Debian/Ubuntu: `apt install libheif1`
  - Fedora: `dnf install libheif`
  - Arch: `pacman -S libheif`

### From source

Requires Rust 1.70+ and (optionally) `libheif` development headers:
- Debian/Ubuntu: `apt install libheif-dev`
- Fedora: `dnf install libheif-devel`
- Arch: `pacman -S libheif`
- Nix: included in the flake

```bash
git clone https://github.com/firecat53/kdeconnect-sms-tui
cd kdeconnect-sms-tui
cargo build --release
cp target/release/kdeconnect-sms-tui ~/.local/bin/
```

To build without HEIF support: `cargo build --release --no-default-features`

### Verify KDE Connect is running

```
# Check daemon
qdbus org.kde.kdeconnect /modules/kdeconnect org.kde.kdeconnect.daemon.devices

# List paired devices
kdeconnect-cli -l
```

## State

Application state (group names, archived/spam thread lists, selected theme) is stored
at `~/.local/state/kdeconnect-sms-tui/state.toml` (`$XDG_STATE_HOME`).
This file is managed automatically by the app.

## Contacts

Contact names are read from KDE Connect's synced vCards at
`~/.local/share/kpeoplevcard/`. Enable the Contacts plugin in the KDE Connect
app to sync them.

## Features

- Browse conversations
- Search conversations and messages
- Send/receive SMS and MMS
- Inline image display (Kitty, Sixel, iTerm2, halfblocks)
- Contact name resolution from synced vCards
- Group conversation support with custom naming
- Multiple device switching with popup selector
- Per-conversation draft messages
- Archive and spam folders for hiding conversations
- Auto-restore archived/spam threads on new incoming messages
- 17 built-in color themes (9 dark, 8 light) with persistent selection

## Known limitations

- **Offline device detection is slow.** When a phone disconnects
  uncleanly (e.g. walks out of WiFi range, battery dies, or Android kills
  the background process), `kdeconnectd` may not notice for 10–20 minutes.
  This is a limitation of the KDE Connect daemon, which relies on TCP
  keep-alive timeouts to detect dead connections. During this window the
  device still appears "reachable," sent messages will be silently queued
  (and likely lost), and `r` to refresh will appear to hang. The official
  `kdeconnect-sms` GUI has the same behavior. A clean disconnect (e.g.
  toggling KDE Connect off on the phone) is detected immediately.

## License

MIT
