# Engine-Agnostic Danmaku Core: Design Notes

Notes from a design discussion about building an engine-agnostic bullet-hell system derived from Danmokou's (DMK) scripting model, motivated by two game concepts and informed by comparisons to SuperCollider.

---

## 1. Motivating game concepts

**Spell-card RPG** — Touhou meets Slay the Spire as the combat system of an RPG (Undertale-adjacent framing). Each combat turn, the enemy reveals a spell card and the player chooses one to counter with. The round then plays out as a Touhou boss phase: the enemy's card determines its attack/movement pattern; the player's card acts like a choice of character/shot-type/upgrade level. Spell *cards* taken literally: cards are data, decks are collections of pattern programs.

**Tunnel runner** — Neon White-style movement meets Touhou: running away from something through a tunnel (instead of auto-scrolling), with bullet patterns keyed on the player's position along the tunnel rather than on time.

Common frustration: existing modern bullet-hell engines are either engine-locked (DMK → Unity) or standalone applications, while games like NieR: Automata show the bullet-hell layer as one component inside a larger game.

---

## 2. Can bullets be pure functions? (the original question)

**Position as `f(t)`:** DMK already partially works this way. Its movement layer supports both "offset"-style functions (`pos = f(t)`, evaluated fresh each frame — e.g. `polar(2*t, 80*t)`) and "velocity"-style functions (`pos += f(t)·dt`, integrated per frame). The pure model can't be the *only* model because:

1. **Bullets consume runtime inputs, not just time.** Homing/aiming/player-reactive behavior makes the trajectory the solution of an ODE whose right-hand side contains an unpredictable signal (player position history). No closed form exists; the only general evaluation is stepwise integration — i.e., the per-frame update.
2. **Bullets get mutated by events.** Bullet controls (decelerate-all on phase break, cancel-into-items, per-bullet graze flags, reflection, deletion) force function splicing under a pure model: capture state at event time, compose a new closure. The eliminated state sneaks back in as closure environments.
3. **Numerics.** Designers think in velocities; converting arbitrary velocity functions to position functions requires symbolic integration, which frequently has no closed form (DMK's constant-speed-along-curve reparametrization needed an iterative solver).

**The field model `f(t, pos) → bullet?`:** elegant for exactly one operation — point-query collision against a tiny hitbox. It fails as the core representation because:

- Rendering, grazing, cancellation, item spawns, and scoring need to *enumerate* bullets and need bullet *identity*; a field has extension but no identity.
- Efficient implementation of the field is "∃ i : |pos − pᵢ(t)| < rᵢ" over a spatial index of an explicit bullet list — so the explicit list is the implementation and the field is just the collision-query *interface*.
- Per-pixel evaluation (the shader approach; ShaderToy danmaku demos exist) works for eye candy but loses sprites, layering, per-bullet animation, and identity.

**FRP framing:** the game truly is a function — of time *and* input history. Input arrives incrementally, so evaluation must be incremental; per-frame stepping is online evaluation of that function. DMK's compose-functions-everywhere architecture is the practical fixed point.

**Important later payoff:** the tunnel game *inverts* this conclusion — see §10.

---

## 3. How Unity-coupled is Danmokou?

The author's own README: "Honestly, I'd rather not be using Unity, but I don't know of any engines which support all this project's requirements, and at this point there's too much investment in Unity-specific handling to have a simple port." There is a 2020 GitHub issue ("About Godot...", Bagoum/danmokou#3) discussing exactly this.

**Already engine-agnostic** (separate MIT-licensed .NET Standard repos, no Unity references):

- **BagoumLib** — utilities: tweening, data structures, type unification, expression-tree helpers (including an `ExpressionPrinter` that emits *compilable* C# source from expression trees).
- **Mizuhashi** — FParsec-style combinatorial parser.
- **Scriptor** — the C#-ish scripting language (BDSL2 core) that recompiles at runtime into C# lambdas via type unification.
- **Suzunoya** — engine-agnostic visual-novel library; **SuzunoyaUnity** is its Unity adapter. This core-library-plus-adapter pattern is the template — applied to the VN half but never to the danmaku half.

**Unity-tied:**

- **Rendering** — tuned instanced mesh pipeline, custom shaders, material property blocks (the 100k-bullets-at-4k120 claim lives here); pathers use Unity's Trail Renderer.
- **UI** — a deep bespoke keyboard-navigation framework on Unity UIToolkit (UXML/USS).
- **Assets/editor** — ScriptableObjects for bullet styles/boss metadata/shot configs; TextAsset script wiring; editor tooling including the IL2CPP expression-baking pipeline.
- **Pervasive small stuff** — `UnityEngine.Vector2` etc. threaded through movement/collision code (mechanical to replace with `System.Numerics`, but everywhere).

**Notably portable:** the simulation core — pooled bullet arrays, the custom `RegularUpdate` / `RegularUpdateCollision` / `RegularUpdateFinalize` loop, cancellation-token phase semantics, state machines — is mostly plain C#. DMK doesn't use Unity physics, per-bullet GameObjects, or Unity coroutines on the hot path (that's *why* it's fast), which makes it more portable than a typical Unity project.

### Godot .NET port assessment

Easier than expected: Godot 4 .NET runs real .NET 8 with a JIT on desktop, so `Expression.Compile()` and reflection just work; the entire IL2CPP problem set disappears on desktop targets. MultiMesh/RenderingServer can plausibly match throughput.

Costs: full shader rewrite (ShaderLab/HLSL → gdshader), full UI rebuild (no UIToolkit analogue), asset-model rewrite (ScriptableObjects → Resources) plus all editor tooling, and weaker AoT/web escape hatches (the expression baker is Unity-editor code; C#-on-web is Godot's historic weak spot). Verdict: nothing conceptually blocked; a multi-month rewrite of the body around a transplantable brain. Solo-maintained upstream — a port means maintaining a fork of everything above the Suzunoya-stack line.

### Reimplementing the language enginelessly

Most tractable option, because it's half-done upstream. "BDSL2 is only supported within the context of DMK" is a statement about the *standard library*, not the language: parser, type system, and delegate compilation are ordinary consumable libraries. What's danmaku-specific is the reflected function repository and the runtime it constructs: movement vocabulary, repeaters (`gsrepeat` and modifiers), V2RV2 coordinates, environment frames, phase/pattern state machines with cancellation semantics, pool controls. All plain C# logic with shallow Unity contamination, unusually well documented (the tutorials are a semi-formal spec), and the play-mode tests — run a state machine, assert on game state — are an oracle proving the sim runs nearly headlessly today.

---

## 4. The engine-agnostic core: API shape

Proposed model: the DSL has free variables bound at the boundary (player position etc.); the host calls essentially `f(inputs, dt) → bullets`, or reads bullets via getters over internal state.

**Refinements:**

- **`step(inputs, dt) → events`, plus bulk getters over render state.** Input snapshot struct: player position, hurtbox, focus state, buttons — whatever the vocabulary needs. Script accessors like `Lplayer` (today: service lookups into live engine objects) rebind to snapshot reads; compiled delegates already take a context parameter, so this is redesign, not surgery.
- **Determinism as a core property:** the sim is a deterministic fold over `(seed, input trace)` → replays, headless testing, and rollback netcode as corollaries. Accumulate `delta` into **fixed internal timesteps**; variable dt silently changes pattern behavior and destroys determinism.
- **Keep collision inside the library.** Collision is semantically entangled with the DSL: grazing is per-bullet stateful, on-collision controls cull/spawn/trigger, lasers and pathers have procedural hitboxes (sampled curves, remembered trails). Host-side collision forces a chatty bidirectional protocol that breaks determinism and leaks the API. Inside, it's cheap: a few point tests against bucketed SoA bullets (DMK moved from parallelized collision to spatial bucketing for simple bullets). The host consumes events (hits, grazes, sfx, spawns/culls) and never writes into the sim except via the input struct and explicit commands ("bomb: clear radius r").
- **Decide early where enemies live.** Bullets-only leaks immediately (enemies fire patterns, take damage, die and cancel patterns). Stable configurations: *entire game sim inside, host is a dumb renderer/input device* (cleaner; the Suzunoya pattern applied to danmaku) or bullets-only with a fatter command API.

### Costs vs. an integrated engine

1. **The copy: nearly free.** ~100k bullets × ~40 B render state ≈ 4 MB/frame — noise against modern memory bandwidth. The trap is API *shape*: object-per-bullet or call-per-bullet dies on interop overhead. Expose per-style contiguous spans over SoA pools; the host does one bulk upload per style (maps directly onto filling a Godot MultiMesh buffer). Zero-copy achievable if the host dictates layout per style at init.
2. **Lost sim/render fusion: the real structural cost.** Integrated engines compute visual behavior in shaders — `hueshift(120*t)` is *scripting vocabulary* evaluated on the GPU. An agnostic sim must either compute visual state on the CPU or make the render contract part of the spec ("a conforming host implements hue-shift, fade, scale-in, animation frames with these semantics"). No third option. Practical split: geometric state (position/rotation/scale) sim-side; chromatic/animation effects as per-instance parameters with reference shader semantics.
3. **Pathers and lasers: the hardest boundary item.** Geometry generation (DMK leans on Unity's Trail Renderer). Options: emit polylines/sample points and let hosts build meshes, or generate vertex buffers in-library (portable, but you've written a small geometry engine). Budget real design time here.
4. **Not paid:** no extra latency frame (unless gratuitously double-buffered), no GC pressure (spans over pooled arrays), no expression-tree penalty (in .NET, the language layer compiles to delegates regardless of host).

**Rough numbers:** perhaps 10–30% frame-time overhead at pathological benchmark scale (repack + CPU-computed visual state); essentially unmeasurable at the 2k–20k bullets real games run. The performance argument for integration only bites at benchmark extremes; the engineering argument (visual semantics as spec surface, laser geometry contracts) is the one that matters.

---

## 5. Native (LLVM) core + thin per-engine wrappers

A solved integration pattern — it's how FMOD, Wwise, PhysX, Box2D ship. Per engine: **Unreal** links native C++ directly; **Unity** uses native plugin + P/Invoke (and works *better* under IL2CPP than managed code — the expression-tree/AoT problem vanishes because the code is already native); **Godot** via GDExtension or P/Invoke from .NET.

Middleware hygiene: pure C ABI (`extern "C"`, opaque handles, no exceptions or non-POD across the boundary), explicit ownership, bulk exchange as pointers to contiguous buffers — the SoA export maps onto a C ABI perfectly (`dmk_get_render_buffer(sim, style_id, &ptr, &count)`). For replays/netplay: cross-platform float determinism is the classic native-lib gotcha (fast-math, FMA contraction, libm variance) — compile with strict FP and own the transcendentals.

**The real price:** going native abandons the reusable .NET asset. Outside .NET you reimplement the parser (easy), type unification (moderate), and must answer what scripts compile *to* — LLVM ORC is heavyweight, Cranelift plausible in Rust, a naive bytecode interpreter costs maybe 2–5× on the hot path.

**The game design deletes most of that price:** a spell-card RPG's pattern content is a finite deck known at build time. AOT-compile the whole pattern library into the shipped binary (scripts → IR → native at build time — the same move as DMK's console expression-baking); keep an interpreter only as the dev-mode hot-reload tool. Runtime JIT is needed only for user-generated cards/mods. Middle path if moddability is core: compile scripts to WASM, embed wasmtime (portable, sandboxed, near-native, keeps hot-reload) at the cost of a runtime dependency.

---

## 6. What BDSL compiles to (the expression-tree pipeline)

`System.Linq.Expressions` = construct the AST of a function as runtime data, call `.Compile()` → real IL in a `DynamicMethod` → JIT → native code. The output is an ordinary delegate, performance-identical to handwritten C#. ("LINQ" in the name is historical.)

Pipeline: script text → Mizuhashi parse → type unification resolves each word against the C# function repository → **backend functions are macros, not runtime functions**. `Sine` is morally `Expression Sine(Expression period, Expression amp, Expression t)` — it returns the expression *tree* with argument trees spliced in. So

```
roffset(pxy(1 * t, sine(1, 0.2, t)))
```

macro-expands into one flat expression tree and compiles to a *single delegate* — all arithmetic and `Math.Sin` calls inlined, no per-node dispatch, no boxing, no interpretation. The script's call structure is erased at compile time (cf. Lisp macros / C++ templates). That is what "a thin wrapper around native C#" means, and it is the whole performance story.

AoT platforms hurt because without a JIT, `.Compile()` falls back to a slow BCL tree-walking interpreter — hence DMK's pipeline of printing expression trees back to compilable C# source (the `ExpressionPrinter`) and baking them into console builds.

---

## 7. Turing completeness and the two-layer execution model

BDSL2 is Turing complete in the boring sense (while/for, mutation, conditionals, blocks-as-values). But it is really **two languages at different execution frequencies**:

- **Hot layer** — movement/style functions, per bullet per frame, millions of evals/sec at scale. In practice straight-line math over `t` and a few environment variables; designers essentially never loop here. Effectively a total language.
- **Control layer** — state machines, phases, repeaters, spawn logic. Loops, waiting, unbounded behavior — but executed at spawn/event frequency (hundreds–thousands/sec).

Consequences:

- The control layer can be a plain tree-walking interpreter; its cost is irrelevant.
- The hot layer needs speed, not generality: compile each movement function to compact register bytecode over a tiny type universe (float, vec2/3, bool) and evaluate **pool-at-a-time** — all bullets in a pool share one function, so run each opcode across the whole SoA array before the next. Dispatch amortizes from per-bullet-per-op to per-pool-per-op; every op becomes a SIMD-friendly loop over contiguous floats (the NumPy/APL/shader execution model — a natural fit since DMK already pools by style). Per-bullet conditionals → branchless select.
- Restricting the hot layer (no loops per-frame) statically bounds frame cost: a malicious/malformed card mod can slow the frame but cannot hang it. Turing completeness stays quarantined in the control layer under a per-frame fuel budget.
- The shipped path is AOT-compiled decks anyway; the interpreter is dev-mode tooling. WASM/Cranelift are not required.

---

## 8. The Lisp reformulation

BDSL — especially BDSL1 — is an s-expression language wearing curly-brace cosplay: every construct is head-word + arguments, arguments parsed by reflecting the head into the repository and reading declared parameter types; `gsrepeat { times(3) circle } { ... }` is a node with a property list and children. Any slot holds a constant, a function of `t`, or an arbitrarily deep subtree, provided the *type* matches — the uniformity of a typed expression tree. The changelog's accreted syntax rules (mandatory commas, late-bolted infix with PEMDAS) are the usual cost of maintaining a non-tree surface over a tree core. BDSL2's "blocks are value expressions; value = last statement" is `progn`.

For the spell-card game, s-expressions are load-bearing, not aesthetic:

- **Cards as literal data:** an upgrade level is a tree transformation of the base card (a macro); procedural/roguelike card generation is tree generation; fusion/deck-building is tree composition; a card editor is a structured tree editor.
- **In-language abstraction:** pattern combinators, difficulty modifiers, "this card but aimed" wrappers become macros instead of backend native functions — which matters in a native core where "just add a repository function" is no longer cheap.
- **Spec and serialization:** the language spec reduces to node types + typing rules with surface syntax as a pluggable skin; s-exprs are a trivially stable serialization format for the card economy.

Caveats: danmaku scripts are dense infix trig — `80*t + sine(1, 0.2, t)` beats `(+ (* 80 t) (sine 1 0.2 t))` for nearly everyone; add an infix escape (a `math` macro or reader syntax), as every Lisp-for-numerics does. And the interesting part of BDSL was never syntax but *semantics*: static type unification with overload resolution and implicit conversions against a typed repository — unusual for a Lisp but fully compatible with one. Target formulation: **BDSL's semantic model (typed trees, unification, two-layer split) with an s-expression canonical form.**

---

## 9. Lessons from SuperCollider

Structurally the closest battle-tested precedent: a compositional "value over time" DSL, Turing-complete control language driving a restricted allocation-free hot core, 25+ years in a domain where a missed deadline is an audible click. Audio-rate DSP : per-sample per-voice :: danmaku : per-frame per-bullet.

- **sclang/scsynth split** — physically separate control language and real-time server speaking timestamped OSC. Validates the two-layer design and adds discipline: *timestamped events* (control messages carry time; the server applies them sample-accurately → spawn/parameter events should carry frame stamps, incidentally making the event stream a replayable log) and *RT-thread rules as an API contract* (server pre-allocates everything; hot path never mallocs or blocks — DMK's zero-allocation rule, enforced at an architectural boundary).
- **SynthDef/Synth** — graph compiled once; instances are cheap. = movement function compiled per style; bullets as instances (bullets are voices, pools are polyphony). SC's granular synthesis hit the same massive-instancing wall and made grains flat internal data rather than full Synths — convergent evolution with DMK's complex-entities vs. simple-bullets split.
- **Rate polymorphism (`ar`/`kr`/`ir`) — the single biggest steal.** Every signal has a rate; any slot accepts any rate with compiler-handled promotion. This is "value over time as final *and* intermediate" plus a **cost model**: the type system knows how often each subexpression must recompute. Danmaku mapping: `ir` = once at bullet spawn (captured constant); `kr` = once per pool per frame (global time, rank, boss position); `ar` = per bullet per frame. With BDSL's unification machinery, rate becomes an *inferred* property → the compiler hoists pool-invariant subexpressions automatically (compute `sine(2, 0.3, globalT)` once, not 20,000 times) — the hand-optimization SC users perform, done by inference. Composes directly with pool-at-a-time bytecode: rate inference decides prelude ops vs. SIMD-loop ops.
- **Multichannel expansion** — pass an array where a scalar is expected and the graph fans out into parallel channels: `SinOsc.ar([440, 443])` = two oscillators. Danmaku: `s(polar(2*t, [0, 120, 240] + 80*t))` = three-armed spiral. Fan-out becomes an ambient property of the expression language rather than a special repeater construct — and it is exactly the array-programming style the pool-at-a-time evaluator wants.
- **The Patterns library** — the missing theory of the control layer. `Pseq`/`Prand`/`Pbind` are pure, composable, *first-class lazy* descriptions of event streams; a `Pbind` is morally a `gsrepeat` with modifier properties; demand-rate UGens are "spawner pulls the next angle each shot." What SC adds: patterns are values with an **algebra** (transpose, stretch, interleave, wrap) — the formal backbone of cards-as-data: upgrade level = pattern transformer, difficulty scaling = stream transformation, with 20 years of compositional idioms to crib.
- **Uniform signal semantics → tooling.** `poll`/`scope` any intermediate node → select any subexpression and plot it over `t` or overlay its induced trajectory, live. **NRT mode** (identical graph renders offline, deterministic, faster than realtime) → headless conformance suite; "render this card to a preview video" as a batch job.
- **`doneAction`** — lifetime as part of the signal graph (envelope completes → free the synth). Bullet cull conditions (lifetime, off-playfield, fade-complete) as graph nodes with done-actions, unifying what DMK splits across options/controls/cull commands, and giving the compiler lifetime visibility for pool sizing.
- **What doesn't transfer (encouragingly):** audio blocks are sequential and feedback-sensitive → SC's parallelism is hard; bullets are independent within a frame → embarrassingly parallel, SIMD-able. Block size latency tradeoffs don't exist (the frame is the block). And don't steal sclang itself: the proliferation of Tidal/Overtone/FoxDot as alternative frontends to the same server is the community voting with its feet — and proof that a solid compiled-graph core outlives its surface language, exactly the property wanted here.

---

## 10. 3D generalization and the tunnel game

**What's dimension-agnostic:** functions of `(t, env)` producing motion; repeaters as coordinate-frame transforms; state machines/phases/cancellation; pooling; bullet controls. **What's secretly 2D:** V2RV2, the rotational coordinate currency — repeater modifiers (`spread`, `circle`, angle increments) assume rotation is one scalar; in 3D, rotation needs an axis.

The convergent industry answer (NieR: Automata included): *don't actually go 3D*. Nier's bullets live in embedded 2D structures — planar fans, rings, spherical shells — authored as 2D patterns with 3D *placement*. So the clean generalization is not quaternion-valued V2RV2 (mathematically annoying; produces patterns humans can't read) but an **emitter-frame layer**: patterns execute in a local oriented plane/cylinder/sphere-surface, with a small new vocabulary for positioning, orienting, and animating those frames in 3D. DMK's repeater model is already a stack of local transforms; this extends the architecture along its existing grain. New 3D costs are peripheral: billboard rendering with depth-sorted transparency, 3D spatial bucketing (trivial), and the design fact that depth makes dense patterns unreadable — itself an argument for planar patterns.

**The tunnel game inverts the §2 conclusion.** Patterns keyed on tunnel progress `s` are just reparametrization (nothing is sacred about `t`; bind pattern phase to arc-length, keep local `t` for per-bullet animation). But `s` is player-controlled and possibly **non-monotonic**: slow down and patterns slow; backtrack and patterns must run *backward*. Integrated per-frame state handles that badly-to-not-at-all (you can't un-integrate a velocity update); pure `pos = f(s)` handles it by construction — evaluate at whatever `s` the player occupies, rewind free, and pattern-speed-follows-movement-speed (Superhot-adjacent) falls out as a mechanic. Restrict reactive/stateful behaviors (homing, on-hit controls) to sections where `s` is guaranteed monotonic. Geometry cooperates: a tunnel is a cylinder; a cylinder unrolls to a plane; pattern space is `(θ, s)` — ordinary 2D danmaku authored on the unrolled surface, radius as garnish. V2RV2 needs reinterpreting, not replacing.

---

## 11. Interactive tooling: the REPL

SC's workflow — scratchpad document, select-and-evaluate any region, persistent interpreter environment — plus one capability audio cannot have.

- **Unit of evaluation = any expression, not the file.** DMK's instant recompilation re-reflects whole scripts; the REPL version is granular: evaluate a bare movement function → trajectory plot over `t`; a spawner expression → fires into a sandbox playfield; a full phase → fight it. S-expressions pay off again: "evaluate the enclosing form" is a well-defined editor gesture, and every subtree is a legal evaluation unit.
- **Sandbox context with defaults** (SC: bare UGens get default args and play to default out). Harness supplies free variables: default style, playfield rect, clock, and a **mock player** (much of the vocabulary reads player position). Mouse-bound mock player → wiggle a homing pattern's target by hand; scripted mock paths (circler, edge-hugger, streamer-bot) → regression-test aim logic. `scope`/`poll` analogue: tapped subexpressions plot live on the sandbox clock.
- **JITLib/NodeProxy → generational hot-swap.** Named slots whose definitions swap while running, with crossfade. Danmaku answer to "what happens to live bullets on redefinition": in-flight bullets keep the delegate they spawned with; new spawns get the new one; optionally fade-and-cull the old generation (visually a crossfade). BDSL2's environment frames (built for cross-script imports) are the seed: a REPL residency is a persistent frame with mutable slots.
- **Rate inference tells the UI what's twiddleable:** `kr` params bind to sliders with immediate effect on all bullets; `ir` params affect only new spawns — and the UI can *say so*, because rate is inferred.
- **Beyond SC: rewind.** Audio is forward-only; this sim is a deterministic fold over `(seed, input trace)` with flat SoA state (snapshot = memcpy) and frame-stamped events. So: pause mid-pattern, scrub backward, edit the expression, **replay the identical input trace through the new code** — watch the same dodge against the revised pattern. Bret Victor / Tomorrow Corporation-class tooling, nearly free given commitments already made for replays/netcode (checkpoint each second + input tape; rewind = restore nearest snapshot + re-step).
- **Structural alignment with the game:** the REPL's unit of prototyping coincides with the unit of content — a card is by construction independently runnable and balanceable, so the sandbox *is* the card workbench, and fight → pause → scrub → tweak → refight is the core authoring loop. A saved session (expressions + input tape + assertions) *is* a conformance test — interactive tool and headless suite become the same artifact (cf. NRT).
- **Obligation:** the dev interpreter and shipped AOT path must be semantically identical — bit-for-bit if scrub-and-replay is to be honest. Same strict-FP discipline the native core needed, with raised stakes: the workflow rests on "what I prototyped is what ships."

---

## 12. Consolidated architecture

1. **Core:** native (or .NET) deterministic simulation library; fixed timestep; SoA pooled bullets; bucketed collision *inside*; entire game sim inside, host as renderer/input device.
2. **API:** C ABI; `step(inputs, dt) → events`; bulk per-style span getters for render state; explicit command surface (bomb, clear, boss…); frame-stamped event log.
3. **Language:** BDSL semantic model — typed trees, type unification/overload resolution against a function repository — with s-expression canonical form and an infix escape for math; macros for card-level abstraction.
4. **Execution:** two layers. Control layer: interpreted state machines/patterns with cancellation semantics and a fuel budget. Hot layer: loop-free per-frame functions → register bytecode → pool-at-a-time SIMD evaluation in dev; AOT-compiled to native for shipping decks. Rate inference (`ir`/`kr`/`ar`) hoists spawn-time and pool-invariant computation automatically. Array broadcasting (MCE) for fan-out.
5. **Lifecycle:** cull conditions as done-action nodes in the value graph.
6. **3D:** 2D pattern language + emitter-frame embedding (planes/cylinders/sphere surfaces); tunnel game as `(θ, s)` danmaku on an unrolled cylinder with position-parametrized pure-offset patterns.
7. **Tooling:** expression-level REPL with sandbox + mock player, generational hot-swap, live parameter binding informed by rate inference, signal tapping/plotting, and deterministic rewind/scrub/replay; sessions double as conformance tests.
8. **Risks to budget:** laser/pather geometry contract; render-semantics spec surface (hue-shift etc.); cross-platform strict-FP determinism; dev-interpreter ≡ shipped-AOT semantic equivalence; reimplementing type unification outside .NET if going native.

---

## 13. References

- Danmokou: https://github.com/Bagoum/danmokou — docs at https://dmk.bagoum.com/docs/ (design philosophy: `articles/t06.html`; AoT/expression baking: `articles/AoTSupport.html`; BDSL2 guide: `articles/language/guide1.html`)
- Godot discussion: https://github.com/Bagoum/danmokou/issues/3
- Suzunoya stack (BagoumLib, Mizuhashi, Scriptor, Suzunoya): https://github.com/Bagoum/suzunoya
- SuperCollider: https://supercollider.github.io/ (SynthDef/UGen rates, JITLib, Patterns, NRT)
- BulletML / libbulletml (Kenta Cho) — prior art for embeddable engine-agnostic danmaku
