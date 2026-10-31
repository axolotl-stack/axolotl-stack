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
    let _: fn(
        &unastar_api::Player,
        Option<&unastar_api::Player>,
        unastar_api::DamageCause,
        f32,
    ) -> Option<f32> = <__UnastarPluginBridge as Guest>::on_player_damage;
    let _: fn(&unastar_api::Player, unastar_api::Vec3) -> Option<unastar_api::Vec3> =
        <__UnastarPluginBridge as Guest>::on_player_respawn;
    <__UnastarPluginBridge as Guest>::on_load();
    <__UnastarPluginBridge as Guest>::on_tick(42);
    assert_eq!(LAST_TICK.load(Ordering::SeqCst), 42);
    <__UnastarPluginBridge as Guest>::on_unload();
}
