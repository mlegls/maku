# Generated wasm snapshot

These files are generated together by `crates/web/build.sh` using the versions
pinned in `rust-toolchain.toml` and `mise.toml`. `release.json` records the
engine version, frame ABI, source identity, and generating tool versions.
Never update only the `.wasm`, bindgen JavaScript, declarations, or wrapper
snapshot: `scripts/check-generated.sh` rebuilds and compares the complete unit.

Local reproducibility builds use source revision `development`. A release
artifact producer sets `MAKU_SOURCE_REVISION` to the immutable source commit
and publishes the resulting directory without writing that release-specific
identity back into development sources.
