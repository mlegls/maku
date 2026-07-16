# Tasks

Sequencing: hold implementation until scoped-channel-overrides' working tree
lands — crates/core/lib is compile-time embedded, so these edits rebuild its test
corpus mid-flight.

## 1. Lib

- [x] 1.1 `family-hitbox` table + template resolution chain (explicit `:hitbox` > family lookup over literal `:style` > flat default; bullet/shot 0.12 and player 0.06 flat defaults unchanged); update the lib header prose that currently carries the radii as a comment.
- [x] 1.2 `:spell` opt on `phases` clauses: entry `set!`s the `$spell` local stream to `{:name … :type …}` + `(event :spell-declared)`; exit clears to `nothing` + `(event :spell-end)` via the clause finally. `boss` allocates `(let [$spell nothing] …)` and extends its producer map to `{:hp … :pos … :spell $spell}`; document `$spell` as a bound convention name next to `boss-main`.
- [x] 1.3 Dissolve `col-or` into `default` (lib-internal call sites; delete the defn).

## 2. Call-site sweep

- [x] 2.1 Drop `:hitbox` at call sites where it equals the family default (duel, coop, reimu_vs_mima shots, tutorials t04); values are identical by construction, so collider output is bit-exact.
- [x] 2.2 Port `reimu_vs_mima`'s hand-rolled `(event :spell)` clause to the `:spell` opt.

## 3. Gates

- [x] 3.1 `cargo test --release --manifest-path crates/core/Cargo.toml` plus the 4 ignored oracle card suites (lib edits change every embedded card). Commit each coherent change-set.
