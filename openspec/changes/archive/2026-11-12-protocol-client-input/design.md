# Design: book editing

## Decisions

### Where the open book lives

Options: (a) a component on the player (`OpenBook { slot }`), set by the item use and cleared by
signing; (b) a flag in the book's NBT. BDS's behaviour is per player (opening a second book closes
the first, selecting another slot keeps it open), so (a).

### How the item use opens a book

`item_use::UseKind` is the one type for "what an item use on air does"; it gains `OpenBook`
instead of a separate check in the packet router. The book items are resolved from the item
registry once (`BookItems::resolve`, a typed error), so runtime code compares item ids, not names.

### NBT key order

Options and outcome in decision 0048 (proposed): a sorted writer for item user data.

## Risks

- Books opened before the sorted writer existed are unaffected (their NBT was never written by
  unastar).
- Whether the open state survives a book moved to another slot was not measured; the gate is by
  slot.
