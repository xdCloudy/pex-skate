# PEX Skate

**Experimental multiplayer Skate 3 simulation for PEX (Pumpkin Experimental Minecraft server).** This repository contains a portable PEX WebAssembly plugin and a Minecraft 26.3 Fabric client companion. The client is required for input, third-person camera behavior, and board presentation.

This is an experimental integration and is **not production-ready**. The PEX server runs SkateGM's portable `skate-host` simulation, including its rigid-body board/skater physics, authored movement/trick graph, animation pose generation, camera state, and Skate 3 score runtime. Minecraft collision is sampled around the player and supplied to the engine; the sample refreshes after eight blocks of travel, and simple connected fence/bar/wall blocks are exposed as grind lines. The Fabric client receives the authoritative board transform, trick and score state, 36 named engine joints with positions and rotations, and the Skate camera origin/orientation/FOV. When a client has its own converted `skater.glb` at `config/pex-skate/private/skater.glb`, a custom render layer skins the retail character and board from those engine joints; otherwise a mixin retargets the pose onto the player's Minecraft skin model and a separate Minecraft board model is shown. The camera follows the engine chase-camera pose and clips against Minecraft blocks. Collision queries run synchronously in the server movement event, and Minecraft movement/prediction is not reconciled with the simulation. The retail mesh path has not yet been validated in a live game session with converted assets. Treat multiplayer and long-distance skating as prototype behavior until tested with converted data and actual clients.

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

### Windows ISO conversion

The release converter is a portable Windows executable. It takes a user-provided Xbox 360 ISO, locally extracts only the inputs needed by this PEX port, converts them, validates the result, then installs the generated files into the PEX plugin data folder.

