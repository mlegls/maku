# Embedding Maku 0.1 in a Rust host

> **Supported surface:** `maku::host`, `maku::source`, and `maku::render`.
> Interpreter, physical simulation storage, lowering, and raw value modules are
> implementation details even when workspace code can reach them.

```toml
[dependencies]
maku = "0.1"
maku-render-touhou = "0.1" # optional bundled render policy
```

## Minimal lifecycle

`Instance` owns card loading, capability negotiation, deterministic simulation,
input and command tapes, snapshots, and backend-neutral render transport.

```rust
use maku::host::{Inputs, Instance};
use maku::render::RenderItem;

fn main() {
    let mut maku = Instance::new(None);
    maku.set_render_kinds(["sprite", "beam"]);
    maku.add_file(
        "example.maku",
        r#"
(import "touhou")
(defpattern main []
  (bullet (pose c[1 2])
          {:style {:family :orb :color :red :variant :w}}))
"#,
    );
    maku.boot("example.maku".into(), Some("main".into()));
    assert!(maku.running(), "{}", maku.status());

    let mut inputs = Inputs::default();
    inputs.set_vec2("player", 0.0, -3.0);
    maku.advance(inputs);

    for item in maku.render_frame() {
        match item {
            RenderItem::Row(row) => println!("row kind={}", row.kind),
            RenderItem::Batch(batch) => println!("batch kind={} rows={}", batch.schema.kind, batch.len),
        }
    }
}
```

`Instance::new` optionally accepts **expanded rig source**, not a path. A rig is
layered into every fresh timeline as a tick-zero command. Most hosts can start
with `None`; the native player uses the stock player rig.

`Instance` and its render values use single-threaded shared ownership. Keep the
instance on its owning thread and send input/commands to that thread if the
frontend is concurrent.

## Loading cards and libraries

`boot(path, pattern)` reads and expands a card, selects a pattern, and starts a
fresh timeline. With `None`, the first `defpattern` is selected.

| Method | Behavior |
|---|---|
| `add_file(path, source)` | Add/replace a path in the in-memory VFS |
| `boot(path, pattern)` | Load and play |
| `reload_from_disk()` | Reload and refresh `patterns()` without playing |
| `restart()` | Start the selected card/pattern on a fresh timeline |
| `reload_restart()` | Reload, then restart on success |
| `select(index)` | Select a menu pattern and restart |
| `clear()` | Stop while retaining loaded card, VFS, and capabilities |

Once `add_file` enables the VFS, path imports are read from it rather than
falling back to the native filesystem. Add the entry card and every transitive
path import before booting. Bare libraries such as `(import "touhou")` remain
embedded and need no VFS entry.

`maku::source` also exposes `stdlib`, `expand_src`, `expand_card`, and
`expand_card_with` for preparing rig source or integrating a custom source
store. `Instance` already expands ordinary cards during load.

Operations report through `status()` and state queries rather than returning a
`Result`. Check the operation's boolean where available and then `running()`:

```rust
if !maku.reload_from_disk() {
    eprintln!("reload failed: {}", maku.status());
}
maku.restart();
if !maku.running() {
    eprintln!("start failed: {}", maku.status());
}
```

A failed read can leave an older timeline running, so `running()` alone does
not prove that the newly requested card loaded.

## Negotiate host and renderer capabilities

Configure strict capabilities before `boot`, `restart`, or `select`:

```rust
maku.set_host_channels([
    "player", "move-x", "move-y", "focus-firing", "bomb", "rank",
]);
maku.set_render_kinds(["sprite", "beam"]);
```

At instantiation, every `from-host` site and every declared render kind is
checked before tick zero. Missing host channels or unsupported declared render
kinds stop the load with an actionable status. Statically emitted undeclared
render kinds remain legal but produce a lint. Leaving a capability list unset
keeps permissive pre-0.1 behavior; setting an empty iterator enables strict
checking with no supported entries.

Capability changes apply to subsequent loads, not an already-running world.
Live source sent through command transport is trusted tooling input, not a
capability sandbox.

## Fixed-tick advancement and inputs

The host accumulates real time and calls `advance` once per simulation tick.
Do not pass variable display delta time into Maku.

```rust
fn update(maku: &mut maku::host::Instance,
          lag: &mut f64,
          elapsed: f64,
          inputs: &maku::host::Inputs) {
    if !maku.running() || maku.paused() {
        return;
    }
    *lag += elapsed;
    while *lag >= 1.0 / maku.tick_rate() {
        *lag -= 1.0 / maku.tick_rate();
        maku.advance(inputs.clone());
        if maku.paused() { break; }
    }
}
```

`tick_rate()` reports the active rate (120 Hz by default). `paused()` is host
policy: `advance` does not itself refuse to step when paused, so the frontend
must guard it.

Build a complete sample for each tick:

```rust
let mut inputs = maku::host::Inputs::default();
inputs.set_vec2("player", 3.0, -2.0);
inputs.set_num("move-x", -1.0);
inputs.set_num("move-y", 0.0);
inputs.set_flag("focus-firing", true);
inputs.set_flag("bomb", false);
```

Use these typed setters or `Inputs::classic`. Raw interpreter-valued input
methods are not part of the supported facade. Send explicit zero/false samples
for released controls. During replay of recorded future ticks, taped inputs are
authoritative.

