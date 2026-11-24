<div align="center">
  <img src="assets/icons/app_icon.png" alt="RimMod logo" width="112" />
  <h1>RimMod</h1>
  <p>A desktop mod manager for RimWorld, written in Rust.</p>
  <p>
    <a href="https://github.com/codebykoi/rimmod/actions/workflows/ci.yml"><img src="https://github.com/codebykoi/rimmod/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
    <a href="https://github.com/codebykoi/rimmod/releases/latest"><img src="https://img.shields.io/github/v/release/codebykoi/rimmod" alt="Latest release" /></a>
    <a href="LICENSE"><img src="https://img.shields.io/github/license/codebykoi/rimmod" alt="License" /></a>
  </p>
</div>

RimMod manages RimWorld mod lists, load order, modpacks, Steam Workshop items, and game logs on Windows and Linux.

## Features

- **Load order management**: Drag-and-drop reordering, multi-select, fuzzy search, undo/redo, live directory watching, and safe mod folder deletion.
- **Dependency-aware sorting**: Sorts active mods using `About.xml` rules (`loadAfter`, `loadBefore`, `forceLoad*`, dependencies, incompatibilities) along with dependencies inferred from `LoadFolders.xml`, XML patches (`MayRequire`, `PatchOperationFindMod`), and `.NET` assembly references. Flags missing dependencies, cycles, duplicates, conflicts, and version mismatches (with [No Version Warning](https://github.com/emipa606/NoVersionWarning) coverage checks).
- **Tags and colors**: Attach custom tags and color labels to mods and filter the list by tag. Stored by package ID so labels survive disabling or reinstalling mods.
- **Modpacks (`.rimpack`)**: Save active mod lists together with local mods, selected mod config files, and saves as local library instances or exportable `.rimpack` archives. Workshop mods are recorded by ID rather than bundled.
- **Steam Workshop integration**: Search and filter Workshop mods by RimWorld version and DLC, manage subscriptions, and download items via SteamCMD.
- **Log viewer and analysis**: Reads RimWorld's `Player.log` (both 1.6/older Unity logs and 1.7 structured logs, current and previous sessions), filters by severity or mod, and groups recurring errors with likely suspect mods and matching evidence.
- **Game launch and updates**: Launches RimWorld directly, including launching with the current in-memory mod list via `-mod=` on RimWorld 1.7+. Checks GitHub Releases for new versions and can replace the binary in place when triggered.

## Download

Prebuilt binaries for Windows (`x86_64`) and Linux (`x86_64`) are available on the [Releases](https://github.com/codebykoi/rimmod/releases) page. Extract the archive and run the binary; no installer is required.

## Configuration

On startup, RimMod attempts to locate your RimWorld installation, Steam Workshop folder, and RimWorld config directory automatically. All paths can be changed in **Settings**.

Optional settings:

- **Steam Web API key**: Required for searching the Workshop and subscribing/unsubscribing inside RimMod. Keys can be created at [steamcommunity.com/dev/apikey](https://steamcommunity.com/dev/apikey) and are stored locally in RimMod's settings.
- **SteamCMD**: Required only if you want to download Workshop mods through SteamCMD. Detected from `PATH` automatically or set manually in Settings.

## Shortcuts

Press `?` in the app to view the full list.

| Input | Action |
| --- | --- |
| `Ctrl+F` / `/` | Focus mod search |
| `Ctrl+S` | Save load order |
| `F5` | Reload mods from disk |
| `Ctrl+Z` | Undo load-order change |
| `Ctrl+Y` / `Ctrl+Shift+Z` | Redo |
| `Ctrl+1` .. `Ctrl+4` | Switch tabs |
| `Esc` | Clear search, then clear selection |
| Double-click | Enable or disable a mod |
| Drag | Reorder or move between active/inactive columns |
| `Ctrl+click` / `Shift+click` | Multi-select or range-select mods |
| Right-click | Open mod context menu |

## Building from source

Requires a stable Rust toolchain (edition 2024).

On Debian/Ubuntu, install the required GUI libraries first:

```bash
sudo apt-get install libgtk-3-dev libxkbcommon-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev
```

Build:

```bash
git clone https://github.com/codebykoi/rimmod.git
cd rimmod
cargo build --release
```

The compiled binary is written to `target/release/rimmod` (`target/release/rimmod.exe` on Windows).

Run the tests with:

```bash
cargo test --all-targets
```

## License

Licensed under the [Apache License 2.0](LICENSE).
