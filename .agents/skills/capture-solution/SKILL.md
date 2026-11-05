---
name: capture-solution
description: Capture a reusable lesson in docs/solutions after finishing work — a trap you hit, a non-obvious root cause, a measurement, a technique that worked. Use at the end of every non-trivial task and during OpenSpec archive; also use to mark older solution notes stale when the change contradicts them. This is the step that makes the next task easier; do not skip it.
---

# Capture a solution note

`docs/solutions/` is the project's memory of what was learned the hard way. Future plans read it
before starting (`rg -n -i '<topic>' docs/solutions/`).

## Is it worth a note?

Write one when a competent newcomer would likely repeat the mistake or redo the investigation:
a misleading symptom, a wrong first fix, a measured number that drove a design, a BDS behaviour
that took captures to understand, a tool invocation that took a while to get right.
Don't write one for things already in a spec, decision record, quirk row, code comment or README —
put it there instead if that's the better home. One lesson per note.

## Steps

1. Search for related notes first. If one covers it, extend it rather than duplicating.
2. If your change makes an existing note wrong, set its `status: stale` and add a line
   `superseded-by: <new note or commit>`; if only partly wrong, fix it in place.
3. Create `docs/solutions/<area>/<kebab-title>.md`. Areas: `protocol`, `transport`, `auth`,
   `server`, `data`, `worldgen`, `plugins`, `testing`, `tooling` (add one if none fits).

   ```markdown
   ---
   title: <one line, phrased as the lesson>
   area: <area>
   tags: [<a few search words>]
   date: <YYYY-MM-DD>
   origin: <PR, commit, or openspec/changes/archive/<change>>
   status: current
   ---

   ## Context
   <the problem, 2–4 sentences>

   ## What we learned
   <the lesson, concrete, numbers where you have them; the wrong path and why it was wrong>

   ## Where it lives
   <file paths>
   ```
4. No BDS/client internal names, addresses or offsets.
