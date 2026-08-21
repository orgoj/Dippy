# Dippy Askpass

GUI approval dialog for Dippy when running in headless environments (tmux, screen, background).

## Installation

```bash
# Symlink to your bin
ln -s /path/to/dippy/tools/askpass/dippy-askpass ~/bin/dippy-askpass

# Or copy
cp /path/to/dippy/tools/askpass/dippy-askpass ~/bin/
```

## Configuration

Add to `~/.dippy/config`:

```
set askpass ~/bin/dippy-askpass
set askpass-timeout 60
```

Or use environment variable:

```bash
export DIPPY_ASKPASS=~/bin/dippy-askpass
```

## Usage

When a Dippy rule returns `ask`, this dialog pops up showing:

- **Command** being approved
- **Directory** where it runs
- **Rule pattern** that matched (if any)
- **Message** from the rule (if any)

### Buttons

| Button | Exit Code | Effect |
|--------|-----------|--------|
| **Allow** | 0 | Approve the command |
| **Deny** | 1 | Block the command |
| **Ask Claude** | 2 | Fall back to Claude's dialog |

### Keyboard Shortcuts

- `Enter` = Allow
- `Esc` = Deny
- `Space` = Ask Claude

## Requirements

- Python 3 with tkinter (usually included)
- X11 display available (set `DISPLAY` env var if needed)

## Troubleshooting

**Dialog doesn't appear:**
```bash
# Check if DISPLAY is set
echo $DISPLAY

# If running in tmux, ensure DISPLAY is forwarded
export DISPLAY=:0
```

**tkinter not found:**
```bash
# Ubuntu/Debian
sudo apt install python3-tk

# Fedora
sudo dnf install python3-tkinter

# Arch
sudo pacman -S tk
```
