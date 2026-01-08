# Axelerator Deployment Guide

This guide covers deploying Axelerator as a long-running systemd service on Linux.

## Quick Start

```bash
# Build
cargo build --release -p axelerator

# Create user and directory
sudo useradd -r -s /usr/sbin/nologin axelerator
sudo mkdir -p /opt/axelerator
sudo chown axelerator:axelerator /opt/axelerator

# Install binary
sudo cp target/release/axelerator /opt/axelerator/
sudo chmod +x /opt/axelerator/axelerator

# Generate and edit config
sudo -u axelerator /opt/axelerator/axelerator init -o /opt/axelerator/axelerator.toml
sudo nano /opt/axelerator/axelerator.toml

# First-time auth (interactive) - as root since axelerator user has no shell
sudo /opt/axelerator/axelerator -c /opt/axelerator/axelerator.toml
# Complete Xbox login, wait for "Session created", then Ctrl+C

# Fix token ownership
sudo chown axelerator:axelerator /opt/axelerator/token.json
sudo chmod 600 /opt/axelerator/token.json

# Install and start service
sudo cp crates/axelerator/deploy/axelerator.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now axelerator
```

## Configuration File

Axelerator uses TOML configuration files. Generate an example:

```bash
axelerator init -o axelerator.toml
```

### Example Configuration

```toml
# Server visible name in friends list
host_name = "My Awesome Server"
world_name = "Survival World"

# Target Minecraft server (where players are transferred)
server_ip = "play.example.com"
server_port = 19132

# Player count display (cosmetic - Xbox doesn't validate these)
max_players = 20            # Shows as "X/20" in friends list
display_players = 5         # Shows as "5/X" - make server look populated!

# Security monitoring
monitor_tampering = true
monitor_interval = 30
auto_block_attackers = false

# Token storage (relative to working directory, or use absolute path)
token_cache_path = "token.json"

# Logging
[logging]
level = "info"              # error, warn, info, debug, trace
show_target = false         # show module names
show_timestamp = false      # journald adds timestamps
```

### Configuration Priority

Settings are applied in this order (later overrides earlier):
1. Default values
2. Config file (`-c` / `--config`)
3. CLI arguments (`--server-ip`, `--monitor`, etc.)

### CLI Overrides

```bash
# Override specific settings without editing config
axelerator -c axelerator.toml --server-ip 192.168.1.100 --monitor
```

## Installation (Detailed)

### 1. Create User and Directory

```bash
# Create system user (no login shell, no home directory)
sudo useradd -r -s /usr/sbin/nologin axelerator

# Create installation directory
sudo mkdir -p /opt/axelerator
sudo chown axelerator:axelerator /opt/axelerator
```

### 2. Deploy Binary and Config

```bash
sudo cp target/release/axelerator /opt/axelerator/
sudo chmod +x /opt/axelerator/axelerator

# Generate config
sudo -u axelerator /opt/axelerator/axelerator init -o /opt/axelerator/axelerator.toml

# Edit config with your settings
sudo nano /opt/axelerator/axelerator.toml
```

### 3. Initial Authentication

Xbox Live requires interactive device code authentication on first run.
Run as root (the axelerator user has no shell):

```bash
sudo /opt/axelerator/axelerator -c /opt/axelerator/axelerator.toml
```

You'll see:
```
╔══════════════════════════════════════════════════════════╗
║                    XBOX LIVE LOGIN                       ║
╠══════════════════════════════════════════════════════════╣
║  1. Open: https://www.microsoft.com/link                 ║
║  2. Enter code: XXXXXXXX                                 ║
╚══════════════════════════════════════════════════════════╝
```

1. Go to the URL on any device
2. Enter the code shown
3. Sign in with your Xbox account
4. Wait until you see "Session created - server is now visible to friends!"
5. Press Ctrl+C to stop

Then fix ownership of the token file:
```bash
sudo chown axelerator:axelerator /opt/axelerator/token.json
sudo chmod 600 /opt/axelerator/token.json
```

### 4. Install systemd Service

