# PEX Skate

**Experimental multiplayer Skate 3 simulation for PEX (Pumpkin Experimental Minecraft server).** This repository contains a portable PEX WebAssembly plugin and a Minecraft 26.3 Fabric client companion. The client is required for input, third-person camera behavior, and board presentation.

This is an experimental integration and is **not production-ready**. The PEX server runs SkateGM's portable `skate-host` simulation, including its rigid-body board/skater physics, authored movement/trick graph, animation pose generation, camera state, and Skate 3 score runtime. Minecraft collision is sampled around the player and supplied to the engine; the sample refreshes after eight blocks of travel, and simple connected fence/bar/wall blocks are exposed as grind lines. The Fabric client receives the authoritative board transform, trick and score state, 36 named engine joints, and the Skate camera origin/orientation/FOV. A client mixin retargets the engine hips, spine, head, arms, and legs onto the player's Minecraft skin model; the board is rendered separately. The camera follows the engine chase-camera pose and clips against Minecraft blocks. It does **not** yet load a Skate 3 skater mesh. Collision queries run synchronously in the server movement event, and Minecraft movement/prediction is not reconciled with the simulation. Treat multiplayer and long-distance skating as prototype behavior until tested with converted data and actual clients.

## What already exists to build on

The key heavy lifting comes from [SkateGM](https://github.com/the-schwilliam/SkateGM), which packages the Rust engine in a small renderer-free `skate-host` crate. That host successfully builds for PEX's `wasm32-wasip2` target, so this mod runs the real simulation instead of recreating skate physics. SkateGM also provides a user-owned Skate 3 asset conversion workflow. The native GMod renderer and multiplayer addon are not used by this project.

PEX's plugin API exposes player custom-payload events, player movement events, and custom payload sending. This project uses those server hooks and a small Fabric client protocol. No Skate 3 assets or game data are included.

The PEX plugin API/WIT and the SkateGM engine/data/host crates are vendored at pinned source revisions. The mod includes no Skate 3 assets or game data. See [vendor/NOTICE.md](vendor/NOTICE.md) for revision and licensing details. Build the plugin with Rust's native `wasm32-wasip2` target; a separately installed componentizer can emit an incompatible export signature.

## Requirements

- PEX/Pumpkin **Minecraft 26.3**, using the plugin API revision pinned in `Cargo.toml`.
- A Java Edition Minecraft 26.3 client with Fabric Loader 0.19.5+, Fabric API 0.162.0+26.3, and Java 25+.
- Rust stable with the `wasm32-wasip2` target to build the server plugin.
- JDK 25+ to build the client jar.
- A legally obtained Skate 3 Xbox 360 ISO if you plan to use the skating engine. The release's Windows helper converts it locally; no game files are supplied here.

Vanilla and Bedrock clients can join the server, but cannot use Skate mode. This first build targets Java/Fabric clients only.

## Build and test

```powershell
rustup target add wasm32-wasip2
cargo test --locked
./build.ps1
```

`build.ps1` creates the server/client files and a SHA-256 checksum manifest under `dist/`:

- `PEX-Skate-26.3.zip` — portable bundle with both mods, install guide, and checksums
- `pex-skate.wasm` — portable PEX plugin component
- `pex-skate-client-26.3.jar` — client-side Fabric companion
- `INSTALL.md` — quick install steps for the bundled files
- `SHA256SUMS` — checksums for both files

GitHub releases also include `PEX-Skate-ISO-Setup-Windows.zip`, with the portable ISO converter and picker-based data installer. CI builds it from the vendored source. To build the converter locally, use Python 3.13+, Rust, and the pinned requirements in `tools/skategm-exporter/tools/requirements-setup.txt`, then run `tools/skategm-exporter/build.ps1 -Out <folder>`.

To build only the Fabric companion, run `client/gradlew.bat build` from the repository root. The jar is in `client/build/libs/`.

## Skate 3 data setup

### One-click ISO conversion on Windows

1. Install the PEX plugin on the server and start it once so the `plugins/data/pex-skate/` folder is created. Stop the server.
2. Download and extract `PEX-Skate-ISO-Setup-Windows.zip` from the [GitHub releases](https://github.com/xdCloudy/pex-skate/releases).
3. Run `Install-PEXSkateData.bat`. Select your legally obtained Skate 3 Xbox 360 `.iso`, then select `<server>/plugins/data/pex-skate/`. If the server is remote, run the helper on a machine with access to that folder, use a mounted server directory, or copy the generated `private/` folder to the server afterward.
4. The bundled converter reads the needed files directly from the ISO and converts them locally. It checks required outputs before installing the generated `private/` asset tree. If converted assets already exist, it asks before replacing them. Other files in the selected folder are kept, and `conversion.log` is written for troubleshooting.
5. Start or restart the server. The plugin data root should now include `private/game.json`, `private/stock/skater-collections.json`, `private/stock/data/config/input.cfg`, and `private/skater.glb`. `/skate` reports a readable error if required data is missing.

The ISO, extracted temporary files, and converted game data stay on your machine. Nothing is uploaded or included in GitHub releases. The setup tool is based on [SkateGM's ISO-aware exporter](https://github.com/the-schwilliam/SkateGM), pinned and licensed in `tools/skategm-exporter/`; its standalone `.exe` runs without a separate Python installation. The release setup ZIP includes the corresponding exporter source. This setup currently targets Windows; Linux/macOS users can follow `tools/skategm-exporter/README.md` and install its Python requirements.

The converter reads `default.xex`, animation files, `miscload.big`, `miscboot.big`, `db.big`, and `createacharacter.big` directly from the ISO; it also reads the trick-display archives when present. It copies only those required disc paths into a temporary folder, converts the gameplay graphs/input settings, animation banks, physics skeleton, skater collections, and default skater model, then deletes the temporary extraction. It does not install Skate 3 maps, music, or sounds. The helper needs access to the PEX data folder: run it on the server machine, use a mounted server directory, or copy the generated `private/` folder to a remote server after conversion.

For a manual conversion on Windows, macOS, or Linux, install Python 3.13+, NumPy, and Pillow, then run `python tools/skategm-exporter/convert.py --xex <path-to-your.iso> --out <output-folder>`. On success, `<output-folder>/assets/private/` contains the generated game data and `conversion.log` records the conversion. Copy that `private/` directory into the PEX Skate plugin data folder. The source exporter also accepts `default.xex` from an already extracted game folder. See [`tools/skategm-exporter/README.md`](tools/skategm-exporter/README.md) for requirements and the optional native decoder build.

## Install and use

1. Put `dist/pex-skate.wasm` in the plugin directory configured by PEX and start the server once to create the plugin data folder.
2. Install the Fabric client jar and Fabric API on every player client.
3. Stop the server, convert the ISO into `plugins/data/pex-skate/` using the steps above, then restart the server.
4. Join the server and press **G** to toggle Skate mode on the client and server. Press **G** again to turn it off.
5. Use **W** or **Space** for a right-foot push, **Shift** for a left-foot push, **S** to brake, and **A/D** to steer. The arrow keys are the right-stick Flick-It directions; customize them under Controls → PEX Skate. A standard mapped gamepad uses **A/X/B** for right-foot push/left-foot push/brake, the left stick for pumping/steering, and the right stick for tricks.

The server command `/skate` is also available for server-side toggling. The default camera is switched to third-person while the client mode is active.

## Controls

PEX Skate uses Skate 3's push, brake, steering, and Flick-It trick inputs. Keyboard mappings follow [SkateGM's documented keyboard layout](https://github.com/the-schwilliam/SkateGM#keyboard) where the PEX port supports the action. A standard gamepad recognized by SDL3 is polled directly; for controllers SDL3 does not recognize, use a mapping/remapper that exposes a standard gamepad layout.

| Action | Keyboard | Standard gamepad | PEX Skate status |
| --- | --- | --- | --- |
| Toggle Skate mode | G | — | Supported |
| Right-foot push | W or Space | A | Supported |
| Left-foot push | Shift | X | Supported |
| Brake | S | B | Supported |
| Steer | A / D | Left stick X | Supported |
| Pump/forward axis | — | Left stick Y | Supported |
| Flick-It tricks | Arrow keys | Right stick | Supported |
| Grab | Q / E | Left / right trigger | Controller triggers supported; keyboard grab bindings pending |
| Get on/off board | F | — | Not implemented in PEX Skate |
| Interact | C | — | Not implemented in PEX Skate |
| D-pad | I / K / U / O | D-pad | Not implemented in PEX Skate |
| Menu / start / back | Z / Enter / Backspace | SkateGM menu controls | Not implemented in PEX Skate |
| Respawn / exit free camera | R / L | LB + X respawns | Not implemented in PEX Skate |
| Rocket board / use item | Ctrl / Alt | Right-stick click / left-stick click | Not implemented in PEX Skate |
| Set/return to marker | — | LB + D-pad down / up (hold up to return) | Not implemented in PEX Skate |
| Minigame host/join | — | LB + D-pad left | Not implemented in PEX Skate |
| Spectate / teleport / accept invite | — | LB + D-pad right | Not implemented in PEX Skate |
| Skate 3 music | Z + Ctrl/arrows | LB + right stick | Not implemented in PEX Skate |
| Settings / map / park editor / replays | — | LB + A / Y / B / RB | Not implemented in PEX Skate |

The Minecraft port wires toggle, pushes, brake, steering/pumping, Flick-It trick axes, and controller grab triggers into the simulation. Keyboard grab keys and the additional menus, dismount, interaction, respawn, items, maps, and minigames are not part of this PEX mod yet. The controller mapping uses the first connected device that SDL3 recognizes as a standard gamepad.

## Protocol

The client sends a `pexskate:control` custom payload once per client tick while Skate mode is active. The v3 body carries action flags, left/right trigger pressure, and four signed stick axes; triggers feed the engine's grab inputs, left-stick X steers, and right-stick X/Y drive Skate 3's Flick-It/trick recognition. The server accepts the original v1 and v2 packets for compatibility. Server state v4 includes the engine root, board position/quaternion, velocity, score values, clean/sketchy flags, UTF-8 trick display name, hips position, a 36-joint presence mask and quantized offsets, plus the camera offset, forward/up vectors and field of view. The client interpolates the skeletal and camera snapshots.

The server clears controls after 250 ms without a fresh client packet, removes player state on disconnect, and sends engine state to clients at no more than 20 updates per second per skater. The local Minecraft skin rig uses a simplified six-part retarget of the engine skeleton; the original Skate 3 mesh and detailed facial/hand/foot animation are not loaded.

## Current validation boundary

The Rust unit suite, WebAssembly component build, Fabric client build, and local PEX plugin startup can be checked without proprietary game files. A complete gameplay validation still requires a user-supplied Skate 3 asset conversion and a Minecraft 26.3 Fabric client/server session. Do not describe a successful build or plugin load as proof of a complete Skate 3 visual port.

## License and credits

PEX Skate is GPL-3.0-only. The server API comes from [PEX](https://github.com/Conman1533/PEX). The linked SkateGM engine host is Apache-2.0 and retains its upstream notices. SkateGM's GPL-3.0 exporter and third-party notices are vendored in `tools/skategm-exporter/`. No EA source code or proprietary game assets are included. See [LICENSE](LICENSE) and [vendor/NOTICE.md](vendor/NOTICE.md).
