use bevy_ecs::prelude::Entity;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use tracing::{error, info, warn};
use wasmtime::component::{Component, Linker, Resource, ResourceTable};
use wasmtime::{Engine, Store};
use wasmtime_wasi::{WasiCtx, WasiCtxView, WasiView};

use crate::ecs::events::{PluginAction, ServerEvent};

#[path = "commands.rs"]
pub(crate) mod commands;
#[path = "guest_runtime.rs"]
mod guest_runtime;
#[path = "lifecycle.rs"]
mod lifecycle;
#[path = "players.rs"]
pub(crate) mod players;
#[path = "readiness.rs"]
pub(crate) mod readiness;
#[path = "storage_bridge.rs"]
mod storage_bridge;
#[path = "tasks.rs"]
mod tasks;
#[path = "ui.rs"]
mod ui;
#[path = "views.rs"]
pub(crate) mod views;

#[path = "blocks.rs"]
mod blocks;
#[path = "damage.rs"]
mod damage;
#[path = "deaths.rs"]
mod deaths;
#[path = "item_actions.rs"]
mod item_actions;
#[path = "movements.rs"]
mod movements;
#[path = "sessions.rs"]
pub(crate) mod sessions;

/// Component API version supported by this host.
pub const PLUGIN_API_VERSION: &str = "0.4.0";
use crate::plugin::manifest::{PluginId, PluginLimits, PluginManifest};

// ============================================================================
// bindgen! — generates UnastarPlugin, Host traits, types, etc.
// Wrapped in a module to avoid collision with the `unastar` crate name.
// ============================================================================

mod wit_bindings {
    wasmtime::component::bindgen!({
        path: "../unastar-api/wit",
        world: "unastar-plugin",
    });
}

use wit_bindings::UnastarPlugin;
use wit_bindings::unastar::plugin::types as wit_types;

// ============================================================================
// Host Context
// ============================================================================

/// Per-plugin host state, stored inside `Store<HostContext>`.
pub struct HostContext {
    wasi_ctx: WasiCtx,
    table: ResourceTable,
    plugin_id: PluginId,
    /// Attached only inside `with_world`, including instantiation and on-load.
    world_ptr: *mut bevy_ecs::world::World,
    /// Actions accumulated during a single WASM call.
    pending_actions: Vec<PluginAction>,
    resources: players::PlayerResources,
    api_limits: PluginLimits,
    capabilities: HashSet<crate::plugin::manifest::PluginCapability>,
    generation: uuid::Uuid,
    pending_messages: Vec<players::BoundMessage>,
    pending_mechanics: Vec<players::MechanicsRequest>,
    mechanics_accepted: bool,
    mechanics_owner: std::sync::Arc<()>,
    active_admission: Option<sessions::ActiveAdmission>,
    pending_admissions: Vec<sessions::StagedResolution>,
    pending_tasks: Vec<tasks::TaskRequest>,
    task_requests: VecDeque<tasks::TaskRequest>,
    task_failures: VecDeque<tasks::TaskFailure>,
    task_sequence: u64,
    command_roots: commands::CommandRoots,
    command_owner: std::sync::Arc<()>,
    pending_command_roots: Vec<commands::Reservation>,
    ui_registry: ui::Registry,
    ui_lease: std::sync::Arc<()>,
    pending_ui: Vec<ui::Action>,
    ui_updates: u32,
    view_registry: views::Registry,
    view_lease: std::sync::Arc<()>,
    pending_views: Vec<views::Action>,
    view_updates: u32,
    #[cfg(test)]
    test_logs: Vec<String>,
    #[cfg(test)]
    test_log_epoch_engine: Option<Engine>,
    limits: wasmtime::StoreLimits,
    /// Immutable opaque bytes loaded before component instantiation.
    configuration: Vec<u8>,
    storage_registry: std::sync::Arc<std::sync::Mutex<super::storage::Registry>>,
    storage_namespace: Option<super::storage::SharedNamespace>,
    io_budget: super::io_budget::IoBudget,
}

// Safety: We only access world_ptr from the thread that owns the World (during system execution).
unsafe impl Send for HostContext {}

impl WasiView for HostContext {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi_ctx,
            table: &mut self.table,
        }
    }
}

impl wasmtime::component::HasData for HostContext {
    type Data<'a> = &'a mut HostContext;
}

impl HostContext {
    fn new(wasi_ctx: WasiCtx, plugin_id: PluginId, limits: &PluginLimits) -> Self {
        Self {
            wasi_ctx,
            table: ResourceTable::new(),
            plugin_id,
            world_ptr: std::ptr::null_mut(),
            pending_actions: Vec::new(),
            resources: players::PlayerResources::default(),
            api_limits: limits.clone(),
            capabilities: HashSet::new(),
            generation: uuid::Uuid::new_v4(),
            pending_messages: Vec::new(),
            pending_mechanics: Vec::new(),
            mechanics_accepted: true,
            mechanics_owner: std::sync::Arc::new(()),
            active_admission: None,
            pending_admissions: Vec::new(),
            pending_tasks: Vec::new(),
            task_requests: VecDeque::new(),
            task_failures: VecDeque::new(),
            task_sequence: 0,
            command_roots: Default::default(),
            command_owner: std::sync::Arc::new(()),
            pending_command_roots: Vec::new(),
            ui_registry: Default::default(),
            ui_lease: std::sync::Arc::new(()),
            pending_ui: Vec::new(),
            ui_updates: 0,
            view_registry: Default::default(),
            view_lease: std::sync::Arc::new(()),
            pending_views: Vec::new(),
            view_updates: 0,
            #[cfg(test)]
            test_logs: Vec::new(),
            #[cfg(test)]
            test_log_epoch_engine: None,
            limits: wasmtime::StoreLimitsBuilder::new()
                .memory_size(limits.max_memory)
                .trap_on_grow_failure(true)
                .build(),
            configuration: Vec::new(),
            storage_registry: Default::default(),
            storage_namespace: None,
            io_budget: Default::default(),
        }
    }

    /// Get an immutable reference to the ECS World.
    /// # Safety
    /// Caller must ensure `world_ptr` is valid.
    unsafe fn world(&self) -> &bevy_ecs::world::World {
        assert!(!self.world_ptr.is_null(), "plugin world is not attached");
        unsafe { &*self.world_ptr }
    }

    /// Access the one world exclusively attached for this callback.
    fn world_mut(&mut self) -> &mut bevy_ecs::world::World {
        assert!(!self.world_ptr.is_null(), "plugin world is not attached");
        // `with_world` exclusively borrows the World for the entire host call.
        unsafe { &mut *self.world_ptr }
    }
}

// ============================================================================
// Plugin Manager
// ============================================================================

pub struct PluginManager {
    engine: Engine,
    plugins: HashMap<PluginId, LoadedPlugin>,
    pending: Vec<PendingPlugin>,
    tick_prepared: bool,
    tick_budget_valid: bool,
    readiness: readiness::PluginReadiness,
    storage_registry: std::sync::Arc<std::sync::Mutex<super::storage::Registry>>,
    command_roots: commands::CommandRoots,
    ui_registry: ui::Registry,
    view_registry: views::Registry,
}

struct LoadedPlugin {
    manifest: PluginManifest,
    store: Store<HostContext>,
    bindings: UnastarPlugin,
}

struct PendingPlugin {
    manifest: PluginManifest,
    component: Component,
    store: Store<HostContext>,
}

