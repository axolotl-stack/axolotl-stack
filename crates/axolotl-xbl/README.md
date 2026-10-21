# Axolotl XBL

**Axolotl XBL** is a standalone, async Rust library for interacting with Xbox Live services. It is designed to handle the complex authentication flows required for Minecraft: Bedrock Edition servers and clients.

## Features

- **Token Exchange**: Handle XSTS token retrieval and exchange.
- **Device Code Flow**: Authenticate seamlessly using the device login flow.
- **Minecraft Services**: specifically tailored for `minecraftservices.com` authentication.
- **PlayFab Integration**: Support for PlayFab login flows (gatekeeper for NetherNet).

## Usage

```toml
[dependencies]
axolotl_xbl = "0.1"
```

## Xbox authentication

OAuth uses Minecraft Android's client ID and Windows Live scope from
[go-xsapi `MinecraftAndroid`](https://github.com/df-mc/go-xsapi/blob/90ce120e31cdf8de6c7225ca1378117c23081ddd/example_test.go).
Device authentication uses Android 13, a P-256 proof key, and the matching XAL
user-agent. SISU obtains title/user/default Xbox tokens; other relying parties
use signed XSTS requests with those underlying tokens. Tokens are cached per
relying party with the upstream one-minute expiration margin.

`XblTokenClient::get_xbl_token_for_url` resolves title/default NSAL data before
choosing a relying party. Service requests sign the actual outgoing headers and
body limit selected by NSAL. Rejected credentials can be invalidated without
discarding the device/proof-key context.

Wire references: go-xsapi `90ce120e31cdf8de6c7225ca1378117c23081ddd`,
`xal/sisu/{oauth2.go,session.go}`, `xal/xasd/authenticate.go`,
`xal/xsts/authorize.go`, and `xal/nsal/{title.go,resolver.go,transport.go}`.
HTTP and JSON error sources are reduced to non-secret status/category metadata:
formatting an error chain cannot disclose response bodies or credential URLs.


## PlayFab and franchise services

Resolve the authorization environment with
`DiscoveryClient::discover_authorization(game_version)`, construct a reusable
`PlayFabClient::with_environment(&environment)`, and request an Xbox token for
`client.login_url()`. Pass the resulting PlayFab session ticket and the actual
game version to `client.service_token(device_id, ticket, game_version)`.
Axelerator defaults to the generated `jolyne::valentine::GAME_VERSION`.

The client caches PlayFab tickets for 23 hours 45 minutes. Service tokens are
reused until one minute before expiry, renewed while still before hard expiry,
and replaced with a fresh session if renewal fails. Keep the client alive across
refreshes. `invalidate_service_token(rejected_header)` invalidates only the
matching token, so a delayed authentication failure cannot erase a newer token.
Discovery caches raw responses by request URL and decodes only the requested
production environment.

`SessionResult` exposes the authorization header, expiration, and player
messaging UUID. As in the Go token source, service-token JWT claims are decoded
and time-validated, not signature-verified. This is not a multiplayer identity
verifier. `multiplayer_token` obtains a fresh connection-specific token from a
caller-supplied SPKI public key; verification of peer identity tokens is a
separate responsibility.

Wire references:

- df-mc/go-playfab `e01ef1ec605c06f2bf56b1b7022fa9e13a9a69fc`:
  `xbox.go:XBLIdentityProvider.Login`, `login.go:LoginRequest/LoginResult.Valid`,
  and `client.go:Client.login`.
- sandertv/gophertunnel `be557e33fe6f8d0c0db7ef05b5fd068725c325ad`:
  `minecraft/service/discovery.go:Discover/Discovery.Environment`,
  `token.go:AuthorizationEnvironment.Token/Renew/MultiplayerToken`,
  `token.go:defaultDeviceConfig/decodeClaims`, and
  `token_source.go:tokenSource.ServiceToken`.

HTTP errors retain status and static operation context, never response bodies.
Authentication object `Debug` output is redacted. Regression tests use only
loopback HTTP servers and do not contact live authentication services.

## Multiplayer token verification

`MultiplayerTokenVerifier::new(trusted_issuer)` resolves OpenID configuration and
verifies peer JWT signatures, issuer, multiplayer audience, and token times.
It caches signing keys, coalesces concurrent fetches, and permits an unknown
key ID to refresh keys after the upstream 30-minute interval. A bad signature
for a known key does not trigger repeated key downloads. Microsoft hex-encoded
SHA-1 `x5t` values are normalized before decoding keys.

Sources: gophertunnel `minecraft/service/token.go:VerifierContext/configuration`
and `jwks.go:refreshingKeySet`. The Rust JWT backend rejects unsupported advertised
algorithms, including ES512, instead of substituting another algorithm. Issuer
URLs with embedded credentials, query parameters, or fragments are rejected.

## Friend broadcasting compatibility

The friend broadcaster retains its franchise WebSocket transport and MPSD
connection type 3. Current go-nethernet/gophertunnel do not implement the
franchise player-messaging signaling service; their HTTP `endpoint` transport
handles address joins, not routing an Xbox friend session to a listener.
The go-xsapi type-7/PmsgId example is therefore not advertised by this path.
Social-summary counts, friend-limit policy, and screenshot/gallery APIs remain
explicit application extensions. Unused speculative MPSD enumeration APIs were
removed rather than retained as unsupported aliases.
