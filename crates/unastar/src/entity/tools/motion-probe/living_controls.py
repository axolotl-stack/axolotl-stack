"""Authored living-mob controls; this module never starts a runtime.

run.py --probe living-mob --wire-capture uses the existing bds-extractor
recorder and bds_runtime.Bds lifecycle. Run only in a coordinated native window:

  <runtime>/venv/Scripts/python.exe \
    crates/unastar/src/entity/tools/motion-probe/run.py \
    --harness tools/bds-sweep --client target/debug/bds-extractor.exe \
    --probe living-mob --wire-capture --output <new-capture-directory>

Prerequisites: the pinned BDS/Endstone runtime, this probe installed from a
prebuilt wheel in its venv, the existing client-diff Endstone packet recorder,
and an already-built bds-extractor. No install-time backend, build or network
access belongs inside a native lease. Restore the original package state after
the session. No real client is needed. The authored pack depends
on stable @minecraft/server 2.0.0; missing pack/script support fails explicitly
at the release acknowledgment, never falls back to guessed motion.

living-controls.json describes every case, requested pose/velocity, component
provenance (source SHA-256 and exact copied components), isolation, and timing.
behavior_pack/ retains the exact authored pack. samples.jsonl retains Endstone
per-tick position/velocity/ground/fluid observations, actual runtime IDs, and
Script API release-state readback. wire/packets.jsonl and endstone-wire/ retain
actual unfiltered packets, including AddActor/SetActorData when BDS sends them;
there are no synthetic packet fixtures or claims that either packet must occur.
Release state is captured synchronously by Script API, whereas subsequent rows
are Endstone scheduler observations: their callback phase relative to the first
native integration is not assumed. Use release_state.script_tick and probe
observation ticks as distinct clocks; do not fit the first delta as one tick.

Controls retain only the source components named below, with native cow/chicken
runtime_identifier. No behavior goals, navigation, spawn/baby events, despawn,
breeding, egg timers, equipment, effects, movement-attribute overrides, or periodic
position/velocity writes are used. Native hard-coded behavior (including chicken
descent) remains deliberately enabled. These are NOT complete vanilla entity
definitions: gameplay AI, interactions, lifecycle and metadata may differ from
real vanilla actors. A chicken-component/cow-runtime counterfactual isolates the
native runtime choice. The authored uniform-air-drag cow uses the component's
published minimum format; its format differs from the unmodified cow control,
so motion differences cannot be attributed to the marker alone. It observes
extended flags, not a vanilla gameplay or single-factor physics golden.
A zero-component cow-runtime counterfactual separately observes native state
without authored component values or properties; it is not a vanilla definition
or a constructor-default claim before capture.

The cow, uniform-air-drag cow and chicken-component/cow-runtime definitions carry the same six
authored int/enum/float/bool properties in opposite JSON member orders, with
server-only fields interleaved. These are diagnostic values, not vanilla
defaults. Release readback records their initial values; one Script API update
after 40 ticks checks the actual changed values two ticks later. Retained native
SyncActorProperty/AddActor/SetActorData packets determine wire indices and sync
membership; no source-order or integer/float slot policy is assumed.

There are 25 parallel lanes, 200 released observation intervals each, 40 terrain
warmup ticks, 20 actor warmup ticks, at most 100 release-ack ticks and 20 packet
drain ticks: <=381 scheduled probe callbacks plus the existing 60-tick join
delay. At 20 TPS this is <=22.05 seconds; this is a tick bound, not a guarantee
against server stalls. The runner uses a 40-second wire settle window and a
240-second probe wall deadline after BDS readiness. The shared lifecycle retains
its separate startup (240s) and graceful shutdown (60s) bounds. Capture settle
may be raised to at most 120s, never shortened below 40s for this mode.

Script API contracts:
https://learn.microsoft.com/en-us/minecraft/creator/scriptapi/minecraft/server/entity
https://learn.microsoft.com/en-us/minecraft/creator/scriptapi/minecraft/server/systemafterevents
"""

import hashlib
import json
import uuid
from pathlib import Path