/// Scope the raw host pointer to an exclusive World borrow, clearing it even
/// when a host panics. No caller may access the World while Wasm is running.
fn with_world<R>(
    store: &mut Store<HostContext>,
    world: &mut bevy_ecs::world::World,
    call: impl FnOnce(&mut Store<HostContext>) -> R,
) -> R {
    struct Attachment<'a> {
        store: &'a mut Store<HostContext>,
        _world: std::marker::PhantomData<&'a mut bevy_ecs::world::World>,
    }
    impl Drop for Attachment<'_> {
        fn drop(&mut self) {
            let ctx = self.store.data_mut();
            ctx.world_ptr = std::ptr::null_mut();
            ctx.resources.end_scope();
            ctx.active_admission = None;
        }
    }
    store.data_mut().world_ptr = world;
    store.data_mut().resources.begin_scope(world);
    let attachment = Attachment {
        store,
        _world: std::marker::PhantomData,
    };
    call(attachment.store)
}

/// Enter a synchronous guest on its world owner, with Tokio-safe WASI I/O.
/// Raw imports/tests may use `with_world`; production exports use this boundary.
pub(super) fn with_guest_world<R>(
    store: &mut Store<HostContext>,
    world: &mut bevy_ecs::world::World,
    call: impl FnOnce(&mut Store<HostContext>) -> anyhow::Result<R>,
) -> anyhow::Result<R> {
    guest_runtime::call(|| with_world(store, world, call))
}

/// One completion point commits staged imports only after a successful guest export.
pub(super) fn finish_invocation(
    context: &mut HostContext,
    world: &mut bevy_ecs::world::World,
    succeeded: bool,
) {
    players::finish_mechanics(context, world, succeeded && context.mechanics_accepted);
    commands::finish_invocation(context, succeeded);
    players::finish_admission_invocation(context, world, succeeded);

    ui::finish_invocation(context, world, succeeded);
    views::finish_invocation(context, world, succeeded);
    players::finish_invocation(context, world, succeeded);
}

/// All worlds and callbacks in one plugin tick share an instruction budget.
fn begin_fuel_budget(store: &mut Store<HostContext>, limits: &PluginLimits) -> anyhow::Result<()> {
    players::begin_budget(store.data_mut());
    store.data_mut().ui_updates = 0;
    store.data_mut().view_updates = 0;
    store.set_fuel(limits.fuel_per_tick)
}

/// Wall-clock interruption is independent of the shared per-tick fuel budget.
fn prepare_invocation(store: &mut Store<HostContext>) {
    store.data_mut().mechanics_accepted = true;
    let timeout = store.data().api_limits.callback_timeout_ms;
    prepare_deadline(store, timeout);
}

/// Set one bounded CPU allowance; only measured trusted storage time can extend it.
fn prepare_deadline(store: &mut Store<HostContext>, timeout_ms: u32) {
    let allowance = std::time::Duration::from_millis(u64::from(timeout_ms));
    store
        .data_mut()
        .io_budget
        .reset_with_allowance(std::time::Instant::now(), allowance);
    let epochs =
        u64::from(timeout_ms).div_ceil(super::io_budget::EPOCH_INTERVAL.as_millis() as u64);
    store.set_epoch_deadline(epochs);
}

/// Instantiation and on-load share this single finite budget, never refreshed between them.
fn begin_initialization_budget(
    store: &mut Store<HostContext>,
    limits: &PluginLimits,
) -> anyhow::Result<()> {
    limits.validate_execution()?;
    players::begin_budget(store.data_mut());
    store.set_fuel(limits.fuel_per_initialization)?;
    prepare_deadline(store, limits.initialization_timeout_ms);
    Ok(())
}

/// Preserve a useful coarse cause through layered Wasmtime call errors.
pub(super) fn wasm_failure_reason(error: &anyhow::Error) -> String {
    let category = match error.downcast_ref::<wasmtime::Trap>() {
        Some(wasmtime::Trap::OutOfFuel) => "guest fuel exhausted",
        Some(wasmtime::Trap::Interrupt) => "guest execution deadline exhausted",
        Some(_) => "guest trapped",
        None => "guest host call failed",
    };
    format!("{category}: {error}")
}

fn create_store(
    engine: &Engine,
    wasi_ctx: WasiCtx,
    plugin_id: PluginId,
    limits: &PluginLimits,
) -> anyhow::Result<Store<HostContext>> {
    limits.validate_execution()?;
    let mut store = Store::new(engine, HostContext::new(wasi_ctx, plugin_id, limits));
    store.limiter(|ctx| &mut ctx.limits);
    store.epoch_deadline_callback(|context| {
        if context
            .data()
            .io_budget
            .may_continue(std::time::Instant::now())
        {
            Ok(wasmtime::UpdateDeadline::Continue(1))
        } else {
            Err(wasmtime::Trap::Interrupt.into())
        }
    });
    begin_fuel_budget(&mut store, limits)?;
    prepare_invocation(&mut store);
    Ok(store)
}

/// BlockClick emits interaction and placement consecutively. Cancelling the
/// clicked-face interaction must also cancel its dependent placement intent.
fn placement_depends_on_interaction(previous: &ServerEvent, event: &ServerEvent) -> bool {
    let (
        ServerEvent::PlayerInteractBlock {
            entity: clicked_entity,
            position: clicked,
            face,
        },
        ServerEvent::BlockPlace {
            entity, position, ..
        },
    ) = (previous, event)
    else {
        return false;
    };
    let offset = match face {
        0 => (0, -1, 0),
        1 => (0, 1, 0),
        2 => (0, 0, -1),
        3 => (0, 0, 1),
        4 => (-1, 0, 0),
        5 => (1, 0, 0),
        _ => return false,
    };
    entity == clicked_entity
        && i64::from(position.0) == i64::from(clicked.0) + offset.0
        && i64::from(position.1) == i64::from(clicked.1) + offset.1
        && i64::from(position.2) == i64::from(clicked.2) + offset.2
}

