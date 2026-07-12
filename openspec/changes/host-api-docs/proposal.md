# Host API docs and tooling follow-ups

Backlog stub — design/specs/tasks are generated when this is picked up.

## Why

Host-facing surface items with no better home: the host API is undocumented outside the code, and two tooling/policy items wait on demand.

## What Changes

- Write `docs/host-api.md` from `core::host::Instance` as the first non-macroquad frontend exercises it (the mesh pack + web host are the forcing functions; web-host adoption of the mesh pack's buffers is a candidate slice).
- Signal tapping/plotting: select a subexpression and plot over `t`.
- Host-facing tick-rate configurability remains a later policy decision (the rate is World-owned `TickTiming`; runtime paths read it).

## Capabilities

Docs + tooling; no engine semantics.

## Impact

- `docs/`, `proto/core/src/host.rs`, web host.
- Tutorial/site direction: tutorials t01-t09, tbosses, tstages are ported; future doc work focuses on the tutorial site, reader view, and host API docs. Tutorials stay standalone (DMK mapping only in `docs/from-dmk.md`).
