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
- A legally obtained Skate 3 Xbox 360 copy, converted on your own machine to the SkateGM asset layout. No game files are supplied here.

Vanilla and Bedrock clients can join the server, but cannot use Skate mode. This first build targets Java/Fabric clients only.

## Build and test

```powershell
rustup target add wasm32-wasip2
cargo test --locked
./build.ps1
```

`build.ps1` creates both distributables and a SHA-256 checksum manifest under `dist/`:

- `pex-skate.wasm` — portable PEX plugin component
- `pex-skate-client-26.3.jar` — client-side Fabric companion
- `SHA256SUMS` — checksums for both files

To build only the Fabric companion, run `client/gradlew.bat build` from the repository root. The jar is in `client/build/libs/`.

## Skate 3 data setup

The engine needs converted game data at the PEX plugin's data folder, normally `plugins/data/pex-skate/`. Convert your own Skate 3 copy with the asset conversion workflow documented by [SkateGM](https://github.com/the-schwilliam/SkateGM), then copy the resulting `assets` directory contents into that PEX folder. At minimum, the root must contain `private/game.json`, `private/stock/skater-collections.json`, and `private/stock/data/config/input.cfg`; the engine will report any additional missing files during `/skate` startup. Do not upload converted files to GitHub or include them in a server release. Keep the files readable by the PEX process.

## Install and use

1. Copy your converted Skate 3 data into `plugins/data/pex-skate/` as described above.
2. Put `dist/pex-skate.wasm` in the plugin directory configured by PEX and start the server.
3. Install the Fabric client jar and Fabric API on every player client.
4. Join the server and press **G** to toggle Skate mode on the client and server. Press **G** again to turn it off.
5. Use **W** or **Space** for a right-foot push, **Shift** for a left-foot push, **S** to brake, and **A/D** to steer. The arrow keys are the right-stick Flick-It directions; customize them under Controls → PEX Skate. Skate 3's engine turns those stick flicks into trick and pop logic.

The server command `/skate` is also available for server-side toggling. The default camera is switched to third-person while the client mode is active.

## Protocol

The client sends a `pexskate:control` custom payload once per client tick while Skate mode is active. Its v2 body is six bytes: protocol version, action flags, and four signed stick axes. The flags represent right-foot push, left-foot push, and brake; left-stick X steers and right-stick X/Y drive Skate 3's Flick-It/trick recognition. The server still accepts the original v1 input packet for compatibility. Server state v4 includes the engine root, board position/quaternion, velocity, score values, clean/sketchy flags, UTF-8 trick display name, hips position, a 36-joint presence mask and quantized offsets, plus the camera offset, forward/up vectors and field of view. The client interpolates the skeletal and camera snapshots.

The server clears controls after 250 ms without a fresh client packet, removes player state on disconnect, and sends engine state to clients at no more than 20 updates per second per skater. The local Minecraft skin rig uses a simplified six-part retarget of the engine skeleton; the original Skate 3 mesh and detailed facial/hand/foot animation are not loaded.

## Current validation boundary

The Rust unit suite, WebAssembly component build, Fabric client build, and local PEX plugin startup can be checked without proprietary game files. A complete gameplay validation still requires a user-supplied Skate 3 asset conversion and a Minecraft 26.3 Fabric client/server session. Do not describe a successful build or plugin load as proof of a complete Skate 3 visual port.

## License and credits

PEX Skate is GPL-3.0-only. The server API comes from [PEX](https://github.com/Conman1533/PEX). The linked SkateGM engine host is Apache-2.0 and retains its upstream notices. No EA source code or proprietary game assets are included. See [LICENSE](LICENSE) and [vendor/NOTICE.md](vendor/NOTICE.md).