impl PluginManager {
    /// Configure isolated storage before discovering or instantiating components.
    pub(crate) fn configure_storage(
        &mut self,
        config: &crate::config::PluginStorageConfig,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.plugins.is_empty() && self.pending.is_empty(),
            "component storage must be configured before discovery"
        );
        config
            .limits()
            .validate()
            .map_err(|_| anyhow::anyhow!("invalid component storage limits"))?;
        self.storage_registry = std::sync::Arc::new(std::sync::Mutex::new(
            super::storage::Registry::new(config.directory.clone(), config.limits()),
        ));
        Ok(())
    }
    pub fn new() -> Result<Self, anyhow::Error> {
        let mut config = wasmtime::Config::new();
        config.wasm_component_model(true);
        config.epoch_interruption(true);
        config.consume_fuel(true);

        let engine = Engine::new(&config)?;

        // Increment the epoch every 5ms so that long-running plugin exports
        // trap instead of blocking the game thread indefinitely.
        // Uses the tokio runtime already running for network I/O.
        let epoch_engine = engine.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(super::io_budget::EPOCH_INTERVAL);
            loop {
                interval.tick().await;
                epoch_engine.increment_epoch();
            }
        });

        Ok(Self {
            engine,
            plugins: HashMap::new(),
            pending: Vec::new(),
            tick_prepared: false,
            tick_budget_valid: true,
            readiness: Default::default(),
            storage_registry: Default::default(),
            command_roots: Default::default(),
            ui_registry: Default::default(),
            view_registry: Default::default(),
        })
    }

    /// Discover and compile components. Initialization attaches the live world
    /// before the production listener starts, or on a standalone manager's first cycle.
    pub fn load_plugins(&mut self, plugins_dir: &Path) -> Result<(), anyhow::Error> {
        if !plugins_dir.exists() {
            info!(path = %plugins_dir.display(), "Plugins directory not found, creating it.");
            std::fs::create_dir_all(plugins_dir)?;
            return self
                .readiness
                .discovered(&Default::default())
                .map_err(anyhow::Error::msg);
        }

        self.storage_registry
            .lock()
            .map_err(|_| anyhow::anyhow!("component storage registry is unavailable"))?
            .validate_directory(plugins_dir)
            .map_err(|_| {
                anyhow::anyhow!(
                    "component storage must be outside filesystem preopens with valid limits"
                )
            })?;

        let mut found_any = false;
        for entry in std::fs::read_dir(plugins_dir)? {
            let entry = entry?;
            found_any = true;
            let path = entry.path();
            if path.is_dir() {
                info!(plugin_dir = %path.display(), "Found potential plugin directory");
                if let Err(e) = self.load_plugin(&path) {
                    error!(path = %path.display(), error = %e, "Failed to load plugin");
                }
            }
        }

        if !found_any {
            info!("Plugins directory is empty, no plugins to load.");
        }

        let present = self
            .plugins
            .keys()
            .map(|id| id.0.clone())
            .chain(
                self.pending
                    .iter()
                    .map(|plugin| plugin.manifest.id.0.clone()),
            )
            .collect();
        self.readiness
            .discovered(&present)
            .map_err(anyhow::Error::msg)
    }

    /// Load a single plugin from a directory.
    fn load_plugin(&mut self, plugin_dir: &Path) -> Result<(), anyhow::Error> {
        // 1. Read manifest
        let manifest_path = plugin_dir.join("plugin.toml");
        if !manifest_path.exists() {
            return Err(anyhow::anyhow!(
                "Manifest file not found at {:?}",
                manifest_path
            ));
        }
        let manifest_content = std::fs::read_to_string(&manifest_path)?;
        let manifest: PluginManifest = toml_edit::de::from_str(&manifest_content)?;
        let id = manifest.id.clone();
        if manifest.required {
            self.readiness.require(&id.0);
        }
        let result = self.prepare_plugin(plugin_dir, manifest);
        if let Err(error) = &result {
            self.readiness.fail(&id.0, &error.to_string());
        }
        result
    }

    /// Compile one manifest and cache its bounded configuration before instantiation.
    fn prepare_plugin(
        &mut self,
        plugin_dir: &Path,
        manifest: PluginManifest,
    ) -> Result<(), anyhow::Error> {
        if manifest.api_version != PLUGIN_API_VERSION {
            anyhow::bail!(
                "Plugin API {} is unsupported; rebuild against {}",
                manifest.api_version,
                PLUGIN_API_VERSION
            );
        }
        let plugin_id = manifest.id.clone();

        if self.plugins.contains_key(&plugin_id)
            || self
                .pending
                .iter()
                .any(|plugin| plugin.manifest.id == plugin_id)
        {
            anyhow::ensure!(
                !self.readiness.is_required(&plugin_id.0),
                "duplicate required component ID {plugin_id}"
            );
            warn!(id = %plugin_id, "Duplicate plugin ID found, skipping");
            return Ok(());
        }

        let configuration = readiness::configuration(plugin_dir, manifest.config.as_deref())?;

        // 2. Load WASM component
        let wasm_path = plugin_dir.join("plugin.wasm");
        if !wasm_path.exists() {
            return Err(anyhow::anyhow!("WASM file not found at {:?}", wasm_path));
        }

        let component = Component::from_file(&self.engine, &wasm_path)?;

        // 4. Build WasiCtx
        let mut wasi_builder = WasiCtx::builder();
        wasi_builder.inherit_stdout().inherit_stderr();
        if manifest
            .capabilities
            .contains(&crate::plugin::manifest::PluginCapability::Filesystem)
        {
            self.storage_registry
                .lock()
                .map_err(|_| anyhow::anyhow!("component storage registry is unavailable"))?
                .validate_directory(plugin_dir)
                .map_err(|_| anyhow::anyhow!("component filesystem overlaps durable storage"))?;
            let dir_perm = wasmtime_wasi::DirPerms::all();
            let file_perm = wasmtime_wasi::FilePerms::all();
            wasi_builder.preopened_dir(plugin_dir, ".", dir_perm, file_perm)?;
        }
        let wasi_ctx = wasi_builder.build();

        let mut store = create_store(&self.engine, wasi_ctx, plugin_id, &manifest.limits)?;
        store.data_mut().configuration = configuration;
        store.data_mut().storage_registry = self.storage_registry.clone();
        tracing::debug!(
            config_bytes = store.data().configuration.len(),
            "Component configuration prepared"
        );
        store.data_mut().capabilities = manifest.capabilities.clone();
        // Runtime discovery happens before the game World is available.
        // Defer *instantiation* too: component start functions can call hosts.
        self.pending.push(PendingPlugin {
            manifest,
            component,
            store,
        });

        Ok(())
    }

    fn initialize_pending(&mut self, world: &mut bevy_ecs::world::World) {
        let mut staged = Vec::new();
        for mut pending in self.pending.drain(..) {
            if pending.manifest.required {
                self.readiness.require(&pending.manifest.id.0);
            }
            pending.store.data_mut().command_roots = self.command_roots.clone();
            pending.store.data_mut().ui_registry = self.ui_registry.clone();
            pending.store.data_mut().view_registry = self.view_registry.clone();
            pending.store.data_mut().plugin_id = pending.manifest.id.clone();
            pending.store.data_mut().capabilities = pending.manifest.capabilities.clone();
            let result =
                with_guest_world(&mut pending.store, world, |store| -> anyhow::Result<_> {
                    let mut linker = Linker::<HostContext>::new(&self.engine);
                    wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;
                    UnastarPlugin::add_to_linker::<HostContext, HostContext>(&mut linker, |ctx| {
                        ctx
                    })?;
                    begin_initialization_budget(store, &pending.manifest.limits)?;
                    let bindings =
                        UnastarPlugin::instantiate(&mut *store, &pending.component, &linker)?;
                    bindings.call_on_load(&mut *store)?;
                    Ok(bindings)
                });
            match result {
                Ok(bindings) => {
                    commands::finish_invocation(pending.store.data_mut(), true);
                    staged.extend(std::mem::take(
                        &mut pending.store.data_mut().pending_actions,
                    ));
                    self.readiness.initialized(&pending.manifest.id.0);
                    info!(id = %pending.manifest.id, "Plugin loaded (Component Model)");
                    self.plugins.insert(
                        pending.manifest.id.clone(),
                        LoadedPlugin {
                            manifest: pending.manifest,
                            store: pending.store,
                            bindings,
                        },
                    );
                }
                Err(error) => {
                    commands::finish_invocation(pending.store.data_mut(), false);
                    error!(id = %pending.manifest.id, %error, "Plugin initialization failed");
                    if self
                        .readiness
                        .fail(&pending.manifest.id.0, &wasm_failure_reason(&error))
                    {
                        break;
                    }
                }
            }
        }
        let succeeded = self.readiness.check().is_ok();
        for plugin in self.plugins.values_mut() {
            finish_invocation(plugin.store.data_mut(), world, succeeded);
        }
        if succeeded {
            for action in staged {
                world
                    .resource_mut::<crate::ecs::events::ActionQueue>()
                    .push(action);
            }
        } else {
            readiness::fence_world(world);
        }
    }

    /// Run a complete standalone world cycle. Routers drain individual worlds
    /// before dispatching their single aggregate tick and ending its budget.
    pub fn tick(&mut self, ecs_world: &mut bevy_ecs::world::World) {
        self.drain_world_events(ecs_world);
        self.dispatch_global_tick(ecs_world);
        self.drain_ui_local(ecs_world);
        self.publish_command_roots(ecs_world);
        self.publish_ui(ecs_world);
        self.end_tick();
    }

    /// Drain local events without synthesizing a game tick or renewing fuel.
    pub(crate) fn drain_world_events(&mut self, ecs_world: &mut bevy_ecs::world::World) {
        self.begin_tick(ecs_world);
        let events = ecs_world
            .resource_mut::<crate::ecs::events::EventBuffer>()
            .drain();
        self.dispatch_events(ecs_world, events);
        crate::server::game::item_actions::clear_blocks(ecs_world);
    }

    /// Dispatch one aggregate tick while retaining the budget for queued work.
    pub(crate) fn dispatch_global_tick(&mut self, ecs_world: &mut bevy_ecs::world::World) {
        self.begin_tick(ecs_world);
        let tick_id = ecs_world
            .resource::<crate::ecs::resources::TickCounter>()
            .current;
        self.dispatch_events(ecs_world, vec![ServerEvent::Tick { tick_id }]);
    }

    /// End the shared cycle only after its deferred callbacks have completed.
    pub(crate) fn end_tick(&mut self) {
        self.tick_prepared = false;
    }

    /// Apply a buffered batch without changing the component's cycle budget.
    fn dispatch_events(
        &mut self,
        ecs_world: &mut bevy_ecs::world::World,
        all_events: Vec<ServerEvent>,
    ) {
        let budget_valid = self.tick_budget_valid;
        let readiness = self.readiness.clone();
        if readiness.check().is_err() {
            return;
        }

        if all_events.is_empty() {
            return;
        }

        // Track cancelled event indices
        let mut cancelled_indices = std::collections::HashSet::new();

        for plugin in self.plugins.values_mut() {
            plugin.store.data_mut().pending_actions.clear();
            for (index, event) in all_events.iter().enumerate() {
                if index > 0
                    && cancelled_indices.contains(&(index - 1))
                    && placement_depends_on_interaction(&all_events[index - 1], event)
                {
                    cancelled_indices.insert(index);
                    continue;
                }
                if !plugin.manifest.subscriptions.contains(&event.kind()) {
                    continue;
                }
                // Full transaction subscribers supersede the scalar compatibility callbacks.
                if plugin
                    .manifest
                    .subscriptions
                    .contains(&crate::ecs::events::EventKind::BlockTransaction)
                    && matches!(
                        event,
                        ServerEvent::BlockBreak { .. }
                            | ServerEvent::BlockPlace { .. }
                            | ServerEvent::PlayerInteractBlock { .. }
                    )
                {
                    continue;
                }
                if plugin
                    .manifest
                    .subscriptions
                    .contains(&crate::ecs::events::EventKind::ItemAction)
                    && matches!(event, ServerEvent::PlayerItemUse { .. })
                {
                    continue;
                }
                if cancelled_indices.contains(&index) {
                    continue;
                }

                let cancelled =
                    !budget_valid || Self::dispatch_event(plugin, event, ecs_world, &readiness);

                // Drain pending actions after each call
                let actions: Vec<PluginAction> =
                    std::mem::take(&mut plugin.store.data_mut().pending_actions);
                if !actions.is_empty() {
                    let mut action_queue = ecs_world
                        .get_resource_mut::<crate::ecs::events::ActionQueue>()
                        .unwrap();
                    for action in actions {
                        action_queue.push(action);
                    }
                }

                if readiness.check().is_err() {
                    self.tick_budget_valid = false;
                    return;
                }
                if cancelled {
                    cancelled_indices.insert(index);
                }
            }
        }

        // Execute default behavior for non-cancelled events
        for (index, event) in all_events.into_iter().enumerate() {
            if !cancelled_indices.contains(&index) {
                if matches!(
                    event,
                    ServerEvent::BlockBreak { .. } | ServerEvent::BlockPlace { .. }
                ) {
                    crate::server::game::blocks::commit_block_event_with(
                        ecs_world,
                        &event,
                        Some(self),
                    );
                } else {
                    Self::execute_default_behavior(event, ecs_world);
                }
            } else {
                crate::server::game::reject_block_event(ecs_world, &event);
            }
        }
    }

    /// Dispatch a single ServerEvent to a plugin's typed export.
    /// Returns `true` if the event was cancelled by the plugin.
    fn dispatch_event(
        plugin: &mut LoadedPlugin,
        event: &ServerEvent,
        world: &mut bevy_ecs::world::World,
        readiness: &readiness::PluginReadiness,
    ) -> bool {
        // Committed notifications may outlive an actor removed earlier in this cycle.
        // Gameplay proposals still take the fallible, fail-closed dispatch below.
        let committed_actor = match event {
            ServerEvent::PlayerJoin { entity, .. }
            | ServerEvent::PlayerQuit { entity }
            | ServerEvent::PlayerMove { entity, .. }
            | ServerEvent::PlayerJump { entity }
            | ServerEvent::PlayerToggleSneak { entity, .. }
            | ServerEvent::PlayerToggleSprint { entity, .. }
            | ServerEvent::PlayerHeldSlotChange { entity, .. }
            | ServerEvent::PlayerStartBreak { entity, .. }
            | ServerEvent::PlayerItemUse { entity }
            | ServerEvent::PlayerSwing { entity } => Some(*entity),
            _ => None,
        };
        if committed_actor.is_some_and(|entity| {
            world
                .get::<crate::entity::components::Player>(entity)
                .is_none()
        }) {
            return false;
        }
        struct Invocation<'a> {
            store: &'a mut Store<HostContext>,
            bindings: &'a UnastarPlugin,
        }
        let result = with_guest_world(&mut plugin.store, world, |store| {
            prepare_invocation(store);
            let mut plugin = Invocation {
                store,
                bindings: &plugin.bindings,
            };
            match event {
                ServerEvent::Tick { tick_id } => plugin
                    .bindings
                    .call_on_tick(&mut plugin.store, *tick_id)
                    .map(|_| false),
                ServerEvent::PlayerJoin { .. }
                | ServerEvent::PlayerQuit { .. }
                | ServerEvent::PlayerMove { .. } => Ok(false),
                ServerEvent::PlayerChat {
                    entity, message, ..
                } => {
                    let handle = make_player_resource(plugin.store, *entity)?;
                    let tx = make_world_tx(plugin.store)?;
                    plugin
                        .bindings
                        .call_on_player_chat(&mut plugin.store, tx, handle, message)
                        .map(|allow| !allow)
                }
                ServerEvent::BlockBreak {
                    entity,
                    position,
                    block_id,
                    ..
                } => {
                    let handle = make_player_resource(plugin.store, *entity)?;
                    let tx = make_world_tx(plugin.store)?;
                    let pos = wit_types::BlockPos {
                        x: position.0,
                        y: position.1,
                        z: position.2,
                    };
                    plugin
                        .bindings
                        .call_on_block_break(&mut plugin.store, tx, handle, pos, *block_id)
                        .map(|allow| !allow)
                }
                ServerEvent::BlockPlace {
                    entity,
                    position,
                    block_id,
                    ..
                } => {
                    let handle = make_player_resource(plugin.store, *entity)?;
                    let tx = make_world_tx(plugin.store)?;
                    let pos = wit_types::BlockPos {
                        x: position.0,
                        y: position.1,
                        z: position.2,
                    };
                    plugin
                        .bindings
                        .call_on_block_place(&mut plugin.store, tx, handle, pos, *block_id)
                        .map(|allow| !allow)
                }
                ServerEvent::PlayerJump { entity } => {
                    let handle = make_player_resource(plugin.store, *entity)?;
                    let tx = make_world_tx(plugin.store)?;
                    plugin
                        .bindings
                        .call_on_player_jump(&mut plugin.store, tx, handle)
                        .map(|_| false)
                }
                ServerEvent::PlayerToggleSneak {
                    entity,
                    is_sneaking,
                } => {
                    let handle = make_player_resource(plugin.store, *entity)?;
                    let tx = make_world_tx(plugin.store)?;
                    plugin
                        .bindings
                        .call_on_player_toggle_sneak(&mut plugin.store, tx, handle, *is_sneaking)
                        .map(|_| false)
                }
                ServerEvent::PlayerToggleSprint {
                    entity,
                    is_sprinting,
                } => {
                    let handle = make_player_resource(plugin.store, *entity)?;
                    let tx = make_world_tx(plugin.store)?;
                    plugin
                        .bindings
                        .call_on_player_toggle_sprint(&mut plugin.store, tx, handle, *is_sprinting)
                        .map(|_| false)
                }
                ServerEvent::PlayerHeldSlotChange {
                    entity,
                    old_slot,
                    new_slot,
                } => {
                    let handle = make_player_resource(plugin.store, *entity)?;
                    let tx = make_world_tx(plugin.store)?;
                    plugin
                        .bindings
                        .call_on_player_held_slot_change(
                            &mut plugin.store,
                            tx,
                            handle,
                            *old_slot,
                            *new_slot,
                        )
                        .map(|_| false)
                }
                ServerEvent::PlayerStartBreak {
                    entity,
                    position,
                    face,
                } => {
                    let handle = make_player_resource(plugin.store, *entity)?;
                    let tx = make_world_tx(plugin.store)?;
                    let pos = wit_types::BlockPos {
                        x: position.0,
                        y: position.1,
                        z: position.2,
                    };
                    plugin
                        .bindings
                        .call_on_player_start_break(&mut plugin.store, tx, handle, pos, *face)
                        .map(|_| false)
                }
                ServerEvent::PlayerInteractBlock {
                    entity,
                    position,
                    face,
                } => {
                    let handle = make_player_resource(plugin.store, *entity)?;
                    let tx = make_world_tx(plugin.store)?;
                    let pos = wit_types::BlockPos {
                        x: position.0,
                        y: position.1,
                        z: position.2,
                    };
                    plugin
                        .bindings
                        .call_on_player_interact_block(&mut plugin.store, tx, handle, pos, *face)
                        .map(|allow| !allow)
                }
                ServerEvent::PlayerItemUse { entity } => {
                    let handle = make_player_resource(plugin.store, *entity)?;
                    let tx = make_world_tx(plugin.store)?;
                    plugin
                        .bindings
                        .call_on_player_item_use(&mut plugin.store, tx, handle)
                        .map(|_| false)
                }
                ServerEvent::PlayerSwing { entity } => {
                    let handle = make_player_resource(plugin.store, *entity)?;
                    let tx = make_world_tx(plugin.store)?;
                    plugin
                        .bindings
                        .call_on_player_swing(&mut plugin.store, tx, handle)
                        .map(|_| false)
                }
                ServerEvent::Timer { id } => {
                    let tx = make_world_tx(plugin.store)?;
                    plugin
                        .bindings
                        .call_on_timer(&mut plugin.store, tx, *id)
                        .map(|_| false)
                }
            }
        });

        if matches!(
            event,
            ServerEvent::BlockBreak { .. }
                | ServerEvent::BlockPlace { .. }
                | ServerEvent::PlayerInteractBlock { .. }
        ) {
            plugin.store.data_mut().mechanics_accepted =
                result.as_ref().is_ok_and(|cancelled| !cancelled);
        }
        finish_invocation(plugin.store.data_mut(), world, result.is_ok());
        match result {
            Ok(cancelled) => cancelled,
            Err(e) => {
                plugin.store.data_mut().pending_actions.clear();
                error!(plugin = %plugin.manifest.id, error = %e, "Event dispatch failed");
                crate::server::game::item_actions::clear_blocks(world);
                if readiness.fail(&plugin.manifest.id.0, &wasm_failure_reason(&e)) {
                    readiness::fence_world(world);
                }
                matches!(
                    event,
                    ServerEvent::PlayerChat { .. }
                        | ServerEvent::BlockPlace { .. }
                        | ServerEvent::BlockBreak { .. }
                        | ServerEvent::PlayerInteractBlock { .. }
                )
            }
        }
    }

    /// Execute the default server behavior for an event (e.g., broadcasting chat).
    fn execute_default_behavior(event: ServerEvent, world: &mut bevy_ecs::world::World) {
        if matches!(
            event,
            ServerEvent::BlockPlace { .. } | ServerEvent::BlockBreak { .. }
        ) {
            crate::server::game::commit_block_event(world, &event);
            return;
        }
        if let ServerEvent::PlayerChat {
            entity,
            player_id: _,
            message,
        } = event
        {
            use crate::entity::components::PlayerSession;
            use jolyne::valentine::{
                McpePacket, TextPacket, TextPacketBody, TextPacketPayloadAuthorAndMessage,
                TextPacketPayloadAuthorAndMessageMessageType,
            };

            let Some(chat) = crate::server::game::player_events::chat(world, entity, message)
            else {
                return;
            };
            if chat.handled().is_some() {
                crate::server::game::player_events::chatted(world, chat);
                return;
            }
            let sender_name = &chat.display_name;
            let message = &chat.message;

            let packet = TextPacket {
                localize: false,
                body: TextPacketBody::AuthorAndMessage(TextPacketPayloadAuthorAndMessage {
                    message_type: TextPacketPayloadAuthorAndMessageMessageType::Chat,
                    player_name: sender_name.as_str().into(),
                    message: message.as_str().into(),
                }),
                senders_xuid: "0".into(),
                platform_id: "".into(),
                filtered_message: None,
            };

            let Some(session_map) = world.get_resource::<crate::server::game::SessionEntityMap>()
            else {
                return;
            };

            let entities: Vec<Entity> = session_map.iter().map(|(_, e)| e).collect();

            for other_entity in entities {
                if let Some(other_session) = world.get::<PlayerSession>(other_entity) {
                    let _ = other_session.send(McpePacket::from(packet.clone()));
                }
            }
            crate::server::game::player_events::chatted(world, chat);
        }
    }
}

