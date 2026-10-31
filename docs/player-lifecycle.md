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
