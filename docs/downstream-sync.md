# Synchronizing the neen.ink Maku project

The machine-readable selection is
[`crates/web/release-sync.json`](../crates/web/release-sync.json). It is the
only supported list of runtime, library, card, and tutorial copies. The atomic
browser identity and artifact hashes are in generated `release.json`.

## Prepare upstream

1. Start from a clean Maku release commit and set `MAKU_SOURCE_REVISION` to its
   full commit id.
2. Run `crates/web/build.sh`, `bun scripts/check-web-release.mjs`, and the
   browser smoke test.
3. Confirm that `release.json` names the intended package versions, frame ABI,
   source revision, and artifact hashes.

## Synchronize declared files

```sh
bun scripts/sync-neen-maku.mjs --write ~/dev/neen-ink/projects/maku
bun scripts/sync-neen-maku.mjs --check ~/dev/neen-ink/projects/maku
```

The command deliberately does not copy page chrome. Preserve neen.ink-owned
`play.html`, `tutorials.html`, `danmaku-site.css`, `index.mdx`, navigation,
drawer/modal behavior, and route policy. Integrate the synchronized
`canvas-renderer.js` behind that UI rather than copying upstream `main.js`.

The runtime unit includes the wasm binary, bindgen glue/declarations, wrapper,
and identity manifest. Updating only `maku_bg.wasm` is invalid.

## Downstream integration checklist

- Record the upstream Maku commit and generated release identity in the
  downstream integration record.
- Replace all legacy point/beam protocol calls with `build_render_frame()`,
  manifest resolution, and ordered command submission through the synchronized
  Canvas2D adapter.
- Keep site input controls and chrome outside the adapter.
- Ensure downstream `manifest.js` lists exactly the synchronized content paths
  required by the site, including `cards/render-pack-showcase.maku`.
- Run compatibility scanning over synchronized cards/tutorials.
- Run route smoke coverage for `/projects/maku/play.html`, tutorial routes,
  JavaScript modules, wasm MIME, runtime identity, and a mixed showcase frame.
- Build and test the complete site, then commit downstream separately.
- Record both upstream and downstream commit ids before deployment.
- Retain the prior deployed artifact until production smoke checks pass.

The sync script compares bytes and never deletes undeclared downstream files.
Removal of an obsolete copied file is an explicit reviewed downstream change.
