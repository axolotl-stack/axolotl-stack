## ADDED Requirements

### Requirement: Plugin timers fire on the owning plugin
The host SHALL provide a `set-timer(delay-ticks)` import that returns a timer id without blocking. When the delay elapses the host MUST call `on-timer(id)` on the plugin that set the timer and on no other plugin.

#### Scenario: Timer reaches only its owner
- **WHEN** plugins A and B both subscribe to `timer` and only A calls `set-timer(20)`
- **THEN** 20 ticks later A's `on-timer` is called with the returned id
- **AND** B's `on-timer` is not called

### Requirement: HTTP requests are capability-gated tasks
The host SHALL provide an `http-request` import that returns a task id without blocking, performs the request outside the tick, and later calls `on-http-response` or `on-http-error` on the requesting plugin. A plugin without the `network` capability granted MUST receive an error and no request is made.

#### Scenario: Request without the network capability
- **WHEN** a plugin whose manifest does not grant `network` calls `http-request`
- **THEN** the call returns an error and no network connection is opened

#### Scenario: Result delivered on a later tick
- **WHEN** a plugin with `network` granted calls `http-request` for an allowed host
- **THEN** the response is delivered through `on-http-response` with the same task id
- **AND** it is never delivered in the tick in which the request was made

#### Scenario: Request times out
- **WHEN** an allowed host does not answer within 30 seconds
- **THEN** `on-http-error` is called with a timeout error for that task id