```bash
sudo cp deploy/axelerator.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable axelerator
sudo systemctl start axelerator
```

### 5. Verify

```bash
sudo systemctl status axelerator
sudo journalctl -u axelerator -f
```

## Service Management

### Common Commands

```bash
# Check status
sudo systemctl status axelerator

# Start/stop/restart
sudo systemctl start axelerator
sudo systemctl stop axelerator
sudo systemctl restart axelerator

# Enable/disable auto-start on boot
sudo systemctl enable axelerator
sudo systemctl disable axelerator

# Reload config (requires restart)
sudo systemctl restart axelerator
```

### Viewing Logs

```bash
# Follow live logs
sudo journalctl -u axelerator -f

# Last 100 lines
sudo journalctl -u axelerator -n 100

# Last hour
sudo journalctl -u axelerator --since "1 hour ago"

# Today's logs
sudo journalctl -u axelerator --since today

# Errors only
sudo journalctl -u axelerator -p err

# Since last boot
sudo journalctl -u axelerator -b

# Export to file
sudo journalctl -u axelerator --since today > axelerator.log
```

### Re-Authentication (Token Refresh)

OAuth tokens last ~90 days. If you see authentication errors:

```bash
# Stop the service
sudo systemctl stop axelerator

# Remove old token
sudo rm /opt/axelerator/token.json

# Re-authenticate (interactive)
sudo /opt/axelerator/axelerator -c /opt/axelerator/axelerator.toml
# Complete Xbox login, wait for "Session created", Ctrl+C

# Fix ownership
sudo chown axelerator:axelerator /opt/axelerator/token.json
sudo chmod 600 /opt/axelerator/token.json

# Start service
sudo systemctl start axelerator
```

## Logging

### Log Levels

| Level | Description |
|-------|-------------|
| `error` | Critical failures only |
| `warn` | Warnings + errors |
| `info` | Normal operations (recommended) |
| `debug` | Detailed operations + stats |
| `trace` | Very verbose (not recommended) |

### Module-Specific Levels

```toml
[logging]
level = "warn,axelerator=info,tokio_nethernet=warn"
```

### journald Notes

- **Timestamps**: journald adds timestamps automatically; `show_timestamp = false` is recommended
- **Rate limiting**: journald limits to ~10000 messages per 30 seconds by default
- **Rotation**: journald auto-rotates logs (default ~10% of disk, max 4GB)
- **No disk fill risk**: Log flooding won't crash your service

## Monitoring

### Health Indicators

**Healthy**:
- `systemctl status` shows `active (running)`
- Logs show "Session created - server is now visible to friends!"
- Player connections show "Friend connected via WebRTC!"
- Transfers show "Transfer packet sent successfully!"

**Warning Signs**:
- Repeated "Reconnecting transfer server" messages
- "Failed to refresh presence" errors
- No "Friend connected" messages when players try to join
- Service keeps restarting (check with `systemctl status`)

### Log Messages Reference

| Message | Meaning |
|---------|---------|
| `Session created - server is now visible to friends!` | Server is ready |
| `Friend connected via WebRTC!` | Player connecting |
| `Transferring player to downstream server` | Handshake complete |
| `Transfer packet sent successfully!` | Player redirected |
| `Reconnecting transfer server` | Auto-recovering from network issue |
| `Transfer server authentication error` | Token may need refresh |
| `Presence refresh failed` | Xbox Live API issue |

### Quick Health Check

```bash
# One-liner to check if running and see recent activity
sudo systemctl is-active axelerator && sudo journalctl -u axelerator -n 5 --no-pager
```

## Troubleshooting

### Service Won't Start

```bash
# Check recent logs
sudo journalctl -u axelerator -n 50 --no-pager

# Check service status
sudo systemctl status axelerator
```

Common causes:
- Config file syntax error - check TOML syntax
- Token file missing - run re-authentication
- Permission denied - check file ownership

### Token Expired / Auth Errors

If you see "authentication error" or "401" in logs:

```bash
sudo systemctl stop axelerator
sudo rm /opt/axelerator/token.json
sudo /opt/axelerator/axelerator -c /opt/axelerator/axelerator.toml
# Re-authenticate, Ctrl+C after "Session created"
sudo chown axelerator:axelerator /opt/axelerator/token.json
sudo systemctl start axelerator
```

### Frequent Disconnects

If "Reconnecting transfer server" appears often:
1. Check network stability
2. Verify Xbox Live services status (https://support.xbox.com/xbox-live-status)
3. Check for Xbox account restrictions
4. Review error messages in logs

### Players Can't Connect

1. Verify your Minecraft server is running at the configured IP/port
2. Check `server_ip` and `server_port` in config match your server
3. Ensure logs show "Session created" (server is visible)
4. Make sure players have added your Xbox account as a friend
5. Check firewall allows UDP to your Minecraft server port

### Service Keeps Crashing

```bash
# Check for crash patterns
sudo journalctl -u axelerator --since "1 hour ago" | grep -i error

# Check system resources
free -h
df -h /opt/axelerator
```

## Security

### File Permissions

```bash
# Secure token file (contains OAuth credentials)
sudo chmod 600 /opt/axelerator/token.json

# Config can be world-readable
sudo chmod 644 /opt/axelerator/axelerator.toml

# Ensure correct ownership
sudo chown axelerator:axelerator /opt/axelerator/*
```

### Firewall

Axelerator needs:
- **Outbound HTTPS (443)**: Xbox Live APIs, signaling WebSocket
- **Outbound UDP**: STUN/TURN for WebRTC
- **No inbound ports required**: WebRTC uses TURN relays

### Attack Detection

Enable monitoring to detect session tampering:

```toml
monitor_tampering = true
monitor_interval = 30
auto_block_attackers = true
```

This detects if someone tries to hijack your Xbox Live session.

## Updating

### From Source (on build machine)

```bash
# Pull latest code
cd axolotl-stack
git pull

# Build new version
cargo build --release -p axelerator

# Copy to server (replace with your server details)
scp target/release/axelerator user@yourserver:/tmp/
```

### On the Server

```bash
# Stop the service
sudo systemctl stop axelerator

# Backup old binary (optional but recommended)
sudo cp /opt/axelerator/axelerator /opt/axelerator/axelerator.bak

# Install new binary
sudo cp /tmp/axelerator /opt/axelerator/
sudo chmod +x /opt/axelerator/axelerator

# Start the service
sudo systemctl start axelerator

# Verify it's running
sudo systemctl status axelerator
sudo journalctl -u axelerator -n 20 --no-pager
```

### Quick Update Script

For frequent updates, create `/opt/axelerator/update.sh`:

```bash
#!/bin/bash
set -e

if [ -z "$1" ]; then
    echo "Usage: $0 /path/to/new/axelerator"
    exit 1
fi

echo "Stopping axelerator..."
systemctl stop axelerator

echo "Backing up old binary..."
cp /opt/axelerator/axelerator /opt/axelerator/axelerator.bak

echo "Installing new binary..."
cp "$1" /opt/axelerator/axelerator
chmod +x /opt/axelerator/axelerator

echo "Starting axelerator..."
systemctl start axelerator

echo "Done! Checking status..."
systemctl status axelerator --no-pager
```

Then update with: `sudo /opt/axelerator/update.sh /tmp/axelerator`

### Rollback

If something goes wrong:

```bash
sudo systemctl stop axelerator
sudo cp /opt/axelerator/axelerator.bak /opt/axelerator/axelerator
sudo systemctl start axelerator
```

## Uninstalling

```bash
# Stop and disable service
sudo systemctl stop axelerator
sudo systemctl disable axelerator

# Remove service file
sudo rm /etc/systemd/system/axelerator.service
sudo systemctl daemon-reload

# Remove installation
sudo rm -rf /opt/axelerator

# Remove user
sudo userdel axelerator
```

## Directory Structure

After installation, `/opt/axelerator/` contains:

```
/opt/axelerator/
├── axelerator          # Binary
├── axelerator.toml     # Configuration
└── token.json          # OAuth token cache (sensitive!)
```