## Channels, events, and host state

Supported typed channel reads return `None` when absent or of another type:

```rust
if let Some(lives) = maku.channel_num("lives") {
    println!("lives={lives}");
}
if let Some((x, y)) = maku.channel_point("player") {
    println!("player=({x}, {y})");
}
```

`positions(column)`, `entity_count()`, `graze()`, `player_hits()`, and
`iframes_active()` serve common host displays. The untyped `channel()` and
`cells()` methods expose internal values and are not supported API.

Events are deterministic, frame-stamped output suitable for sound and visual
side effects:

```rust
let now = maku.tick().unwrap_or(0);
for event in maku.recent_events(24) {
    println!("{} age={} pos={:?}", event.name, now - event.tick, event.pos);
}
```

`recent_events(max_age)` is inclusive and newest-first. Scrubbing reconstructs
the event log along with the world.

## Ordered render transport

`render_frame()` returns authoritative `Vec<RenderItem>`. Each item is one row
or one typed batch at that exact stream position. Batch lanes are in matched-row
order. Do not globally regroup items by kind, schema, or material when that
would change transparent draw order.

`RenderItem::expand_into` defines exact row expansion. `render()` expands the
whole frame for compatibility; a high-throughput custom renderer should consume
`RenderBatch` columns directly.

```rust
use maku::render::{RenderItem, RenderRow};

fn expanded(frame: &[RenderItem]) -> Vec<RenderRow> {
    let mut rows = Vec::new();
    for item in frame {
        item.expand_into(&mut rows);
    }
    rows
}
```

After load, `declared_render_schema(kind)` returns the stable schema identity
for a declared kind. Cache validated bindings against that identity. Runtime-
accreted undeclared kinds do not have this negotiated guarantee.

Core frame items are owned and remain valid while held, but retaining old
frames can prevent storage reuse. Render or copy promptly. Render-pack and wasm
adapters reuse their own buffers and have the shorter lifetimes documented in
[`renderer-api.md`](renderer-api.md).

Render output is a tick snapshot. Interpolation between simulation ticks is
frontend policy.

## Optional Touhou render pack

The pack compiles semantic sprite/beam transport to fixed buffers and ordered
material commands:

```rust
use maku_render_touhou::{TouhouMesh, TouhouProfile};
use std::rc::Rc;

let mut pack = TouhouMesh::new(Rc::new(TouhouProfile::stock()));
for kind in TouhouMesh::RENDER_KINDS {
    if let Some(schema) = maku.declared_render_schema(kind) {
        pack.bind_schema(kind, schema).expect("compatible schema");
    }
}
let transport = maku.render_frame();
let mesh_frame = pack.build(&transport).expect("valid render frame");
for command in &mesh_frame.draws {
    println!("material {} source {:?}", command.material, command.source);
}
```

The profile owns Touhou palette, family, orientation, radius, layer, material,
and resource policy. The frontend owns texture/pipeline creation, upload,
submission, and GPU lifetime. See [`renderer-api.md`](renderer-api.md).

## Timeline, replay, and scrub

`timeline()` provides UI metadata: current tick, input tape length, snapshot
ticks, and command ticks. Snapshots are seek caches, not semantic checkpoints.

```rust
maku.seek(120); // synchronous and pauses
assert_eq!(maku.tick(), Some(120));
let frame_at_120 = maku.render_frame();

// Resume explicitly. This branches and truncates recorded future state.
maku.set_paused(false);
```

Seeking restores a snapshot and replays ordered input and command tapes. Seeking
past the recorded tip computes new ticks using the last live input; a history-
only scrub UI should clamp to `timeline().tape_len`.

Resuming after rewind intentionally discards future inputs, commands, and
snapshots. Use `set_paused(false)` or `toggle_pause()` to make that branch
explicit rather than calling `advance` directly after `seek`.

The wire facade accepts forms such as:

```text
(seek 120)
(step 1)
(snapshots 120)
(resume)
(run ...)
(add ...)
(swap ...)
(resize-entities 200000)
```

`run`, `add`, and `swap` are live-development tools recorded on the command
tape. Parse and execution diagnostics appear in `status()`.

## Errors and teardown

Typical status prefixes distinguish read, load/check/capability, simulation,
seek, command parse, and live-edit failures. A simulation error pauses the
instance for inspection. A load failure stops the attempted new timeline.
Renderer errors are frontend errors; a host may report them with `set_status`
and pause explicitly.

`clear()` stops simulation and discards timeline state while retaining the
loaded card and configuration. `restart()` can then create a fresh timeline.
There is no engine shutdown call: drop `Instance`. Windows, audio, threads,
networking, renderer buffers, and GPU resources belong to the frontend and must
be released there.

## Contract links

- Supported/unstable boundary: [`public-api.md`](public-api.md)
- Language inputs/rendering: [`language-reference.md`](language-reference.md)
- Render adapters: [`renderer-api.md`](renderer-api.md)
- Normative session behavior: [`openspec/specs/session/spec.md`](../openspec/specs/session/spec.md)
- Ordered transport: [`openspec/specs/render-rows/spec.md`](../openspec/specs/render-rows/spec.md)
- Load-time schemas: [`openspec/specs/load-time-schema/spec.md`](../openspec/specs/load-time-schema/spec.md)