SOURCE_COMPONENTS = (
    'minecraft:type_family', 'minecraft:collision_box', 'minecraft:nameable',
    'minecraft:health', 'minecraft:movement', 'minecraft:movement.basic',
    'minecraft:jump.static', 'minecraft:can_climb', 'minecraft:physics',
)
CHICKEN_COMPONENTS = ('minecraft:damage_sensor', 'minecraft:uses_legacy_friction')
OBSERVATION_TICKS = 200
ACK_TICKS = 100
WIRE_SECONDS = 40

PROPERTY_CONTROLS = {
    'entity_motion:z_int': {'type': 'int', 'range': [-16, 16], 'default': 7, 'client_sync': True},
    'entity_motion:a_enum': {'type': 'enum', 'values': ['zero', 'one', 'two'],
                             'default': 'two', 'client_sync': True},
    'entity_motion:y_float': {'type': 'float', 'range': [-4.0, 4.0],
                              'default': 1.25, 'client_sync': True},
    'entity_motion:b_server_int': {'type': 'int', 'range': [-16, 16],
                                   'default': 9, 'client_sync': False},
    'entity_motion:x_bool': {'type': 'bool', 'default': True, 'client_sync': True},
    'entity_motion:c_server_float': {'type': 'float', 'range': [-4.0, 4.0],
                                     'default': 2.5, 'client_sync': False},
}
PROPERTY_UPDATES = {
    'entity_motion:z_int': -3, 'entity_motion:a_enum': 'one',
    'entity_motion:y_float': -0.75, 'entity_motion:b_server_int': -9,
    'entity_motion:x_bool': False, 'entity_motion:c_server_float': -2.5,
}

# Only the release operation writes motion. Readback is transported through the
# actor's nameTag and tags, which the pinned Endstone API can read. No additional
# recorder, server lifecycle, socket, or filesystem API is introduced in script.
RELEASE_SCRIPT = r'''import { system, world } from "@minecraft/server";

function fail(actor, error) {
    if (actor) {
        actor.nameTag = String(error);
        actor.addTag("motion_release_error");
    }
    console.error(`entity_motion control failed: ${error}`);
}

system.afterEvents.scriptEventReceive.subscribe(event => {
    if (event.id !== "entity_motion:release") return;
    let actor;
    try {
        const request = JSON.parse(event.message);
        const found = world.getDimension("overworld").getEntities({tags: [request.tag]});
        if (found.length !== 1) throw new Error(`expected one tagged actor, found ${found.length}`);
        actor = found[0];
        actor.teleport({x: request.position[0], y: request.position[1], z: request.position[2]},
                       {rotation: {x: request.pitch, y: request.yaw}});
        actor.clearVelocity();
        actor.applyImpulse({x: request.velocity[0], y: request.velocity[1], z: request.velocity[2]});
        const p = actor.location, v = actor.getVelocity(), r = actor.getRotation();
        const position = [p.x, p.y, p.z], velocity = [v.x, v.y, v.z];
        for (let i = 0; i < 3; ++i) {
            if (Math.abs(position[i] - request.position[i]) > 0.00001 ||
                Math.abs(velocity[i] - request.velocity[i]) > 0.00001)
                throw new Error(`release readback differs: ${JSON.stringify({position, velocity})}`);
        }
        const updates = request.property_updates;
        const properties = Object.fromEntries(Object.keys(updates).map(name => [name, actor.getProperty(name)]));
        actor.nameTag = JSON.stringify({script_tick: system.currentTick,
            entity_id: actor.id, position, velocity, on_ground: actor.isOnGround,
            pitch: r.x, yaw: r.y, properties});
        actor.addTag("motion_released");
        if (Object.keys(updates).length !== 0) {
            system.runTimeout(() => {
                try {
                    for (const [name, value] of Object.entries(updates)) actor.setProperty(name, value);
                    // Property writes take effect on a subsequent native tick.
                    system.runTimeout(() => {
                        try {
                            const actual = Object.fromEntries(Object.keys(updates).map(name => [name, actor.getProperty(name)]));
                            for (const [name, expected] of Object.entries(updates)) {
                                if (actual[name] !== expected)
                                    throw new Error(`property ${name}: expected ${expected}, observed ${actual[name]}`);
                            }
                            console.warn(`entity_motion properties ${JSON.stringify({tag: request.tag, script_tick: system.currentTick, actual})}`);
                            actor.addTag("motion_properties_updated");
                        } catch (error) { fail(actor, error); }
                    }, 2);
                } catch (error) { fail(actor, error); }
            }, 40);
        }
    } catch (error) { fail(actor, error); }
});
'''


