# Entity backlog (moved)

This backlog now lives in the vanilla-parity catalog:

- [`docs/parity/entities.toml`](../../../docs/parity/entities.toml): one row per vanilla
  actor and per component it uses, with the missing system, unsupported loot functions and
  conditions, and unsupported spawn-rule conditions in each row's `gaps`.
- [`docs/parity/systems.toml`](../../../docs/parity/systems.toml): engine systems entities
  depend on (weather, block light, riding, leads, villages and raids, game events).
- [`docs/parity/PARITY.md`](../../../docs/parity/PARITY.md): counts, and the entity
  components that block the most actors.

Refresh entity rows from the runtime itself: `MobRegistry::unsupported()` returns every
definition the server cannot spawn and the first component, filter or event that blocks it.