/// Create a new player resource handle for passing to WIT exports.
fn make_player_resource(
    store: &mut Store<HostContext>,
    entity: Entity,
) -> anyhow::Result<Resource<wit_types::Player>> {
    store
        .data_mut()
        .borrow_player(entity)
        .map_err(|error| anyhow::anyhow!("{error:?}"))
}

/// Supply the current callback's world scope without exposing a live foreign world.
fn make_world_tx(store: &mut Store<HostContext>) -> anyhow::Result<Resource<wit_types::WorldTx>> {
    store
        .data_mut()
        .borrow_transaction()
        .map_err(|error| anyhow::anyhow!("{error:?}"))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::plugin::manifest::PluginLimits;
    use wit_bindings::unastar::plugin::host::Host;
    #[path = "damage.rs"]
    mod damage_tests;
    #[path = "deaths.rs"]
    mod death_tests;
    mod fixture;
    #[path = "mechanics.rs"]
    mod mechanics_tests;
    #[path = "movements.rs"]
    mod movement_tests;
    #[path = "sessions.rs"]
    mod session_tests;

    pub(super) fn engine() -> Engine {
        let mut config = wasmtime::Config::new();
        config.consume_fuel(true).epoch_interruption(true);
        Engine::new(&config).unwrap()
    }

    fn store(engine: &Engine, limits: &PluginLimits) -> Store<HostContext> {
        create_store(
            engine,
            WasiCtx::builder().build(),
            PluginId("test".into()),
            limits,
        )
        .unwrap()
    }

    /// Real component exports, with canonical ABI lifting and host imports.
    /// Keeping the fixture as WAT avoids needing a wasm32 target in host tests.
    fn component(engine: &Engine, allow: bool) -> Component {
        component_with_lifecycle(
            engine,
            allow,
            if allow {
                "i32.const 32 i32.const 1 i32.store i32.const 36 local.get 4 f32.store i32.const 32"
            } else {
                "i32.const 32 i32.const 0 i32.store i32.const 32"
            },
            if allow {
                "i32.const 64 i32.const 1 i32.store i32.const 72 local.get 1 f64.store i32.const 80 local.get 2 f64.store i32.const 88 local.get 3 f64.store i32.const 64"
            } else {
                "i32.const 64 i32.const 0 i32.store i32.const 64"
            },
        )
    }

    /// Build a real component with independently controlled lifecycle decisions.
    pub(super) fn component_with_lifecycle(
        engine: &Engine,
        allow: bool,
        damage_body: &str,
        respawn_body: &str,
    ) -> Component {
        component_with_tick(engine, allow, damage_body, respawn_body, "")
    }

    /// Build a real component whose buffered tick may call hosts and trap.
    pub(super) fn component_with_tick(
        engine: &Engine,
        allow: bool,
        damage_body: &str,
        respawn_body: &str,
        tick_body: &str,
    ) -> Component {
        Component::new(
            engine,
            component_source(
                allow,
                damage_body,
                respawn_body,
                tick_body,
                "call $tick i64.const 42 i64.ne if unreachable end",
            ),
        )
        .unwrap()
    }

    /// Executable fixture supports actual initialization from a plugin directory.
    pub(crate) fn component_source(
        allow: bool,
        damage_body: &str,
        respawn_body: &str,
        tick_body: &str,
        load_body: &str,
    ) -> Vec<u8> {
        // Existing controlled lifecycle bodies gain the transaction argument.
        fn scoped(body: &str) -> String {
            let mut result = body.to_owned();
            for index in (0..=5).rev() {
                result = result.replace(
                    &format!("local.get {index}"),
                    &format!("local.get __{}", index + 1),
                );
            }
            result
                .replace("local.get __", "local.get ")
                .replace("global.get $damage-calls", "i32.const 400 i32.load")
        }
        let allow = format!("i32.const {}", u8::from(allow));
        fixture::bytes(&[
            (
                "on-load",
                load_body.replace("call $tick", "call @get-tick-id@"),
            ),
            (
                "on-tick",
                tick_body.replace("call $tick", "call @get-tick-id@"),
            ),
            ("on-player-chat", allow.clone()),
            ("on-block-break", allow.clone()),
            ("on-block-place", allow.clone()),
            ("on-player-interact-block", allow),
            (
                "on-player-damage",
                format!(
                    "i32.const 400 i32.const 400 i32.load i32.const 1 i32.add i32.store {}",
                    fixture::legacy_damage_body(damage_body)
                ),
            ),
            ("on-player-respawn", scoped(respawn_body)),
        ])
    }

    #[path = "blocks.rs"]
    mod block_tests;
    #[path = "commands.rs"]
    mod command_tests;
    #[path = "execution.rs"]
    mod execution_tests;
    #[path = "item_actions.rs"]
    mod item_action_tests;
    #[path = "readiness.rs"]
    mod readiness_tests;
    #[path = "storage.rs"]
    mod storage_tests;
    #[path = "ui.rs"]
    mod ui_tests;
    #[path = "views.rs"]
    mod views_tests;
    #[path = "wasi_stdio.rs"]
    mod wasi_stdio_tests;

    #[test]
    fn real_plugin_load_reads_world_and_chat_cancellation_prevents_delivery() {
        for allow in [false, true] {
            let engine = engine();
            let manifest = PluginManifest {
                id: PluginId("test".into()),
                name: "test".into(),
                version: "1".into(),
                api_version: PLUGIN_API_VERSION.into(),
                required: false,
                config: None,
                capabilities: Default::default(),
                subscriptions: [crate::ecs::events::EventKind::PlayerChat].into(),
                limits: PluginLimits::default(),
            };
            let pending = PendingPlugin {
                component: component(&engine, allow),
                store: store(&engine, &manifest.limits),
                manifest,
            };
            let mut manager = PluginManager {
                engine,
                plugins: HashMap::new(),
                pending: vec![pending],
                tick_prepared: false,
                tick_budget_valid: true,
                readiness: Default::default(),
                storage_registry: Default::default(),
                command_roots: Default::default(),
                ui_registry: Default::default(),
                view_registry: Default::default(),
            };
            let mut world = bevy_ecs::world::World::new();
            world.insert_resource(crate::world::ChunkManager::new(
                crate::world::WorldConfig::void_spawn_platform(1),
            ));
            world.insert_resource(crate::ecs::resources::TickCounter { current: 42 });
            world.insert_resource(crate::ecs::events::ActionQueue::default());
            let (tx, mut rx) = tokio::sync::mpsc::channel(8);
            let entity = world
                .spawn((
                    crate::entity::components::Player,
                    crate::entity::components::PlayerUuid(uuid::Uuid::from_u128(1)),
                    crate::entity::components::RuntimeEntityId(1),
                    crate::entity::components::PlayerName("Alice".into()),
                    crate::entity::components::PlayerSession::new(
                        1,
                        "Alice".into(),
                        None,
                        None,
                        tx,
                    ),
                ))
                .id();
            let mut sessions = crate::server::game::SessionEntityMap::default();
            sessions.insert(1, entity);
            world.insert_resource(sessions);
            let mut events = crate::ecs::events::EventBuffer::default();
            events.push(ServerEvent::PlayerChat {
                entity,
                player_id: "test".into(),
                message: "hello".into(),
            });
            world.insert_resource(events);
            manager.tick(&mut world);
            assert_eq!(manager.plugins.len(), 1, "on-load host query must succeed");
            assert!(
                manager
                    .plugins
                    .values()
                    .all(|plugin| plugin.store.data().world_ptr.is_null())
            );
            if allow {
                assert!(matches!(
                    rx.try_recv().unwrap().data,
                    jolyne::valentine::McpePacketData::TextPacket(_)
                ));
            }
            assert!(
                rx.try_recv().is_err(),
                "cancelled chat must not deliver; allowed chat delivers once"
            );
            manager.tick(&mut world);
            assert!(rx.try_recv().is_err(), "drained chat must not be replayed");
            let plugin = manager.plugins.values_mut().next().unwrap();
            for event in [
                ServerEvent::BlockPlace {
                    click: None,
                    entity,
                    player_id: "test".into(),
                    position: (0, 1, 0),
                    block_id: 1,
                },
                ServerEvent::BlockBreak {
                    entity,
                    player_id: "test".into(),
                    position: (0, 1, 0),
                    block_id: 1,
                },
            ] {
                assert_eq!(
                    PluginManager::dispatch_event(plugin, &event, &mut world, &Default::default()),
                    !allow
                );
                assert!(plugin.store.data().world_ptr.is_null());
            }
        }
    }

    #[test]
    fn cancelled_place_leaves_world_and_owned_stack_unchanged() {
        use crate::ecs::events::EventKind;
        use crate::entity::components::{GameMode, HeldSlot, MainInventory, Position};
        use crate::server::game::types::{BlockRegistryResource, ItemRegistryResource};
        use crate::world::chunk::{Chunk, blocks};
        use crate::world::ecs::{BlockBroadcastEvent, ChunkData, ChunkManager, ChunkPosition};
        use std::sync::Arc;
        for (allow, subscription) in [
            (false, EventKind::BlockPlace),
            (true, EventKind::BlockPlace),
            (false, EventKind::PlayerInteractBlock),
        ] {
            let engine = engine();
            let manifest = PluginManifest {
                id: PluginId("test".into()),
                name: "test".into(),
                version: "1".into(),
                api_version: PLUGIN_API_VERSION.into(),
                required: false,
                config: None,
                capabilities: Default::default(),
                subscriptions: [subscription].into(),
                limits: PluginLimits::default(),
            };
            let pending = PendingPlugin {
                component: component(&engine, allow),
                store: store(&engine, &manifest.limits),
                manifest,
            };
            let mut manager = PluginManager {
                engine,
                plugins: HashMap::new(),
                pending: vec![pending],
                tick_prepared: false,
                tick_budget_valid: true,
                readiness: Default::default(),
                storage_registry: Default::default(),
                command_roots: Default::default(),
                ui_registry: Default::default(),
                view_registry: Default::default(),
            };
            let mut world = bevy_ecs::world::World::new();
            world.insert_resource(crate::ecs::resources::TickCounter { current: 42 });
            world.insert_resource(crate::ecs::events::ActionQueue::default());
            world.insert_resource(crate::server::game::types::ServerConfigResource(
                crate::server::game::ServerConfig::default(),
            ));
            world.insert_resource(bevy_ecs::message::Messages::<BlockBroadcastEvent>::default());
            world.insert_resource(ChunkManager::new(
                crate::world::WorldConfig::void_spawn_platform(1),
            ));
            let chunk = world.spawn(ChunkData::new(Chunk::new(0, 0))).id();
            world
                .resource_mut::<ChunkManager>()
                .insert(ChunkPosition::new(0, 0), chunk);
            let mut items = crate::registry::ItemRegistry::new();
            items.load_vanilla();
            world.insert_resource(ItemRegistryResource(Arc::new(items)));
            world.insert_resource(BlockRegistryResource(Arc::new(
                crate::registry::BlockRegistry::vanilla().clone(),
            )));
            let mut inventory = MainInventory::default();
            inventory
                .0
                .set_item(0, crate::item::test_support::stack("minecraft:stone", 2))
                .unwrap();
            let (tx, _rx) = tokio::sync::mpsc::channel(8);
            let entity = world
                .spawn((
                    crate::entity::components::Player,
                    crate::entity::components::PlayerName("Builder".into()),
                    crate::entity::components::PlayerUuid(uuid::Uuid::from_u128(1)),
                    crate::entity::components::RuntimeEntityId(1),
                    crate::entity::components::PlayerSession::new(
                        1,
                        "Builder".into(),
                        None,
                        None,
                        tx,
                    ),
                    inventory,
                    HeldSlot::default(),
                    GameMode::Survival,
                    Position(glam::DVec3::new(1.0, 64.0, 1.0)),
                ))
                .id();
            let mut events = crate::ecs::events::EventBuffer::default();
            events.push(ServerEvent::PlayerInteractBlock {
                entity,
                position: (1, 64, 1),
                face: 1,
            });
            events.push(ServerEvent::BlockPlace {
                click: None,
                entity,
                player_id: "test".into(),
                position: (1, 65, 1),
                block_id: *blocks::STONE,
            });
            world.insert_resource(events);
            manager.tick(&mut world);
            assert_eq!(manager.plugins.len(), 1);
            assert_eq!(
                world
                    .get::<ChunkData>(chunk)
                    .unwrap()
                    .inner
                    .get_block(1, 65, 1),
                if allow { *blocks::STONE } else { *blocks::AIR }
            );
            assert_eq!(
                world
                    .get::<MainInventory>(entity)
                    .unwrap()
                    .0
                    .item(0)
                    .unwrap()
                    .count,
                if allow { 1 } else { 2 }
            );
        }
    }

    #[test]
    fn chat_without_plugins_delivers_once() {
        let mut manager = PluginManager {
            engine: engine(),
            plugins: HashMap::new(),
            pending: Vec::new(),
            tick_prepared: false,
            tick_budget_valid: true,
            readiness: Default::default(),
            storage_registry: Default::default(),
            command_roots: Default::default(),
            ui_registry: Default::default(),
            view_registry: Default::default(),
        };
        let mut world = bevy_ecs::world::World::new();
        world.insert_resource(crate::ecs::resources::TickCounter::default());
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let entity = world
            .spawn((
                crate::entity::components::PlayerName("Alice".into()),
                crate::entity::components::PlayerSession::new(1, "Alice".into(), None, None, tx),
            ))
            .id();
        let mut sessions = crate::server::game::SessionEntityMap::default();
        sessions.insert(1, entity);
        world.insert_resource(sessions);
        let mut events = crate::ecs::events::EventBuffer::default();
        events.push(ServerEvent::PlayerChat {
            entity,
            player_id: "test".into(),
            message: "hello".into(),
        });
        world.insert_resource(events);
        manager.tick(&mut world);
        assert!(matches!(
            rx.try_recv().unwrap().data,
            jolyne::valentine::McpePacketData::TextPacket(_)
        ));
        assert!(rx.try_recv().is_err());
        manager.tick(&mut world);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn loading_wasm_host_calls_observe_world_and_clear_attachment() {
        let engine = engine();
        let mut store = store(&engine, &PluginLimits::default());
        let mut world = bevy_ecs::world::World::new();
        world.insert_resource(crate::ecs::resources::TickCounter { current: 42 });
        let mut linker = wasmtime::Linker::new(&engine);
        linker
            .func_wrap(
                "host",
                "tick",
                |mut caller: wasmtime::Caller<'_, HostContext>| {
                    caller.data_mut().get_tick_id() as i64
                },
            )
            .unwrap();
        let module = wasmtime::Module::new(
            &engine,
            r#"
            (module
                (import "host" "tick" (func $tick (result i64)))
                (func $load
                    call $tick
                    i64.const 42
                    i64.ne
                    if unreachable end)
                (start $load))
        "#,
        )
        .unwrap();
        with_world(&mut store, &mut world, |store| {
            linker.instantiate(store, &module).unwrap();
        });
        assert!(store.data().world_ptr.is_null());
    }

    #[test]
    fn world_attachment_is_cleared_on_unwind() {
        let engine = engine();
        let mut store = store(&engine, &PluginLimits::default());
        let mut world = bevy_ecs::world::World::new();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            with_world(&mut store, &mut world, |_| panic!("host panic"));
        }));
        assert!(result.is_err());
        assert!(store.data().world_ptr.is_null());
    }

    #[test]
    fn wasm_memory_growth_traps_at_manifest_limit() {
        let engine = engine();
        let mut store = store(
            &engine,
            &PluginLimits {
                max_memory: 65536,
                ..PluginLimits::default()
            },
        );
        let module = wasmtime::Module::new(
            &engine,
            r#"
            (module (memory 1)
                (func (export "grow") i32.const 1 memory.grow drop))
        "#,
        )
        .unwrap();
        let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
        let grow = instance
            .get_typed_func::<(), ()>(&mut store, "grow")
            .unwrap();
        assert!(grow.call(&mut store, ()).is_err());
    }

    #[test]
    fn wasm_initial_memory_cannot_exceed_manifest_limit() {
        let engine = engine();
        let mut store = store(
            &engine,
            &PluginLimits {
                max_memory: 65536,
                ..PluginLimits::default()
            },
        );
        let module = wasmtime::Module::new(&engine, "(module (memory 2))").unwrap();
        assert!(wasmtime::Instance::new(&mut store, &module, &[]).is_err());
    }

    #[test]
    fn wasm_invocations_share_tick_fuel_and_next_tick_replenishes_it() {
        let engine = engine();
        let limits = PluginLimits {
            fuel_per_tick: 100,
            ..PluginLimits::default()
        };
        let mut store = store(&engine, &limits);
        let module = wasmtime::Module::new(
            &engine,
            r#"
            (module
                (func (export "spin") (loop br 0))
                (func (export "ok") (result i32) i32.const 7))
        "#,
        )
        .unwrap();
        let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
        let spin = instance
            .get_typed_func::<(), ()>(&mut store, "spin")
            .unwrap();
        let error = spin.call(&mut store, ()).unwrap_err();
        assert!(matches!(
            error.downcast_ref::<wasmtime::Trap>(),
            Some(wasmtime::Trap::OutOfFuel)
        ));
        prepare_invocation(&mut store);
        let ok = instance
            .get_typed_func::<(), i32>(&mut store, "ok")
            .unwrap();
        let second_error = ok.call(&mut store, ()).unwrap_err();
        assert!(matches!(
            second_error.downcast_ref::<wasmtime::Trap>(),
            Some(wasmtime::Trap::OutOfFuel)
        ));
        begin_fuel_budget(&mut store, &limits).unwrap();
        prepare_invocation(&mut store);
        assert_eq!(ok.call(&mut store, ()).unwrap(), 7);
    }

    mod router_ticks;
}

