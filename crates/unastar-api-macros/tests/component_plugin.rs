use std::sync::atomic::{AtomicU64, Ordering};
use unastar_api::{Guest, Plugin};
use unastar_api_macros::plugin;

static LAST_TICK: AtomicU64 = AtomicU64::new(0);

struct ComponentPlugin;

// This consumer must compile without serialization dependencies or a Default
// implementation: registration uses the current WIT-generated Guest bridge.
#[plugin]
impl Plugin for ComponentPlugin {
    fn on_tick(tick_id: u64) {
        LAST_TICK.store(tick_id, Ordering::SeqCst);
    }
}

#[test]
fn component_export_dispatches_to_the_plugin() {
    <__UnastarPluginBridge as Guest>::on_load();
    <__UnastarPluginBridge as Guest>::on_tick(42);
    assert_eq!(LAST_TICK.load(Ordering::SeqCst), 42);
    <__UnastarPluginBridge as Guest>::on_unload();
}
