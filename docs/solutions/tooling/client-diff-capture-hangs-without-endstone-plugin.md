---
title: client-diff capture mode hangs silently if the Endstone venv lacks the capture plugin
area: tooling
tags: [client-diff, endstone, captures]
date: 2026-10-24
origin: data-driven items research appendix (inventory-moves scenario, db31cda)
status: current
---

## Context

`tools/client-diff/run.ps1 -Mode capture` starts BDS with Endstone and waits for `ready.json`.

## What we learned

If the shared runtime's Endstone virtualenv does not have the `endstone_client_diff` plugin
installed, nothing writes `ready.json`, and the run times out with no other error. This blocked the
`inventory-moves` scenario in October 2026. Check the plugin is installed in that venv before
debugging the scenario itself.

## Where it lives

`tools/client-diff/` (`run.ps1`, README).
