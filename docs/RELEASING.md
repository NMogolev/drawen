# Releasing Drawen

Drawen uses Semantic Versioning because Cargo only understands semver. The
version lives once in the root `Cargo.toml`:

```toml
[workspace.package]
version = "0.2.0"
```

Every crate inherits it with `version.workspace = true`, and internal path
dependencies use a caret requirement (`version = "0.2", path = "../drawen-core"`)
so a patch release does not require editing twenty manifests.

## Why not calendar versioning

Calendar versions were considered and rejected. They are *representable* in
Cargo, but they are not *usable* as semver:

- `2026.11.15` parses as major 2026, minor 11, patch 15. Semver then treats a
  plain date change in the minor position as a new feature line and 2027 as a
  major (breaking) release. The numbers stop meaning what Cargo's resolver and
  `cargo-semver-checks` assume.
- `0.20261115.0` avoids a huge major, but under `0.x` every minor bump is an
  incompatible-change boundary, so each date would look like a breaking release.
- `0.2.0+20261115` build metadata is ignored by dependency resolution and by
  crates.io ordering, so it cannot distinguish two builds of the same version.

The date is therefore recorded where it does not fight the resolver: in the
`CHANGELOG.md` heading and in the annotated `vX.Y.Z` tag message.

## Version rules

| Change | Version |
| --- | --- |
| One feature or fix batch | patch: `0.2.0 -> 0.2.1 -> 0.2.2` |
| Breaking public API change | minor: `0.2.3 -> 0.3.0` |
| Stable public API | `1.0.0` (not yet planned) |

Every merged feature is released as its own patch version; versions are not
batched until a large milestone. A breaking change is never shipped in a patch
release, even while the project is pre-1.0, because `^0.2` accepts `0.2.x` for
existing users.

Examples of breaking changes:

- changing a public function signature (`GpuRenderer::render` gained
  `&mut self` in 0.2.0);
- removing `const` from a public function (`drawen_widgets::Image::new` in
  0.2.0);
- removing, renaming, or changing the type of a public item;
- changing default behaviour that existing code depends on.

Adding a method, adding an enum variant behind `#[non_exhaustive]`, or adding an
optional feature is a patch release.

## Release checklist

1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets`
3. `cargo test --workspace`

   Run both with `--all-features` on a machine that has the audio system
   libraries installed, so the optional `audio` and `director` paths are built
   too.
4. Run semver checks for every published crate against the previous release:

   ```text
   cargo install cargo-semver-checks --locked
   git fetch --tags
   cargo semver-checks check-release --package drawen-core --baseline-rev v0.1.0
   cargo semver-checks check-release --package drawen-render --baseline-rev v0.1.0
   cargo semver-checks check-release --package drawen-ui --baseline-rev v0.1.0
   cargo semver-checks check-release --package drawen-widgets --baseline-rev v0.1.0
   cargo semver-checks check-release --package drawen-wgpu --baseline-rev v0.1.0
   cargo semver-checks check-release --package drawen-software --baseline-rev v0.1.0
   cargo semver-checks check-release --package drawen-tasks --baseline-rev v0.1.0
   cargo semver-checks check-release --package drawen-platform --baseline-rev v0.1.0
   cargo semver-checks check-release --package drawen-platform-winit --baseline-rev v0.1.0
   cargo semver-checks check-release --package drawen-workspace --baseline-rev v0.1.0
   cargo semver-checks check-release --package drawen-director --baseline-rev v0.1.0
   cargo semver-checks check-release --package drawen --baseline-rev v0.1.0
   cargo semver-checks check-release --package paleo --baseline-rev v0.1.0
   cargo semver-checks check-release --package paleo-h264 --baseline-rev v0.1.0
   cargo semver-checks check-release --package paleo-audio --baseline-rev v0.1.0
   ```

   The browser and Servo crates are checked on their native target. The
   commands use `v0.1.0` as the baseline for the 0.2.0 release; substitute the
   tag of the release being superseded. If no baseline tag exists yet, use
   `--baseline-root` against an unpacked previous release instead.

5. Update `CHANGELOG.md` under the new version with the release date.
6. `cargo update --workspace` to refresh `Cargo.lock`.
7. Commit `Release X.Y.Z`, then create an annotated tag whose message repeats
   the date and the changelog summary:

   ```text
   git tag -a v0.2.0 -m "Drawen 0.2.0 (2026-11-15)"
   ```

8. Publish in dependency order (core and render first, the facade last):

   ```text
   cargo publish -p drawen-core
   cargo publish -p drawen-render
   ...
   cargo publish -p drawen
   ```

Do not publish a crate whose semver check reports an error. If the error is
unexpected, fix the API or bump the minor version; do not silence the check.

## Version history

| Version | Date | Summary |
| --- | --- | --- |
| 0.1.0 | 2026-11-14 | Initial published crate set |
| 0.2.0 | 2026-11-15 | GPU paint-order correctness, raster image pipelines, software fill rewrite |
