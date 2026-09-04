# RimMod Release Readiness

Last reviewed: 2026-09-03

## Current status

**Not ready for a public release.**

RimMod's implementation now meets the safety bar described below for a public alpha: both formal release blockers are fixed, mod scanning tolerates individual failures, load-order writes are protected, and core user actions report failures. Public-project documentation, licensing, CI, and packaging work still need completion before publishing a release.

## Release blockers

### 1. Saving after a load failure can empty the active mod list

- [x] Disable Save unless the load order was loaded successfully.
- [x] Never treat a failed load as an empty, valid load order.
- [x] Show a persistent error explaining why saving is unavailable.
- [x] Add a regression test covering this scenario.

Relevant code:

- `src/app.rs`: application loading and saving
- `src/ui/bottom_panel.rs`: Save button
- `src/services/load_order.rs`: `ModsConfig.xml` writing

### 2. Settings cannot be applied

The Settings window now applies validated paths transactionally and keeps the active settings unchanged when validation fails.

- [x] Add Apply and Cancel buttons.
- [x] Validate selected paths before accepting them.
- [x] Persist valid settings.
- [x] Reload the game version and mod list after applying settings, or clearly request a restart.
- [x] Show validation errors inside the Settings window.

Relevant code:

- `src/ui/settings_window.rs`
- `src/app.rs`

## High-priority work

### Resilient mod loading

- [x] Skip an invalid mod and report it instead of aborting the entire scan.
- [x] Include the affected file or directory in parsing errors.
- [x] Do not require Git to be installed just because a mod contains a `.git` directory.
- [x] Allow RimMod to work when the Steam Workshop directory does not exist.
- [x] Display a summary of skipped or invalid mods.

### Reliable user actions

- [x] Display errors from manual reordering.
- [x] Display errors from automatic sorting, including dependency cycles and duplicate package IDs.
- [x] Report failures when opening files, folders, or URLs in the GUI.
- [x] Prevent buttons from appearing to succeed when an operation failed.

### Configuration safety

- [x] Write a new configuration to a temporary file first.
- [x] Validate the completed XML before replacing the original file.
- [x] Replace the original using an atomic rename where supported.
- [x] Retain a recoverable backup and tell the user where it is.
- [x] Add tests proving that a failed write does not damage the existing configuration.

## Testing requirements

- [x] Parsing valid and malformed `About.xml` files
- [x] Loading a mixture of valid and invalid mods
- [x] Reading an existing `ModsConfig.xml`
- [x] Preserving active mods when loading fails
- [x] Saving and restoring a load-order backup
- [x] Handling missing package IDs
- [x] Detecting duplicate package IDs
- [x] Detecting dependency cycles
- [x] Producing stable automatic sorting results
- [x] Applying and persisting settings
- [x] Constructing the RimWorld configuration path on Windows
- [x] Constructing the RimWorld configuration path on Linux
- [ ] Smoke-testing Steam and RimWorld discovery on actual Windows and Linux installations

## Public project requirements

- [ ] Create the initial Git commit and ensure the worktree is clean.
- [ ] Add a `README.md` with screenshots, features, limitations, and usage instructions.
- [ ] Choose and add a `LICENSE`.
- [ ] Add installation instructions for Windows and Linux.
- [ ] Document where RimMod stores settings and backups.
- [ ] Document how to report bugs and collect useful diagnostics.
- [ ] Add package metadata to `Cargo.toml`.
- [x] Avoid publishing internal notes such as `NOTE.md` and `AGENTS.md` in release packages.
- [x] Pin the Git dependency on `egui-notify` to a reviewed commit.
- [ ] Document why the pinned `egui-notify` fork is required.
- [x] Add Windows and Linux CI build workflows.
- [x] Configure tagged, versioned Windows and Linux release artifacts.
- [ ] Run the CI and tagged-release workflows in GitHub.
- [x] Add a native application icon and versioned window title.

## User-interface polish

- [ ] Document that a CJK font fallback is not bundled for the initial release.
- [x] Make important failures persistent instead of showing only short-lived notifications.
- [ ] Test empty libraries, large libraries, long paths, and unusual characters.
- [x] Test the Run button and clearly report launch failures.
- [ ] Confirm that the application remains usable at common display scaling values.

## Verification commands

Run these before each public build:

```powershell
cargo fmt --check
cargo check --all-targets
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo build --release
cargo package --allow-dirty --no-verify --list
cargo audit
```

Review the output of `cargo package --allow-dirty --no-verify --list` and confirm that only intended public files are included. Install `cargo-audit` before running the final command if it is not already available.

## Results from the 2026-09-01 review

| Check | Result |
| --- | --- |
| Windows release build | Passed |
| Windows application startup | Passed |
| Windows path discovery | Passed |
| Mod loading smoke test | Passed |
| `cargo check --all-targets` | Passed with 11 warnings |
| `cargo test --all-targets` | Passed, but ran 0 tests |
| `cargo fmt --check` | Failed |
| `cargo clippy --all-targets -- -D warnings` | Failed with 20 errors |
| Linux build and smoke test | Not performed |
| Full dependency audit | Not performed; `cargo-audit` was unavailable |

The Windows smoke test discovered the RimWorld installation and configuration paths, identified the game version, and loaded 151 disabled and 21 active mods. The Settings window displayed the discovered paths correctly.

## Suggested release stages

### Private pre-alpha

Suitable for development and carefully supervised testing. This is the current stage.

### Public alpha

Appropriate after both release blockers are fixed, configuration writes are safe, core errors are visible, and safety-focused tests exist.

### Public beta

Appropriate after Windows and Linux builds are tested in CI, documentation and licensing are complete, release artifacts are available, and the full verification suite passes.

## Release decision

Do not publish a build for general users while any item under **Release blockers** remains unresolved.
