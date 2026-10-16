# PEX Skate

**Experimental multiplayer skate-mode bridge for PEX (Pumpkin Experimental Minecraft server).** This repository contains a portable PEX WebAssembly plugin and a Minecraft 26.3 Fabric client companion. The client is required for control input and third-person camera behavior.

This is an early playable foundation, not a complete Skate 3 port yet. It has server-side momentum, steering, braking, pop input, and basic kickflip/shove trick state. Minecraft still owns terrain collision and gravity. The client companion draws a temporary oak pressure-plate board under every active skater so other modded players can see them. It does not yet include a proper skateboard/skater model, Skate 3 animations, grind detection, trick scoring UI, or the original engine's simulation. The Skate 3 Rust Engine is a separate Bevy game and cannot be loaded directly into PEX's sandboxed WASM plugin runtime.

## What already exists to build on

The strongest integration reference is [SkateGM](https://github.com/the-schwilliam/SkateGM): it runs the Skate 3 Rust engine with tricks, scoring, a board, a skater, a chase camera, and multiplayer inside Garry's Mod. Its Rust engine module is native and host-specific, and every player installs a client add-on. It is GPL-3.0 and includes no EA game data. These facts make it an architecture reference rather than a drop-in PEX library.

PEX's plugin API exposes player custom-payload events, player movement events, and custom payload sending. This project uses those server hooks and a small Fabric client protocol. No Skate 3 assets or game data are included.

The PEX plugin API is vendored at the pinned PEX 26.3 commit. The copy makes the workspace dependencies explicit and normalizes numeric WIT identifiers so current `cargo-component` can package the plugin; the declaration order is preserved. See [vendor/NOTICE.md](vendor/NOTICE.md).

## Requirements

- PEX/Pumpkin **Minecraft 26.3**, using the plugin API revision pinned in `Cargo.toml`.
- A Java Edition Minecraft 26.3 client with Fabric Loader 0.19.5+, Fabric API 0.162.0+26.3, and Java 25+.
- Rust stable, `wasm32-wasip2`, and `cargo-component` to build the server plugin.
- JDK 25+ to build the client jar.

Vanilla and Bedrock clients can join the server, but cannot use Skate mode. This first build targets Java/Fabric clients only.

## Build and test

```powershell
rustup target add wasm32-wasip2
cargo install cargo-component
cargo test --locked
./build.ps1
```

`build.ps1` creates both distributables under `dist/`:

- `pex-skate.wasm` — portable PEX plugin component
- `pex-skate-client-26.3.jar` — client-side Fabric companion

To build only the Fabric companion, run `client/gradlew.bat build` from the repository root. The jar is in `client/build/libs/`.

## Install and use

1. Put `dist/pex-skate.wasm` in the plugin directory configured by PEX and start the server.
2. Install the Fabric client jar and Fabric API on every player client.
3. Join the server and press **G** to toggle Skate mode on the client and server. Press **G** again to turn it off.
4. Use the movement keys to steer, **Sprint** while pushing to accelerate, **Shift** to brake, **Space** to pop, **Attack** for a kickflip, and **Use** for a shove-it.

The server command `/skate` is also available for server-side toggling. The default camera is switched to third-person while the client mode is active.

## Protocol

The client sends a `pexskate:control` custom payload once per client tick while Skate mode is active. Its v1 body is four bytes: protocol version, action flags, forward axis, and steer axis. The action flags are push, brake, pop, flip, and shove-it in bits 0 through 4. The PEX plugin validates the payload length/version/flags before using it.

## License and credits

PEX Skate is GPL-3.0-only. The server API comes from [PEX](https://github.com/Conman1533/PEX), also GPL-3.0. The Skate 3 Rust Engine is GPL-3.0-only and is not bundled or linked by this early bridge. [SkateGM](https://github.com/the-schwilliam/SkateGM) is credited as the multiplayer integration reference. No EA source code or proprietary game assets are included. See [LICENSE](LICENSE).