#[cfg(test)]
mod native_chat_acceptance_tests {
    use super::PluginManager;
    use crate::ecs::events::ServerEvent;
    use crate::entity::components::PlayerSession;
    use crate::server::game::player_events::{
        PlayerChat, PlayerChatted, PlayerEventHandler, PlayerEventHooks,
    };
    use bevy_ecs::prelude::*;
    use jolyne::valentine::{McpePacket, McpePacketData};
    use tokio::sync::mpsc;

    /// Accepted messages recorded only at the committed native boundary.
    #[derive(Resource, Default)]
    struct Accepted(Vec<(String, Option<String>)>);

    /// A generic custom route that publishes its effect only after acceptance.
    struct RouteChat(bool);
    impl PlayerEventHandler for RouteChat {
        fn on_chat(&self, _world: &mut World, event: &mut PlayerChat) {
            if self.0 {
                event.handle("custom.route".into());
            }
        }
        fn on_chatted(&self, world: &mut World, event: &PlayerChatted) {
            world
                .resource_mut::<Accepted>()
                .0
                .push((event.message.clone(), event.handled.clone()));
            if event.handled.is_some() {
                world
                    .get::<PlayerSession>(event.player())
                    .unwrap()
                    .send(crate::server::game::types::system_text("Custom route accepted").into());
            }
        }
    }

