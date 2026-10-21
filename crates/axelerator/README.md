## Usage

```bash
cargo run -p axelerator
```

## Token cache security

OAuth credentials are saved to a uniquely named temporary file in the cache
file's directory, flushed and synced, then atomically renamed over the old file.
Readers observe complete old or new JSON rather than a truncated cache.

On Unix the temporary file is created with mode `0600`. On Windows it is created
with a protected DACL granting full access only to the current process user's
SID, with inheritance disabled. The DACL is checked before credential bytes are
written; filesystems that cannot enforce it fail closed. These protections also
apply to custom cache paths and replace an existing file's permissive ACL/mode.
Use a directory you control: file permissions cannot prevent a directory owner
from deleting or replacing directory entries. Privileged administrators and
backup operators remain outside this protection boundary.

Cache Debug output redacts credentials, and refresh warnings omit server error
details. Loaded OAuth tokens retain their original expiry. Refresh eligibility
depends on a refresh token being present, not a guessed 90-day lifetime, matching
`go-xsapi/xal/sisu/oauth2.go: tokenRefresher.Token`. Actual refresh validity is
decided by the authentication server.

Malformed/unreadable cache files and failed token refreshes return errors instead
of silently starting an interactive login. A missing cache file is still a normal
first-login case. Configuration diagnostics redact Discord webhook credentials,
including parse errors and nested `Debug` output.

## Authentication and broadcasting

Game version and protocol defaults come from the generated
`jolyne::valentine::{GAME_VERSION, PROTOCOL_VERSION}`. The configured game version
is also sent to Minecraft service discovery and franchise session creation.
Authorization uses the discovered service URL/PlayFab title and NSAL-selected
Xbox scope. PlayFab and Minecraft service-token caches survive listener restarts;
a rejected service token is invalidated before retrying authentication.

The existing franchise WebSocket friend-join transport and MPSD connection type 3
are retained: current upstream HTTP endpoint signaling is an address-join
transport, not a drop-in friend-join replacement. RTA must establish a real
connection ID before a session is published; resynchronization requests trigger
social refreshes. Missing relationship flags fail closed instead of causing
automatic unfollows.

The disabled `xbox_client` diagnostic binary used nonexistent research APIs and
was removed; `axelerator` remains the supported runnable binary.