def save(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2) + '\n', encoding='utf-8')


def prepare(out: Path):
    """Retain a source-derived diagnostic BP and full reproducibility manifest."""
    repo = Path(__file__).resolve().parents[6]
    source_dir = repo / 'crates/unastar-data/data/vanilla_bp/behavior_pack/entities'
    pack = out / 'behavior_pack'
    pack_id = str(uuid.uuid4())
    save(pack / 'manifest.json', {
        'format_version': 2,
        'header': {'name': 'Living motion controls', 'description': 'Authored AI-free native-runtime controls',
                   'uuid': pack_id, 'version': [1, 0, 0], 'min_engine_version': [1, 26, 0]},
        'modules': [{'type': 'data', 'uuid': str(uuid.uuid4()), 'version': [1, 0, 0]},
                    {'type': 'script', 'language': 'javascript', 'entry': 'scripts/release.js',
                     'uuid': str(uuid.uuid4()), 'version': [1, 0, 0]}],
        'dependencies': [{'module_name': '@minecraft/server', 'version': '2.0.0'}],
    })
    (pack / 'scripts').mkdir()
    (pack / 'scripts/release.js').write_text(RELEASE_SCRIPT, encoding='utf-8')
    sources, definitions = {}, {}
    for species in ('cow', 'chicken'):
        path = source_dir / f'{species}.json'
        raw = path.read_bytes()
        source = json.loads(raw)
        keys = SOURCE_COMPONENTS + (CHICKEN_COMPONENTS if species == 'chicken' else ())
        components = {key: source['minecraft:entity']['components'][key] for key in keys}
        sources[species] = {'path': path.relative_to(repo).as_posix(),
                            'sha256': hashlib.sha256(raw).hexdigest(),
                            'components': components}
        identifier = f'entity_motion:{species}_control'
        definition = {'format_version': source['format_version'], 'minecraft:entity': {
            'description': {'identifier': identifier, 'runtime_identifier': f'minecraft:{species}',
                            'is_spawnable': False, 'is_summonable': True},
            'components': components,
        }}
        if species == 'cow':
            definition['minecraft:entity']['description']['properties'] = PROPERTY_CONTROLS
        save(pack / 'entities' / f'{species}_control.json', definition)
        definitions[identifier] = definition
    uniform = json.loads(json.dumps(definitions['entity_motion:cow_control']))
    uniform['minecraft:entity']['description']['identifier'] = 'entity_motion:cow_uniform_air_drag'
    uniform_schema_path = (
        repo / 'crates/unastar-data/data/vanilla_bp/metadata/json_schemas/server/entity' /
        '1.26.20/UsesUniformAirDragDefinition.json')
    uniform_schema_bytes = uniform_schema_path.read_bytes()
    uniform_schema = json.loads(uniform_schema_bytes)
    required_format = uniform_schema['x-format-version']
    if tuple(map(int, uniform['format_version'].split('.'))) < tuple(map(int, required_format.split('.'))):
        uniform['format_version'] = required_format
    uniform_format_source = {
        'path': uniform_schema_path.relative_to(repo).as_posix(),
        'sha256': hashlib.sha256(uniform_schema_bytes).hexdigest(),
        'required_format': required_format,
        'control_format': uniform['format_version'],
        'baseline_cow_format': definitions['entity_motion:cow_control']['format_version'],
        'confound': 'Marker and required format differ; not a single-factor motion comparison.',
    }
    uniform['minecraft:entity']['components']['minecraft:uses_uniform_air_drag'] = {}
    save(pack / 'entities/cow_uniform_air_drag.json', uniform)
    definitions['entity_motion:cow_uniform_air_drag'] = uniform
    bare = json.loads(json.dumps(definitions['entity_motion:cow_control']))
    bare['minecraft:entity']['description']['identifier'] = 'entity_motion:cow_bare_runtime'
    bare['minecraft:entity']['description'].pop('properties')
    bare['minecraft:entity']['components'] = {}
    save(pack / 'entities/cow_bare_runtime.json', bare)
    definitions['entity_motion:cow_bare_runtime'] = bare
    counterfactual = json.loads(json.dumps(definitions['entity_motion:chicken_control']))
    counterfactual['minecraft:entity']['description'].update(
        identifier='entity_motion:chicken_components_cow_runtime', runtime_identifier='minecraft:cow')
    counterfactual['minecraft:entity']['description']['properties'] = dict(
        reversed(tuple(PROPERTY_CONTROLS.items())))
    save(pack / 'entities/chicken_components_cow_runtime.json', counterfactual)
    definitions['entity_motion:chicken_components_cow_runtime'] = counterfactual
    profiles = [
        ('cow_control', 'entity_motion:cow_control', 'isolated_native_cow'),
        ('chicken_control', 'entity_motion:chicken_control', 'isolated_native_chicken'),
        ('chicken_components_cow_runtime', 'entity_motion:chicken_components_cow_runtime', 'counterfactual'),
        ('cow_uniform_air_drag', 'entity_motion:cow_uniform_air_drag', 'authored_uniform_air_drag'),
        ('cow_bare_runtime', 'entity_motion:cow_bare_runtime', 'zero_component_cow_runtime'),
    ]
    motions = [
        ('ground_stone', 'minecraft:stone', -60.0, [0.3, 0.0, 0.0]),
        ('ground_ice', 'minecraft:ice', -60.0, [0.3, 0.0, 0.0]),
        ('air_rest', 'minecraft:stone', -52.0, [0.0, 0.0, 0.0]),
        ('air_downward', 'minecraft:stone', -52.0, [0.0, -0.4, 0.0]),
        ('air_upward_horizontal', 'minecraft:stone', -52.0, [0.3, 0.4, 0.0]),
    ]
    cases = []
    for row, (profile, identifier, isolation) in enumerate(profiles):
        for column, (motion, floor, y, velocity) in enumerate(motions):
            x, z = -24 + 12 * column, -24 + 12 * row
            case = f'{profile}_{motion}'
            cases.append({'case': case, 'actor_type': identifier, 'isolation': isolation,
                          'tag': f'motion_{row}_{column}', 'position': [x + 0.5, y, z + 0.5],
                          'velocity': velocity, 'yaw': 0.0, 'pitch': 0.0,
                          'property_updates': PROPERTY_UPDATES
                          if 'properties' in definitions[identifier]['minecraft:entity']['description'] else {},
                          'floor': {'min': [x - 5, -61, z - 5], 'max': [x + 5, -61, z + 5],
                                    'block': floor}})
    metadata = {'schema': 1, 'mode': 'living-mob', 'pack_id': pack_id,
                'script_module': '@minecraft/server 2.0.0', 'sources': sources,
                'definitions': definitions, 'cases': cases,
                'authored_components': {'entity_motion:cow_uniform_air_drag': {'minecraft:uses_uniform_air_drag': {}}},
                'uniform_format_source': uniform_format_source,
                'observation_ticks': OBSERVATION_TICKS, 'release_ack_ticks': ACK_TICKS,
                'wire_seconds': WIRE_SECONDS,
                'isolation': 'Source-only minimal components; no goals/navigation/events/effects; native runtime behavior retained. Not complete vanilla actors or vanilla metadata goldens.',
                'sampling_phase': 'Release readback is synchronous Script API; subsequent rows are Endstone callbacks, not assumed to share the script tick phase.',
                'terrain': {'cleared_air_min': [-32, -60, -32], 'cleared_air_max': [32, -44, 32],
                            'base_floor_min': [-32, -61, -32], 'base_floor_max': [32, -61, 32],
                            'base_floor_block': 'minecraft:stone'}}
    save(out / 'living-controls.json', metadata)
    return metadata