    /// Connected source and viewer with the same final broadcast path as runtime.
    fn world(
        route: bool,
    ) -> (
        World,
        Entity,
        mpsc::Receiver<McpePacket>,
        mpsc::Receiver<McpePacket>,
    ) {
        let mut world = World::new();
        world.init_resource::<Accepted>();
        world.init_resource::<PlayerEventHooks>();
        world
            .resource_mut::<PlayerEventHooks>()
            .register(RouteChat(route));
        world.init_resource::<crate::server::game::SessionEntityMap>();
        let (source_tx, source_rx) = mpsc::channel(64);
        let (viewer_tx, viewer_rx) = mpsc::channel(64);
        let source = world
            .spawn(PlayerSession::new(
                1,
                "Source".into(),
                Some("123".into()),
                None,
                source_tx,
            ))
            .id();
        let viewer = world
            .spawn(PlayerSession::new(
                2,
                "Viewer".into(),
                Some("456".into()),
                None,
                viewer_tx,
            ))
            .id();
        world
            .resource_mut::<crate::server::game::SessionEntityMap>()
            .insert(1, source);
        world
            .resource_mut::<crate::server::game::SessionEntityMap>()
            .insert(2, viewer);
        (world, source, source_rx, viewer_rx)
    }

    /// Dispatch the final plugin-accepted chat behavior without opening sockets.
    fn deliver(world: &mut World, source: Entity) {
        PluginManager::execute_default_behavior(
            ServerEvent::PlayerChat {
                entity: source,
                player_id: "123".into(),
                message: "Hello".into(),
            },
            world,
        );
    }

