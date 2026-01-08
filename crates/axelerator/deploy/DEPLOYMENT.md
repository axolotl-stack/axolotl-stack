# Axelerator Deployment Guide

This guide covers deploying Axelerator as a long-running systemd service on Linux.

## Quick Start

```bash
# Build
cargo build --release -p axelerator

# Install
sudo mkdir -p /opt/axelerator
sudo cp target/release/axelerator /opt/axelerator/

# Generate config
sudo /opt/axelerator/axelerator init -o /opt/axelerator/axelerator.toml

# Edit config
sudo nano /opt/axelerator/axelerator.toml

# First-time auth (interactive)
sudo /opt/axelerator/axelerator -c /opt/axelerator/axelerator.toml

# Install and start service
sudo cp crates/axelerator/deploy/axelerator.service /etc/systemd/system/
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

# Security monitoring
monitor_tampering = true
monitor_interval = 30
auto_block_attackers = false

# Token storage
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

## Installation

### 1. Create User and Directory

```bash
sudo useradd -r -s /usr/sbin/nologin axelerator
sudo mkdir -p /opt/axelerator
sudo chown axelerator:axelerator /opt/axelerator
```

### 2. Deploy Binary and Config

```bash
sudo cp target/release/axelerator /opt/axelerator/
sudo chmod +x /opt/axelerator/axelerator

# Generate and edit config
sudo -u axelerator /opt/axelerator/axelerator init -o /opt/axelerator/axelerator.toml
sudo nano /opt/axelerator/axelerator.toml
```

### 3. Initial Authentication

Xbox Live requires interactive device code authentication on first run:

```bash
sudo -u axelerator /opt/axelerator/axelerator -c /opt/axelerator/axelerator.toml
```

Follow the prompts to authenticate. Once you see "Session created", press Ctrl+C.

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

### journald Commands

```bash
# Follow live logs
sudo journalctl -u axelerator -f

# Last hour
sudo journalctl -u axelerator --since "1 hour ago"

# Errors only
sudo journalctl -u axelerator -p err

# Export to file
sudo journalctl -u axelerator --since today > axelerator.log
```

### journald Notes

- **Timestamps**: journald adds timestamps automatically; `show_timestamp = false` is recommended
- **Rate limiting**: journald limits to ~10000 messages per 30 seconds by default
- **Rotation**: journald auto-rotates logs (default ~10% of disk, max 4GB)
- **No disk fill risk**: Log flooding won't crash your service

## Monitoring

### Health Indicators

**Healthy**:
- systemd shows `active (running)`
- Logs show periodic "Presence refreshed"
- "Session created" message present

**Warning Signs**:
- Repeated "Reconnecting transfer server" messages
- "Failed to refresh presence" errors
- No "Friend connected" messages when players try to join

### Log Messages Reference

| Message | Meaning |
|---------|---------|
| `Session created` | Server visible to friends |
| `Friend connected via WebRTC` | Player connecting |
| `Transfer packet sent` | Player redirected successfully |
| `Reconnecting transfer server` | Auto-recovering from network issue |
| `Transfer server fatal error` | Check authentication |
| `Presence refresh failed` | Xbox Live API issue |

## Troubleshooting

### Service Won't Start

```bash
sudo journalctl -u axelerator -n 50 --no-pager
```

Common causes:
- Config file syntax error
- Token file missing/expired
- Permission denied on files

### Token Expired

Tokens are valid for ~14 days. If you see auth errors:

```bash
sudo systemctl stop axelerator
sudo rm /opt/axelerator/token.json
sudo -u axelerator /opt/axelerator/axelerator -c /opt/axelerator/axelerator.toml
# Authenticate, then Ctrl+C
sudo systemctl start axelerator
```

### Frequent Disconnects

If "Reconnecting transfer server" appears often:
1. Check network stability
2. Verify Xbox Live services status
3. Check for Xbox account restrictions
4. Review error messages in logs

### Players Can't Connect

1. Verify Minecraft server is running
2. Check `server_ip` and `server_port` in config
3. Ensure session is visible (logs show "Session created")
4. Check firewall allows UDP to Minecraft port

## Security

### File Permissions

```bash
sudo chmod 600 /opt/axelerator/token.json
sudo chmod 644 /opt/axelerator/axelerator.toml
sudo chown axelerator:axelerator /opt/axelerator/*
```

### Firewall

Axelerator needs:
- **Outbound**: HTTPS (443) for Xbox Live APIs
- **No inbound ports**: WebRTC uses TURN relays

### Attack Detection

Enable monitoring to detect session tampering:

```toml
monitor_tampering = true
monitor_interval = 30
auto_block_attackers = true
```

## Updating

```bash
cargo build --release -p axelerator
sudo systemctl stop axelerator
sudo cp target/release/axelerator /opt/axelerator/
sudo systemctl start axelerator
```

## Uninstalling

```bash
sudo systemctl stop axelerator
sudo systemctl disable axelerator
sudo rm /etc/systemd/system/axelerator.service
sudo rm -rf /opt/axelerator
sudo userdel axelerator
sudo systemctl daemon-reload
```
