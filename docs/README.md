# Maku documentation

Documentation describes **Maku 0.2** and the source revision reported by the
release artifact's `release.json`. Capability specs under `openspec/specs/` are
the semantic authority; [`release-notes/0.2.0.md`](release-notes/0.2.0.md) is
the migration authority. Backlog OpenSpec changes are plans, not current
language or API documentation.

## Choose a path

| Audience | Start here | Continue with | Governing capabilities |
|---|---|---|---|
| Card authors | [`tutorials/01-first-bullets.md`](tutorials/01-first-bullets.md) | `docs/language-reference.md`, then the remaining tutorials | `language`, `language-type-checking`, `determinism` |
| Rust embedders | [`public-api.md`](public-api.md) | `docs/host-api.md`, `crates/core/README.md` | `session`, `render-rows`, `release-packaging` |
| Renderer/frontend authors | [`renderer-api.md`](renderer-api.md) | `crates/core/README.md`, `crates/web/README.md` | `render-rows`, `mesh-renderer-api` |
| Native player users | [`player.md`](player.md) | `crates/player/README.md` | `session`, `mesh-renderer-api` |
| JavaScript users | `crates/js/maku/README.md` | `crates/web/README.md`, `docs/renderer-api.md` | `web-demo-delivery`, `release-packaging` |
| Migrating DMK authors | [`from-dmk.md`](from-dmk.md) | tutorials and language reference | `language` |
| Performance readers | [`benchmarks/`](benchmarks/) | dated reports and retained raw envelopes under `bench/results/` | `perf`, `scale-benchmarking` |
| Release maintainers | [`releasing.md`](releasing.md) | release notes and generated `release.json` | `release-packaging` |

## Authority and version presentation

Published pages display the package version, frame ABI, and source revision
from the same atomic web release manifest used by the runtime. Repository
Markdown links to the pre-release record until an immutable release manifest
exists. Documentation must not infer current behavior from an active backlog
change.

## Example validation contract

- Rust examples are compiled from an external smoke crate using only supported
  package APIs.
- JavaScript examples are imported and executed against the generated wrapper
  and wasm release unit.
- Fenced `.maku` examples are extracted, wrapped only when explicitly marked as
  fragments, and parsed/type-checked with the declared standard libraries.
- Links and referenced artifact paths are checked from a clean checkout.

The release gate runs these checks together with the package and browser smoke
checks. Examples intentionally demonstrating an error must be labeled with the
expected diagnostic rather than silently excluded.
