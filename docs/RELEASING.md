# Releasing Drawen

Drawen uses Semantic Versioning because Cargo only understands semver. The
version lives once in the root `Cargo.toml`:

```toml
[workspace.package]
version = "0.3.0"
```

Every crate inherits it with `version.workspace = true`, and internal path
dependencies use a caret requirement (`version = "0.3", path = "../drawen-core"`)
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
| One feature or fix batch | patch: `0.3.0 -> 0.3.1 -> 0.3.2` |
| Breaking public API change | minor: `0.3.1 -> 0.4.0` |
| Stable public API | `1.0.0` (not yet planned) |

Every merged feature is released as its own patch version; versions are not
batched until a large milestone. A breaking change is never shipped in a patch
release, even while the project is pre-1.0, because `^0.3` accepts `0.3.x` for
existing users. A milestone that batches many feature branches and contains a
breaking change bumps the minor position, as 0.3.0 did for the `Video` `const`
removals, the new `PseudoClass` variants, and the new default frame caps.

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

Continuous integration runs this list on every push to `main` and every pull
request; see [`.github/workflows/ci.yml`](../.github/workflows/ci.yml). The jobs
to watch are `rustfmt`, `clippy`, `test` (Ubuntu, Windows, and macOS), and
`feature matrix`. Do not tag a release while one of them is red, and do not
treat the commands below as the gate: they exist to reproduce a CI failure on a
local machine.

1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets --all-features -- -D warnings`
3. `cargo test --workspace` and `cargo test --workspace --all-features`

   Run the second one on a machine that has the audio system libraries
   installed, so the optional `audio` and `director` paths are built too.

4. `cargo hack check -p drawen --no-dev-deps --feature-powerset --depth 2`

   CI installs `cargo-hack` for this. The command grows with the feature set, so
   a new feature is gated without editing the workflow. `--feature-powerset`
   already includes the no-default-features and default-features runs, so
   `--no-default-features` cannot be combined with it.

   The dependency graphs are asserted, not inspected. Two tests in
   `crates/drawen/tests/feature_gates.rs` run as part of `cargo test` and fail
   when `drawen --no-default-features` pulls `wgpu`, Servo, Symphonia, Paleo,
   `image`, or `cpal`; when the defaults stop enabling the renderers or media;
   or when the workspace resolves two `wgpu` versions.
5. Run semver checks for every published crate against the previous release:

   ```text
   cargo install cargo-semver-checks --locked
   git fetch --tags
   cargo semver-checks check-release --package drawen-core --baseline-rev v0.3.0
   cargo semver-checks check-release --package drawen-render --baseline-rev v0.3.0
    cargo semver-checks check-release --package drawen-signals --baseline-rev v0.3.0
   cargo semver-checks check-release --package drawen-ui --baseline-rev v0.3.0
   cargo semver-checks check-release --package drawen-widgets --baseline-rev v0.3.0
   cargo semver-checks check-release --package drawen-wgpu --baseline-rev v0.3.0
   cargo semver-checks check-release --package drawen-software --baseline-rev v0.3.0
   cargo semver-checks check-release --package drawen-tasks --baseline-rev v0.3.0
   cargo semver-checks check-release --package drawen-platform --baseline-rev v0.3.0
   cargo semver-checks check-release --package drawen-platform-winit --baseline-rev v0.3.0
   cargo semver-checks check-release --package drawen-workspace --baseline-rev v0.3.0
   cargo semver-checks check-release --package drawen-director --baseline-rev v0.3.0
   cargo semver-checks check-release --package drawen --baseline-rev v0.3.0
   cargo semver-checks check-release --package paleo --baseline-rev v0.3.0
   cargo semver-checks check-release --package paleo-h264 --baseline-rev v0.3.0
   cargo semver-checks check-release --package paleo-audio --baseline-rev v0.3.0
   cargo semver-checks check-release --package paleo-video --baseline-rev v0.3.0
   cargo semver-checks check-release --package paleo-player --baseline-rev v0.3.0
   cargo semver-checks check-release --package paleo-hevc --baseline-rev v0.3.0
   cargo semver-checks check-release --package paleo-vp9 --baseline-rev v0.3.0
   cargo semver-checks check-release --package paleo-av1 --baseline-rev v0.3.0
   ```

   The browser and Servo crates are checked on their native target. The
   commands use `v0.3.0` as the baseline for the next release; substitute the
   tag of the release being superseded. If no baseline tag exists yet (a crate
   first published in this release, such as `paleo-player` in 0.3.0), skip the
   check for that crate or use `--baseline-root` against an unpacked previous
   release instead.

   When the release bumps the minor position under `0.x`, the tool treats the
   bump as incompatible and skips every lint (`260 skip`), because breaking
   changes are already permitted. To enumerate the breaking changes for the
   changelog's Breaking changes section, force `--release-type patch` so no
   lint is allowed to pass silently; the reported failures are exactly the
   list to document. The 0.3.0 Breaking changes section was produced this way.

6. Update `CHANGELOG.md` under the new version with the release date.
7. `cargo update --workspace` to refresh `Cargo.lock`.
8. Commit `Release X.Y.Z`, then create an annotated tag whose message repeats
   the date and the changelog summary:

   ```text
   git tag -a v0.3.0 -m "Drawen 0.3.0 (2026-11-25)"
   ```

9. Publish in dependency order (core and render first, the facade last):

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
| 0.2.0 | 2026-11-17 | GPU paint-order correctness, raster image pipelines, software fill rewrite, worker task boundary |
| 0.3.0 | 2026-11-25 | Overlay layer and widget families, extended DSS, multilingual fonts, presentation-aware ticking, multicodec Paleo player, variable-height lists and autocomplete |