1. Install the PEX plugin on the server and start it once. This creates `<server>/plugins/data/pex-skate/`. Stop the server before installing game data.
2. Download and extract the complete `PEX-Skate-ISO-Setup-Windows.zip` from the [GitHub releases](https://github.com/xdCloudy/pex-skate/releases). Keep `Install-PEXSkateData.bat`, `Install-PEXSkateData.ps1`, and `skategm-convert.exe` together in the extracted folder; the batch file expects the executable beside it.
3. Run `Install-PEXSkateData.bat`. In the file picker, select your legally obtained Skate 3 Xbox 360 `.iso`. Select the PEX plugin data folder itself, normally `<server>/plugins/data/pex-skate/` (not its `private/` subfolder), or cancel for client-only model setup. The next picker can target a local Minecraft instance's `config/` folder so the converted skater model is installed at `config/pex-skate/private/skater.glb` for rendering on that client. Each client that should see the retail character must install its own converted model locally.
4. The converter scans the ISO and extracts the required files into a temporary working folder. It converts animation banks, gameplay/state graphs and input settings, physics skeleton data, skater collections, the default skater model and textures, and available Skate 3 trick-display assets. It then checks for required outputs, including `private/game.json`, `private/stock/physics-skeletons.json`, `private/stock/skater-collections.json`, and `private/skater.glb`.
5. After validation, the installer moves the generated `assets/private/` tree into `<plugin-data>/private/` and writes `conversion.log` when a server data folder was selected. If `private/` already exists, it asks before replacing those converted assets; other files in the plugin data folder are kept. If you selected a client config folder, it also installs the extracted `skater.glb` there after asking before replacement. Temporary ISO extracts are removed after conversion.
6. Restart the server. The plugin needs the converted data before Skate mode can run; `/skate` reports a readable error if required files are missing.

The conversion runs locally: the ISO and converted game data are not uploaded, committed, or included in the release. The release ZIP includes the corresponding exporter source and third-party notices. The helper currently targets Windows and runs without a separate Python install. Keep enough free disk space for temporary extraction and conversion. If the server is remote, run the helper where its data folder is accessible, use a mounted server directory, or copy the generated `private/` folder to `<server>/plugins/data/pex-skate/` after conversion. If the client model was not installed by the helper, copy `private/skater.glb` to `<minecraft-instance>/config/pex-skate/private/skater.glb` on that client.

This PEX setup exports the gameplay, animation, physics, skater, and trick-display data listed above. It does not currently install Skate 3 maps, music, or sounds. The converter is based on [SkateGM's exporter](https://github.com/the-schwilliam/SkateGM), pinned and licensed in `tools/skategm-exporter/`.

### Manual conversion from source

For Windows, macOS, or Linux, install Python 3.13+, NumPy, and Pillow. Then, from the repository root, run:

```powershell
python tools/skategm-exporter/convert.py --xex "<path-to-your.iso>" --out "<output-folder>"
```

The `--xex` argument accepts either the ISO or `default.xex` from an already extracted Skate 3 game folder (keep the game's `data/` folder beside `default.xex`). On success, `<output-folder>/assets/private/` contains the converted data and `<output-folder>/conversion.log` records the conversion. Copy the `private/` directory into `<server>/plugins/data/pex-skate/`. See [`tools/skategm-exporter/README.md`](tools/skategm-exporter/README.md) for source setup and the optional native decoder build.

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
| Grab with left / right hand | Q / E | Left / right trigger | Supported |
| Get on/off board | F | Y | Supported |
| Interact | C | Skate 3's in-game interact control | Not implemented in PEX Skate |
| Gameplay gestures | I (up), K (down), U (left), O (right) | D-pad | Supported |
| Menu / start / back | Z / Enter / Backspace | SkateGM menu controls | Not implemented in PEX Skate |
| Respawn / exit free camera | R / L | LB + X respawns | Not implemented in PEX Skate |
| Rocket board / use item | Ctrl / Alt | Right-stick click / left-stick click | Not implemented in PEX Skate |
| Set/return to marker | — | LB + D-pad down / up (hold up to return) | Not implemented in PEX Skate |
| Minigame host/join | — | LB + D-pad left | Not implemented in PEX Skate |
| Spectate / teleport / accept invite | — | LB + D-pad right | Not implemented in PEX Skate |
| Skate 3 music | Z + Ctrl/arrows | LB + right stick | Not implemented in PEX Skate |
| Settings / map / park editor / replays | — | LB + A / Y / B / RB | Not implemented in PEX Skate |

SkateGM documents the broader original keyboard and controller layout; the table marks which actions this PEX port currently handles. The supported PEX Skate keys can be rebound under Controls → Key Binds → PEX Skate. While skating, Q/E are reserved for grabs and Minecraft's matching drop/inventory actions are suppressed. For a gamepad, SDL3 reads the first connected standard gamepad. The Minecraft port currently sends Skate mode toggle, pushes, brake, steering/pumping, Flick-It trick axes, grab triggers, and D-pad gesture inputs to the simulation. The remaining SkateGM actions are reference mappings only and are not implemented in PEX Skate yet.

## Protocol

The client sends a `pexskate:control` custom payload once per client tick while Skate mode is active. The v5 body carries action flags for pushes, brake and dismount, four D-pad directions, left/right trigger pressure, and four signed stick axes; triggers feed the engine's grab inputs, the D-pad drives Skate 3 gameplay gestures, and the dismount flag drives the engine's on/off-board toggle. Left-stick X steers, and right-stick X/Y drive Skate 3's Flick-It/trick recognition. The server accepts v1 through v4 packets for compatibility. Server state v5 includes the engine root, board position/quaternion, velocity, score values, clean/sketchy flags, UTF-8 trick display name, hips position, 36-joint positions and world-space quantized rotations, plus the camera offset, forward/up vectors and field of view. State v4 remains readable by the client; missing rotations are treated as identity. The client interpolates the skeletal, rotation and camera snapshots.

The server clears controls after 250 ms without a fresh client packet, removes player state on disconnect, and sends engine state to clients at no more than 20 updates per second per skater. Without the locally converted mesh, the client uses a simplified six-part retarget of the engine skeleton onto the Minecraft skin. The retail skater uses the engine's weighted skeleton, but detailed facial morphs and some hand/foot details remain unverified in Minecraft.

## Current validation boundary

The Rust unit suite, WebAssembly component build, Fabric client build, and local PEX plugin startup can be checked without proprietary game files. A complete gameplay validation still requires a user-supplied Skate 3 asset conversion and a Minecraft 26.3 Fabric client/server session. Do not describe a successful build or plugin load as proof of a complete Skate 3 visual port.

CI also downloads the pinned PEX 1.0.0 Linux server, verifies its SHA-256, approves only the plugin's declared `fs.read.data` permission for the disposable test server, then checks that PEX compiles and loads the component and listens on its Minecraft port. That smoke test validates the server plugin boundary; it does not simulate a player or validate converted-game-data gameplay.

## License and credits

PEX Skate is GPL-3.0-only. The server API comes from [PEX](https://github.com/Conman1533/PEX). The linked SkateGM engine host is Apache-2.0 and retains its upstream notices. SkateGM's GPL-3.0 exporter and third-party notices are vendored in `tools/skategm-exporter/`. No EA source code or proprietary game assets are included. See [LICENSE](LICENSE) and [vendor/NOTICE.md](vendor/NOTICE.md).
