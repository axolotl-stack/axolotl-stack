# Native player lifecycle notifications

Register a `PlayerLifecycleHandler` in `PlayerLifecycleHooks`, or observe the
corresponding Bevy events. Damage and respawn authorization precede their
ordinary effects. Death is a committed notification after drops and before an
optional immediate respawn.

`PlayerDeath.inventory` owns the complete main inventory, four armour slots,
offhand, player UI container and held-slot selection captured immediately before
drops. Item metadata and empty slot positions are retained. Changing the source
inventory or respawning later does not change this image. Incomplete player
entities report `None` rather than a partial image.

`keep_inventory` records the world rule at the death. The image includes all
carried items, including items retained by that rule or by an item component; it
is not a list of drops. Reading it does not alter drops, authorize item transfers
or change respawn behavior.

The scoped component API exposes `Player.runtime_identity()` as descriptive native
lineage and world-lifetime values. They remain readable while an inventory write
is pending and do not authorize access to a departed or replacement actor.

`DeathState.death_occurrence()` retains an immutable event ID, original cause and
attribution, the rule and fall-distance observations, and the Unix millisecond
time captured at the native death boundary. A missing clock or legacy generation
is reported as unknown. Archive reads retain those same observations after the
actor departs. `SourceActorOperation.death_occurrence()` reads only an observed
canonical generation. An owning `AdmissionHold` can read the genuine loaded
initialization row in its exact world transaction before publication.

When supported native terminal persistence completes, the occurrence carries its
actual post-drop receipt. Raw state edits, restoration, ordinary saves and
inventory effects preserve the latest canonical occurrence instead of replaying
historical metadata. An occurrence is an observation, not a restoration permit;
durable game logic must still record its own acceptance before exposing effects.
