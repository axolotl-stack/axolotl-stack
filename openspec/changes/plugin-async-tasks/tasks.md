# Tasks

## 1. Timers

- [ ] 1.1 Fix timer dispatch to deliver only to the owning plugin (test: two plugins subscribed to `timer`, one sets a timer, only it receives `on-timer`)
- [ ] 1.2 Add the `set-timer(delay-ticks) -> u64` import to `crates/unastar-api/wit/host.wit` and implement it over `TimerManager::set_timer`
- [ ] 1.3 Example plugin uses `set-timer` and subscribes to `timer`

## 2. HTTP tasks

- [ ] 2.1 Add `http-request` import and `on-http-response` / `on-http-error` exports to the WIT world
- [ ] 2.2 Host: background request task, completion queue drained at tick start, typed errors
- [ ] 2.3 Gate on the `network` capability plus server config; per-plugin host allowlist, rate limit, 30 s timeout, response size cap, in-flight cap
- [ ] 2.4 Cancel pending tasks when a plugin is dropped
- [ ] 2.5 Tests: denied without capability, delivered on a later tick, timeout path

## 3. Versioning and docs

- [ ] 3.1 Bump the WIT package version and the expected `api_version`; regenerate `unastar_api::Guest` bindings
- [ ] 3.2 Update `docs/guides/plugins.md` and the plugin macros
- [ ] 3.3 Record the result-delivery-timing decision (`record-decision`)

## 4. Validate

- [ ] 4.1 `cargo fmt --all -- --check`
- [ ] 4.2 `cargo clippy -p unastar -p unastar-api -p unastar-api-macros --all-targets -- -D warnings`
- [ ] 4.3 `cargo test -p unastar --lib --tests`
- [ ] 4.4 Build the example plugin for `wasm32-wasip2` and load it in a test
- [ ] 4.5 `openspec validate plugin-async-tasks --strict`; run `review-change`
