# RFG Physics Sandbox

A Windows sandbox for exploring destruction from the **original Red Faction Guerrilla**. Built with Rust, Bevy and Avian, it converts data from your own game installation into a playable district where you can swing the sledgehammer, remove supports and watch structures collapse.

> **Original game only. Re-Mars-tered is NOT supported.**
> This project targets the original game's Steam build, version **1.0.2.1** (Steam app **20500**). The remaster uses different formats.

This repository includes the sandbox, extraction tools and research notes. Game assets, game executables, memory dumps and decompiled source are not included. You must supply your own game installation.

## What works

- Mason's character model, animations and weapon models, converted from your installation.
- Sledgehammer hits and a structural destruction system based on recovered game rules.
- A district of converted building chunks, loaded as you move around.
- A Buildings panel for teleporting between lots and running support-removal experiments.
- Pause, single-step and reset controls for inspecting the simulation.

This is an experimental reconstruction. Root detection and collision shapes are approximations, and some game behavior is still missing. **Only the sledgehammer has gameplay implemented**; the rifle, remote charge and grenade tool slots are placeholders. See [destruction fidelity and limitations](Docs/destruction_reference.md) for details.

## Requirements

- Windows.
- Your own installation of **Red Faction Guerrilla, original Steam build 1.0.2.1**.
- [Rust](https://rustup.rs). Complete any Windows C++ build tools setup requested by the installer. The repository selects Rust **1.95.0** automatically.
- [Python 3.9 or newer](https://www.python.org/downloads/windows/). Enable the Python launcher or add Python to PATH during installation.
- Internet access for the first setup, to download the Rust toolchain, Rust dependencies and Python packages.
- About **3 GB for extracted and converted game data**, plus additional space for Rust, dependencies and build output.

## Get started

### 1. Download the project

Clone this repository, or download its ZIP from GitHub and extract it. Open the project folder containing `ExtractAssets.bat` and `Play.bat`.

### 2. Extract your game assets

Double-click **ExtractAssets.bat**, or run it from a terminal in the project folder:

```bat
ExtractAssets.bat
```

The extractor looks for the game in your Steam libraries. If it cannot find it, a Windows folder picker opens. Select the original game's install folder containing **RFG.exe** and **build\pc\cache**. Cancelling stops extraction.

The script checks the executable's SHA256, creates a Python environment in `LocalData\venv`, installs NumPy, Pillow and SciPy, and writes converted assets into `LocalData\Assets`. Your game installation is read as input.

Full extraction takes roughly an hour on the reference machine; most of that time converts around 680 chunks. You can stop and rerun it. Completed outputs are reused, incomplete conversions are retried, and a failed conversion makes the command fail rather than report success.

### 3. Build and play

For normal play, use the optimized build:

```bat
Play.bat release
```

The first build can take a while. Later runs reuse the compiled dependencies. Double-clicking `Play.bat` builds the development profile, which is useful while changing code but slower with many buildings.

To start near a specific lot:

```bat
Play.bat release 0203house01_a
```

## Controls

Click the scene to capture the mouse. Press **Esc** to toggle between camera control and a free cursor for the panels. Alt+Tab also frees the cursor.

| Input | Action |
|---|---|
| W / A / S / D | Move |
| Mouse movement | Orbit the camera while the cursor is captured |
| Left Shift | Sprint |
| Space | Jump |
| Left mouse button | Primary hammer swing |
| Right mouse button | Alternate hammer swing |
| 1 | Select sledgehammer |
| 2 / 3 / 4 | Select rifle / remote charge / grenade placeholder slots |
| [ / ] | Cycle the displayed weapon model |
| P | Pause or resume |
| Enter | Resume |
| N | Advance one simulation tick while paused |
| R | Reset the simulation |

Cycling the displayed weapon model does not implement that weapon's gameplay.

### Try a demolition

1. Press **Esc** to free the cursor.
2. In the **Buildings** panel, filter the lot list or select a building. Selecting it teleports you nearby.
3. Swing the hammer manually, or use **Support ring**, **One side**, or **All supports at once** to run a demolition experiment.
4. Use **Stop** to stop an experiment, and **P** / **N** to pause and inspect it one tick at a time.

## Extraction options

Run these commands from the project folder. If you supply a game path, put it before the options and quote paths containing spaces.

| Option | Effect |
|---|---|
| `--chunks NAME ...` | Convert only the named building chunks; character, weapon and table steps still run |
| `--force` | Re-extract and rebuild the selected steps instead of reusing completed outputs |
| `--only STEP,STEP` | Run only the named pipeline steps; prerequisites must already exist |
| `--clean` | Remove intermediate extracted files after a successful full extraction |

Examples:

```bat
REM Supply the game folder explicitly
ExtractAssets.bat "D:\SteamLibrary\steamapps\common\Red Faction Guerrilla"

REM Build one lot for a quicker first try
ExtractAssets.bat --chunks 0203house01_a
Play.bat release 0203house01_a

REM Rebuild a converted lot using previously extracted data
ExtractAssets.bat --only build --chunks 0203house01_a --force

REM Extract everything, then remove intermediate game data
ExtractAssets.bat --clean
```

Valid step names, in order: `misc`, `items`, `anims`, `player`, `chunks`, `assets`, `tables`, `build`.

`--only` reports completion of the selected steps, not readiness of the whole sandbox. Run `ExtractAssets.bat` without `--only` to finish setup. `--clean` cannot be combined with `--only` or `--chunks`; the extractor keeps intermediates until a full conversion succeeds. A later extraction recreates any intermediates removed by `--clean`.

The selected game folder is remembered in `LocalData\game_dir.txt`. Pass another folder explicitly to change it.

## Troubleshooting

**The executable is not the supported build.** Confirm you selected the original game, not Re-Mars-tered. The supported `RFG.exe` SHA256 is:

```text
0606BBED95481EE6D54B5A926ADACCAA29DB6E2E42E2FFF09FE9DFD25946C72D
```

**Python or cargo was not found.** Finish installing Python/Rust and open a new terminal, then retry. Double-clicked scripts also check Rust's default installation directory.

**Extraction failed or assets are incomplete.** Read the first `FAILED` message or error above the summary, correct the problem, and run the command again. Successful chunk conversions remain available. Use `--force` when you need to regenerate existing outputs.

**The scene is slow.** Use `Play.bat release`. Reduce the stream radius in the Buildings panel to keep fewer buildings loaded.

**The project moved to another folder.** Rebuild it with `Play.bat`; some asset paths are baked in at compile time. Move `LocalData` with the project if you want to retain extracted assets.

## Project layout

| Path | Contents |
|---|---|
| `Sandbox/` | Bevy application, character, tools, UI and building simulation |
| `Crates/destruction/` | Structural stress, damage and island splitting, independent of Bevy |
| `Crates/physics/` | Reserved rigid-body backend boundary |
| `Extractors/` | Archive readers, asset converters, validation and Windows folder picker |
| `Research/` | Findings, evidence IDs and player asset inventory |
| `Docs/` | Destruction pipeline and fidelity notes |
| `ReverseEngineering/` | Function addresses, proposed names, call graph and structure layouts |
| `Tests/` | Extraction regression tests using synthetic data |
| `LocalData/` | Generated assets, intermediate extraction, Python environment and Rust builds; ignored by Git |

## Development and checks

```bat
cargo check --locked -p sandbox
cargo test --locked -p rfg_destruction -p rfg_physics
python -m unittest discover -s Tests -v
```

The Python regression tests require NumPy, Pillow and SciPy for converter tests. After extraction setup, you can use `LocalData\venv\Scripts\python.exe` instead of `python`.

The Rust tests run without game assets. The building-data test skips when local assets are absent; it does not replace gameplay validation against the original game.

To check converted assets without launching the sandbox:

```bat
LocalData\venv\Scripts\python.exe Extractors\asset_status.py
```

Research references: [destruction status](Docs/destruction_reference.md), [evidence registry](Research/evidence_registry.md), [player asset inventory](Research/player_asset_inventory.md), and [reverse-engineering reference](ReverseEngineering/README.md).

## License and attribution

Original project code is available under the [MIT License](LICENSE). This license does not grant rights to Red Faction Guerrilla assets, binaries, trademarks or other third-party material. Extracted game data remains local and is excluded from this repository.

This is a fan research project, not affiliated with or endorsed by Volition or THQ Nordic. Red Faction belongs to its respective rights holders.
