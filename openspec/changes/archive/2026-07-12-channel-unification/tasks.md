# Tasks

## 1. Surface and scoping

- [x] 1.1 Sigiled binding forms construct streams: `(def $x)` / `(def $x init)` at top level, `(let [$x init] ...)` locally; sigiled pattern parameters receive stream handles, unsigiled parameters snap. Reference-position semantics (bare read snaps, `(live $x)`, `(set! $x v)`, per-tick signal-body reads) unchanged from today's channels.
- [x] 1.2 `(bind! $x expr)` as the single producer-attachment form (resolves `$x` in scope; `bind-channel!` dissolves); `(export! $x)` / `(export! $x :as $name)` with collision as load error; `(from-host :name)` as a standalone stream-valued expression. `defchannel` becomes a lib macro over `def` + `bind!` + `export!`.
- [x] 1.3 Free-`$name` resolution is a load error; the host-channel manifest is the set of `(from-host ...)` sites. Lint (not error) for `set!` on a stream with an always-writing producer.

## 2. Runtime unification

- [x] 2.1 Dissolve the dynamic cell scope: delete `CELLS_KEY` env threading, `cell_scope`, `fresh_cell_scope`, the CallPattern caller_cells/fresh_cells adapter plumbing, and the signal-gated bare-sym cell-read arm. The id-keyed `sig.cells` backing store remains as runtime representation (handles get deterministic identity from deterministic re-execution; `set!` stays a frame-stamped action).
- [x] 2.2 `DynNode::Live` holds a stream handle instead of a channel name; local streams anchor tracking frames identically to exported ones. Captured handles classify as channel-input slots in lowering (no name analysis) — stay within the DynNode ≤96-byte guard.
- [x] 2.3 Producer refresh keyed on bind!ed-ness with pinned order (defs in order, then bound producers); producer yielding `nothing` leaves the last `set!` standing, with no host special case.

## 3. Load-time schema pass

- [x] 3.1 One shared load-time collection pass building the channel manifest, per-kind render row schemas, and entity field tables (separate tables, shared machinery); missing host channels fail the load naming the channel, before tick 0.

## 4. Migration and gates

- [x] 4.1 Migrate cell users: `cards/tutorials/t05.maku`, `cards/translations/ph_boss2_spell2.maku`, stdlib `defchannel`s in `crates/core/lib/touhou.maku`; update tutorial prose (docs/tutorials) where it teaches cells.
- [x] 4.2 Update the `## Reference` section of `openspec/specs/language/spec.md` (§3 injected signals / cells) to the unified stream model at archive time.
- [x] 4.3 Gates: `cargo test --release --manifest-path crates/core/Cargo.toml` plus the 4 ignored oracle card suites with `MAKU_LOWER_ORACLE=1`; commit each coherent change-set.
