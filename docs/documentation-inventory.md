# Documentation inventory for Maku 0.1

This maintainer inventory records the starting point for the 0.1 documentation
and demo refresh. The public entry point is [`README.md`](README.md).

## Canonical upstream material

| Area | Current source | 0.1 action |
|---|---|---|
| Project/package overview | root `README.md` | Keep package names and scoped npm name current; link the public document map. |
| Supported Rust boundary | `docs/public-api.md`, `crates/core/README.md` | Expand into `docs/host-api.md`; retain the explicit unstable-internals boundary. |
| Card-author learning | `docs/tutorials/*.md` | Syntax-check every card and remove renderer-policy claims. |
| Language semantics | `openspec/specs/language/spec.md` | Derive a lookup-oriented `docs/language-reference.md`; the capability spec remains normative. |
| Native player | `docs/player.md`, `crates/player/README.md` | Remove the core-owned palette and flat-dot/beam descriptions. |
| Render transport/pack | `crates/render-touhou/README.md`, `docs/public-api.md` | Document BYO batches, fixed layouts, manifests, ordering, and host-owned resources. |
| Wasm/JavaScript | `crates/web/README.md`, `crates/js/maku/README.md` | Add identity, lifetime, full frame ABI, and Canvas2D adapter guidance. |
| Migration/release | `docs/release-notes/0.1.0-pre.md`, `docs/releasing.md` | Treat release notes as migration authority and generated `release.json` as artifact identity. |
| DMK comparison | `docs/from-dmk.md` | Keep comparative material out of the Maku-first reference and tutorials. |

## Known stale upstream claims

- `docs/player.md` and the `maku-web` crate prose describe flat `f32`
  dots/beams and place palette ownership in core. The current contract is
  ordered typed transport interpreted by the optional Touhou profile.
- `docs/tutorials/01-first-bullets.md` implies that a style record directly
  selects a sprite. It supplies semantics; profile policy selects resources.
- `docs/tutorials/04-pathers-and-lasers.md` attributes rendering to
  `touhou.maku`. The library defines gameplay/lifecycle vocabulary while the
  host-selected pack realizes rendering.
- The JavaScript package guide omits release identity, frame construction,
  typed-view invalidation, and material/resource resolution.
- No public language reference or complete `Instance` lifecycle guide exists.

## Browser surfaces

`crates/web/static/main.js` already consumes ordered sprite/ribbon commands,
but presentation still needs to identify it as a Canvas2D compatibility
adapter. `crates/web/static/manifest.js` selects content; it is not the atomic
release manifest. The generated `static/pkg/release.json` currently carries
identity but must grow artifact hashes and the selected synchronization unit.

## Downstream neen.ink snapshot

The coordinated downstream copy is `~/dev/neen-ink/projects/maku`.

- `main.js` still calls the removed `dots()` and `beams()` protocol.
- `pkg/` is an older wasm/wrapper unit without release identity checks.
- `cards/`, `tutorials/`, and `from-dmk.md` are divergent copies rather than a
  declared synchronized set.
- `play.html`, `tutorials.html`, `danmaku-site.css`, navigation, drawer, modal,
  and project routing are site-owned and must remain downstream customizations.
- The site currently has no Maku route/wasm/render deployment smoke test.

The downstream refresh therefore consumes a manifest-selected upstream runtime
and content set; it must not replace only the wasm binary or overwrite site
chrome.