    #[test]
    fn accepted_handled_chat_routes_once_without_default_broadcast() {
        let (mut world, source, mut source_rx, mut viewer_rx) = world(true);
        deliver(&mut world, source);
        assert_eq!(
            world.resource::<Accepted>().0,
            vec![("Hello".into(), Some("custom.route".into()))]
        );
        assert!(matches!(
            source_rx.try_recv().unwrap().data,
            McpePacketData::TextPacket(_)
        ));
        assert!(source_rx.try_recv().is_err());
        assert!(viewer_rx.try_recv().is_err());
    }

    #[test]
    fn ordinary_chat_broadcasts_then_notifies_acceptance_once() {
        let (mut world, source, mut source_rx, mut viewer_rx) = world(false);
        deliver(&mut world, source);
        assert_eq!(world.resource::<Accepted>().0, vec![("Hello".into(), None)]);
        assert!(matches!(
            source_rx.try_recv().unwrap().data,
            McpePacketData::TextPacket(_)
        ));
        assert!(matches!(
            viewer_rx.try_recv().unwrap().data,
            McpePacketData::TextPacket(_)
        ));
        assert!(source_rx.try_recv().is_err());
        assert!(viewer_rx.try_recv().is_err());
    }

    #[test]
    fn late_observer_veto_suppresses_custom_and_ordinary_acceptance_and_delivery() {
        for route in [false, true] {
            let (mut world, source, mut source_rx, mut viewer_rx) = world(route);
            world.add_observer(|mut event: On<PlayerChat>| event.cancel());
            deliver(&mut world, source);
            assert!(world.resource::<Accepted>().0.is_empty());
            assert!(source_rx.try_recv().is_err());
            assert!(viewer_rx.try_recv().is_err());
        }
    }

    #[test]
    fn invalid_or_conflicting_handled_actions_never_publish_acceptance() {
        for (route, action) in [
            (false, "".to_owned()),
            (false, "x".repeat(129)),
            (false, "line\nbreak".to_owned()),
            (true, "other.route".to_owned()),
        ] {
            let (mut world, source, mut source_rx, mut viewer_rx) = world(route);
            world.add_observer(move |mut event: On<PlayerChat>| event.handle(action.clone()));
            deliver(&mut world, source);
            assert!(world.resource::<Accepted>().0.is_empty());
            assert!(source_rx.try_recv().is_err());
            assert!(viewer_rx.try_recv().is_err());
        }
    }
}

impl Drop for HostContext {
    /// Dropping or replacing a component cancels jobs still owned by its instance.
    fn drop(&mut self) {
        tasks::cancel_pending(self);
    }
}
