# PEX Skate

**Experimental multiplayer Skate 3 simulation for PEX (Pumpkin Experimental Minecraft server).** This repository contains a portable PEX WebAssembly plugin and a Minecraft 26.3 Fabric client companion. The client is required for input, third-person camera behavior, and board presentation.

This is an experimental integration and is **not production-ready**. The PEX server runs SkateGM's portable `skate-host` simulation, including its rigid-body board/skater physics, authored movement/trick graph, animation pose generation, camera state, and Skate 3 score runtime. With converted `.skate` maps installed, `/skate` loads the University district by default (or a filename from `settings/default-map.txt`), starts at its authored spawn, and uses the district collision and grind rails without replacing them with sampled Minecraft collision. Converted district props contribute transformed opaque-mesh triangles to the moving Skate collision layer, with bounded district-wide box fallback when mesh collision exceeds conversion limits. Swept board impacts can translate props and apply bounded yaw rotation after off-center hits, with motion damping over time; the implementation estimates effective prop mass and yaw inertia from transformed collision bounds at uniform density; authored material mass and hinged doors remain unsupported. Prop transforms are shared per world/map and sent to clients, which move the matching instance meshes. The SKATE14 writer can preserve authored point/spot/area lights and NPC routes supplied by a map manifest. The client renders point and spot lights and approximates area lights as point emitters. It also applies each converted district's extracted SKYB sun direction and intensity as directional diffuse lighting. The current ISO extraction pipeline does not yet produce point/spot light or NPC route manifest fields, and SkateGM has no NPC controller; local point/spot lights and NPC skaters remain absent from converted maps. Without a converted map, the server falls back to sampling nearby Minecraft collision; these scans refresh after eight blocks of horizontal travel or three blocks of vertical travel, and simple connected fence/bar/wall blocks are exposed as grind lines. Initial and refresh scans are spread across server ticks, with at most 1,024 block queries per scan and 2,048 total collision block queries across the whole server per tick. A round-robin cursor shares that budget among skaters. The `/skate` command reports while fallback collision is being prepared. The Fabric client receives the authoritative board transform, trick and score state, 36 named engine joints with positions and rotations, and the Skate camera origin/orientation/FOV. When a client has its own converted `skater.glb` at `config/pex-skate/private/skater.glb`, a custom render layer skins the retail character and board from those engine joints; otherwise a mixin retargets the pose onto the player's Minecraft skin model and a separate Minecraft board model is shown. The client also reads the selected `.skate` map and draws its geometry with embedded albedo textures. Opaque, alpha-tested, and blended material modes are rendered; blended primitives are sorted back-to-front. Baked indirect-light textures contribute per-vertex color, and Minecraft world lighting is sampled per chunk. The converter carries each authored district sky dome into the map package and the client renders it around the Skate camera. The client applies authored fog color and distance through Minecraft's atmospheric fog, with an approximation of Skate 3's power curve. Converted normal maps now affect per-vertex diffuse lighting through the exported tangent frame; this is an approximation because the client does not use a per-pixel Skate material shader. Incandescent and unlit retail materials retain their albedo color in low light. Reflection, animated-texture, and water shader effects are not implemented. The camera follows the engine chase-camera pose and clips against Minecraft blocks and opaque/alpha-tested converted district geometry, including currently moved props. The local client shortens the interpolation buffer and extrapolates the authoritative velocity for at most 50 ms, blending back to each incoming snapshot. Remote skaters remain buffered for interpolation. This is presentation smoothing; the client does not replay unacknowledged inputs or predict Skate Engine physics. District rendering, moving props, and the retail skater mesh still need live validation with converted assets and actual clients. Treat multiplayer and long-distance skating as prototype behavior until that validation is complete.

Nearby Skate sessions in the same dimension and converted district now contribute a lightweight body column and oriented board proxy to each other’s Skate collision world. These proxies are refreshed independently of the authored prop mesh. High-speed board-to-rider contacts use a swept check between consecutive authoritative poses before triggering SkateGM’s knockdown response, with a per-pair cooldown to prevent repeated hits while contact continues; shove input keeps its separate explicit knockdown action. This multiplayer collision path is covered by unit tests but has not been validated in a live server/client session, and the simplified contact check is not a full two-player rigid-body solver.

Allocate at least 4 GiB of Java heap for large districts such as University. Parsing the map and its authored prop package together needs over 1 GiB temporarily. If a district references DMO templates absent from the supplied game archive, the converter skips those prop instances and records them in its per-map report.

## What already exists to build on

The key heavy lifting comes from [SkateGM](https://github.com/the-schwilliam/SkateGM), which packages the Rust engine in a small renderer-free `skate-host` crate. That host successfully builds for PEX's `wasm32-wasip2` target, so this mod runs the real simulation instead of recreating skate physics. SkateGM also provides a user-owned Skate 3 asset conversion workflow. The native GMod renderer and multiplayer addon are not used by this project.

PEX's plugin API exposes player custom-payload and movement events, server tick events, player teleportation, and custom payload sending. This project uses those server hooks and a small Fabric client protocol. No Skate 3 assets or game data are included.

The PEX plugin API/WIT and the SkateGM engine/data/host crates are vendored at pinned source revisions. The mod includes no Skate 3 assets or game data. See [vendor/NOTICE.md](vendor/NOTICE.md) for revision and licensing details. Build the plugin with Rust's native `wasm32-wasip2` target; a separately installed componentizer can emit an incompatible export signature.

## Requirements

- PEX/Pumpkin **Minecraft 26.3**, using the plugin API revision pinned in `Cargo.toml`.
- A Java Edition Minecraft 26.3 client with Fabric Loader 0.19.5+, Fabric API 0.162.0+26.3, and Java 25+.
- At least 4 GiB of Java heap for large converted districts such as University.
- Rust stable with the `wasm32-wasip2` target to build the server plugin.
- JDK 25+ to build the client jar.
- A legally obtained Skate 3 Xbox 360 ISO if you plan to use the skating engine. The release's Windows helper converts it locally; no game files are supplied here.

Vanilla and Bedrock clients can join the server, but cannot use Skate mode. The Fabric client advertises its protocol when it connects; `/skate` stays disabled until that handshake succeeds. This first build targets Java/Fabric clients only.

## Build and test

```powershell
rustup target add wasm32-wasip2
cargo test --locked
python -m unittest discover -s tools/skategm-exporter/tests -v
./build.ps1
```

`build.ps1` creates the server/client files and a SHA-256 checksum manifest under `dist/`:

- `PEX-Skate-26.3.zip` — portable bundle with both mods, install guide, and checksums
- `pex-skate.wasm` — portable PEX plugin component
- `pex-skate-client-26.3.jar` — client-side Fabric companion
- `INSTALL.md` — quick install steps for the bundled files
- `SHA256SUMS` — checksums for both files

GitHub releases also include `PEX-Skate-ISO-Setup-Windows.zip`, with the portable ISO converter and picker-based data installer, and `RELEASE-SHA256SUMS`, which covers every file attached to that release, including both ZIPs. Verify a downloaded release asset against that manifest; `SHA256SUMS` inside `PEX-Skate-26.3.zip` separately verifies the WASM and Fabric client JAR. CI builds the Windows setup helper from vendored source. To build the converter locally, use Python 3.13+, Rust, and the pinned requirements in `tools/skategm-exporter/tools/requirements-setup.txt`, then run `tools/skategm-exporter/build.ps1 -Out <folder>`.

To build only the Fabric companion, run `client/gradlew.bat build` from the repository root. The jar is in `client/build/libs/`.

## Skate 3 data setup

### Windows ISO conversion

The release converter is a portable Windows executable. It takes a user-provided Xbox 360 ISO, locally extracts the gameplay inputs and `worldDIST_*.big` district archives, converts them, validates the generated map packages through the Rust gameplay host, then installs the generated files into the selected PEX data folder.

1. Install the PEX plugin on the server and start it once. This creates `<server>/plugins/data/pex-skate/`. Stop the server before installing game data.
2. Download and extract the complete `PEX-Skate-ISO-Setup-Windows.zip` from the [GitHub releases](https://github.com/xdCloudy/pex-skate/releases). Keep `Install-PEXSkateData.bat`, `Install-PEXSkateData.ps1`, and `skategm-convert.exe` together in the extracted folder; the batch file expects the executable beside it.
3. Run `Install-PEXSkateData.bat`. In the file picker, select your legally obtained Skate 3 Xbox 360 `.iso`. Select the PEX plugin data folder itself, normally `<server>/plugins/data/pex-skate/` (not its `private/` subfolder), or cancel for client-only setup. The next picker can target a local Minecraft instance's `config/` folder for its converted skater, map and authored prop packages, plus the map integrity catalog.
4. The converter reads the ISO's XDVDFS filesystem and extracts the gameplay inputs plus the `worldDIST_*.big` district archives into a temporary folder beside the selected output. It converts animation banks, gameplay/state graphs and input settings, physics skeleton data, skater collections, the default skater model and textures, trick-display assets, retail district packages, and authored DMO prop placements and meshes, with transformed opaque-mesh collision triangles and a bounded box fallback. Each map package is checked by parsing it and constructing a Skate host session with its native collision before installation. `maps.json` records map and available prop package paths and SHA-256 digests.
5. After validation, the installer moves `assets/private/` into `<plugin-data>/private/`, `maps/` into `<plugin-data>/maps/`, and writes `maps.json` plus `conversion.log`. Existing private data, maps, and manifest are backed up while the new files are installed, then restored if installation fails. If you selected a client config folder, it can also install the converted skater model, map packages and authored prop packages there after asking before replacement. Temporary ISO extracts and staging files are removed after conversion.
6. Restart the server. The plugin needs the converted data before Skate mode can run; `/skate` reports a readable error if required files are missing.

The conversion runs locally: the ISO and converted game data are not uploaded, committed, or included in the release. The release ZIP includes the corresponding exporter source and third-party notices. The helper currently targets Windows and runs without a separate Python install. Keep enough free disk space for temporary extraction and conversion. If the server is remote, run the helper where its data folder is accessible, use a mounted server directory, or copy the generated `private/` folder to `<server>/plugins/data/pex-skate/` after conversion. If the client model was not installed by the helper, copy `private/skater.glb` to `<minecraft-instance>/config/pex-skate/private/skater.glb` on that client.

This setup exports and installs gameplay, animation, physics, skater, trick-display, retail district map, and authored district prop packages. The PEX runtime loads an installed district's native collision and grind rails for Skate sessions, plus transformed opaque-mesh prop collision from the `PEXCOL3` instance sidecar (or the bounded compatibility/fallback boxes). That sidecar preserves prop IDs and local geometry. The server uses it to build authored collision and shared, server-authoritative translation and yaw responses when a swept board impact hits a prop; transforms are synchronized to clients and the moving collision layer is rebuilt atomically. The response remains an approximation: it estimates effective mass and yaw inertia from transformed collision bounds at uniform density because authored mass is unavailable; hinged doors remain unsupported. The server sends its selected map filename to connected clients, so each Fabric client loads that same package from `config/pex-skate/maps/` and its matching initial-placement props from `config/pex-skate/props/` without matching local selection settings. The helper also installs `config/pex-skate/maps.json`; the client checks the selected map's SHA-256 and, when present, its prop package SHA-256 against this catalog before parsing. Both sides default to `University.skate` when present, then the first map filename alphabetically; `settings/default-map.txt` on the server selects a different district, and the matching client packages are selected automatically. Every client still needs its own converted map and prop files installed locally. Music and sounds are not installed. The converter is based on [SkateGM's exporter](https://github.com/the-schwilliam/SkateGM), pinned and licensed in `tools/skategm-exporter/`.

### Manual conversion from source

For Windows, macOS, or Linux, install Python 3.13+, NumPy, Pillow, and Rust stable. Then, from the repository root, run:

```powershell
python tools/skategm-exporter/convert.py --xex "<path-to-your.iso>" --out "<output-folder>"
```

The `--xex` argument accepts either the ISO or `default.xex` from an already extracted Skate 3 game folder (keep the game's `data/` folder beside `default.xex`). On success, `<output-folder>/assets/private/`, `<output-folder>/maps/`, and `<output-folder>/maps.json` contain the converted data; `<output-folder>/conversion.log` records the conversion. Copy `assets/private/` to `<server>/plugins/data/pex-skate/private/` and `maps/` plus `maps.json` to `<server>/plugins/data/pex-skate/`. For clients, place the converted `maps/` directory and `maps.json` under `<minecraft-instance>/config/pex-skate/` so the client can verify map integrity. See [`tools/skategm-exporter/README.md`](tools/skategm-exporter/README.md) for source setup and the optional native decoder build.

## Install and use

1. Put `dist/pex-skate.wasm` in the plugin directory configured by PEX and start the server once to create the plugin data folder.
2. Install Fabric Loader, Fabric API, and the Fabric client jar on every player client. In CurseForge, open the profile folder and put `pex-skate-client-26.3.jar` directly in its `mods/` folder; do not import the jar with **Add Content**. Fully close and relaunch Minecraft after adding the jar so Fabric scans it at startup.
3. Stop the server, convert the ISO into `plugins/data/pex-skate/` using the steps above, then restart the server.
4. Join the server and press **G** to toggle Skate mode on the client and server. Press **G** again to turn it off.
5. Use **W** or **Space** for a right-foot push, **Shift** for a left-foot push, **S** to brake, and **A/D** to steer. The arrow keys are the right-stick Flick-It directions; customize them under Controls → PEX Skate. A standard mapped gamepad uses **A/X/B** for right-foot push/left-foot push/brake, the left stick for pumping/steering, and the right stick for tricks.

The client keybinds appear in Controls even outside a PEX server. Actual skating and the `/skate` command require a server running the PEX Skate WebAssembly plugin; ordinary single-player worlds do not include it.

The server command `/skate` is also available for server-side toggling. The default camera is switched to third-person while the client mode is active.

The server physics difficulty can be selected for all new sessions with `<server>/plugins/data/pex-skate/settings/difficulty.txt`. Set it to `easy`, `normal`, or `hardcore` (case-insensitive); if the file is absent, the engine's Easy mode is used. Toggle Skate mode off and on again to apply a changed setting.

## Controls

PEX Skate uses Skate 3's push, brake, steering, grabs, board dismount, and Flick-It trick inputs. The keyboard reference below follows [SkateGM's documented keyboard layout](https://github.com/the-schwilliam/SkateGM#keyboard); the status column shows which actions this PEX port currently sends to the Skate simulation. PEX Skate polls the first controller SDL3 recognizes as a standard gamepad and reads its standard face buttons, sticks, triggers, and D-pad. Xbox-style labels are used below; PlayStation and other controllers use the corresponding SDL-mapped positions.

| Action | SkateGM keyboard | Xbox-style gamepad | PEX Skate status |
| --- | --- | --- | --- |
| Toggle Skate mode | G | — | Supported |
| Right-foot push | W or Space | A | Supported (Space also uses Minecraft's jump binding) |
| Left-foot push | Left Shift | X | Supported |
| Brake | S | B | Supported |
| Steer | A / D | Left stick X | Supported |
| Pump/forward axis | — | Left stick Y | Supported |
| Flick-It tricks | Arrow keys | Right stick | Supported |
| Grab with left / right hand | Q / E | Left / right trigger | Supported |
| Get on/off board | F | Y | Supported |
| Interact | C | Skate 3's in-game interact control | Not implemented |
| Gameplay gestures | I (up), K (down), U (left), O (right) | D-pad | Supported |
| Open menu / start / back | Z / Enter / Backspace (Backspace in menus) | Start / Back | Not implemented |
| Respawn to last safe checkpoint | R | LB + X | Supported |
| Shove / punch | V (PEX default) | RB | Starts the engine shove; knocks down the nearest Skate player in front within 1.5 blocks |
| Exit free camera | L | — | Not implemented |
| Rocket board / use item | Ctrl / Alt | Right-stick click / left-stick click | Not implemented |
| Set/return to marker | Ctrl + K / I (hold I to return) | LB + D-pad down / up (hold up to return) | Supported |
| Minigame host/join | — | LB + D-pad left | Not implemented |
| Spectate / teleport / accept invite | — | LB + D-pad right | Not implemented |
| Skate 3 music | Z + Ctrl/arrows | LB + right stick | Not implemented |
| Settings / map / park editor / replays | WASD to move in map/editor/replays | LB + A / Y / B / RB | Not implemented |

The table combines SkateGM's complete documented keyboard controls and its LB modifier/controller extras with the PEX port's current inputs. The actual Skate 3 controller layout is used for gameplay: A/X/B push or brake, the left stick steers/pumps, the right stick performs Flick-It tricks, triggers grab, Y gets on/off the board, and the D-pad sends gameplay gestures. Hold Ctrl or LB with D-pad down to place a session marker; hold Ctrl/LB with D-pad up to return. Supported keyboard actions can be rebound under Controls → Key Binds → PEX Skate. When the bound keys overlap, Q/E's drop/inventory actions and Shift's sneak action are suppressed only while the matching Skate grab or left-foot push is active. SDL3 currently uses the first connected standard gamepad.

## Protocol

The Fabric client advertises protocol version 8 with a `pexskate:hello` custom payload when it joins; this release requires matching protocol-v8 server and client components. The server acknowledges accepted handshakes on `pexskate:hello_ack`, and the client waits for that acknowledgement before sending Skate controls. The client sends a `pexskate:control` custom payload once per client tick while Skate mode is active. The PEX server advances the engine on server ticks, consuming the latest fresh control packet, so simulation progress does not depend on a movement packet arriving. The v9 control body carries action flags for pushes, brake and dismount, four D-pad directions, left/right trigger pressure, four signed stick axes, and a modifier byte with LB/session-marker, manual-respawn, and shove/punch bits, followed by a big-endian 32-bit control sequence; triggers feed the engine's grab inputs, the D-pad drives Skate 3 gameplay gestures and session-marker actions, and the dismount flag drives the engine's on/off-board toggle. Manual respawn returns to the engine's latest safe checkpoint on a press edge; shove/punch starts the engine's timed shove animation on a press edge and can knock down the nearest Skate player in front within 1.5 blocks in the same Minecraft dimension. Left-stick X steers, and right-stick X/Y drive Skate 3's Flick-It/trick recognition. The parser retains v1 through v8 control layouts for compatibility tests, but a live protocol-v8 session accepts only sequenced v9 controls; stale and replayed sequence numbers are rejected with wraparound-safe ordering. Server state v11 includes the engine root, board position/quaternion, velocity, score values, clean/sketchy flags, UTF-8 trick display name, hips position, 36-joint positions and world-space quantized rotations, camera offset/forward/up/FOV, authored-map state, the active server map filename, SHA-256 digests for the exact district and authored-prop packages used by server physics, the latest control sequence consumed by a simulation step, and revisioned translation and yaw deltas for DMO props moved by board impacts, including their shared collision pivots. Prop transforms are shared per world/map, and the client interpolates matching instance-mesh offsets over the 50 ms snapshot interval. Off-center impacts apply bounded yaw rotation around each prop collision pivot, and props translate and damp; effective mass and yaw inertia are estimated from collision bounds at uniform density; authored mass and hinged doors are not simulated. The client refuses to render a same-named but different map or prop package and applies received DMO offsets to the matching instance meshes. State v4 through v11 remain readable by the client; missing rotations, map selection, package digests, control acknowledgements, and prop updates are defaulted.

The supported keyboard actions, including right/left-foot push, brake and steering, are rebindable under Controls → Key Binds → PEX Skate. Space remains an alternate right-foot push through Minecraft's jump binding. The server clears controls after 250 ms without a fresh client packet, removes player state on disconnect, death, respawn, or a successful dimension change, and sends engine state to clients at no more than 20 updates per second per skater. Without the locally converted mesh, the client uses a simplified six-part retarget of the engine skeleton onto the Minecraft skin. The retail skater uses the engine's weighted skeleton, but detailed facial morphs and some hand/foot details remain unverified in Minecraft.

## Current validation boundary

The Rust unit suite, WebAssembly component build, Fabric client build, and local PEX plugin startup can be checked without proprietary game files. With a user-supplied Skate 3 asset conversion, the PEX runtime smoke path has also been exercised on all ten converted districts: an offline test player authenticated, started `/skate`, and had one v9 control sequence consumed on each map. This does not validate Fabric rendering, controller input, or gameplay feel. A complete gameplay validation still requires a Minecraft 26.3 Fabric client/server session with converted assets. Do not describe a successful build or plugin load as proof of a complete Skate 3 visual port.

CI also downloads the pinned PEX 1.0.0 Linux server, verifies its SHA-256, approves only the plugin's declared `fs.read.data` permission for the disposable test server, then checks that PEX compiles and loads the component, accepts an offline Minecraft 26.3 player, returns the PEX Skate protocol-8 handshake acknowledgement, and completes a status handshake advertising protocol 777. This exercises the actual client-to-server custom payload event and server-to-client payload path; it does not load converted Skate 3 assets or validate gameplay rendering and physics in a Minecraft client.

To exercise a converted server data folder locally, build or download `pex-skate.wasm` and run `bash tools/smoke-test-pex-runtime.sh path/to/pex-skate.wasm path/to/converter-output`. The helper copies the converted private assets and maps into a disposable PEX server, logs in an offline test player, requests `/skate`, and requires an active state plus a v9 control sequence acknowledged by a simulation step. Set `PEX_SKATE_SMOKE_MAPS` to a semicolon-separated list of map filenames to restart PEX and repeat the runtime check for each map while reusing the copied data. Set `PEX_SKATE_SMOKE_KEEP_SERVER=1` to leave the single-map smoke server running after the protocol check so a local Fabric dev client can join for visual QA; press Ctrl+C to stop the server and remove its temporary files. This validates server session startup and input consumption with your converted data; it does not by itself validate Fabric rendering, controller input, or gameplay feel in a live Minecraft client.

## License and credits

PEX Skate is GPL-3.0-only. The server API comes from [PEX](https://github.com/Conman1533/PEX). The linked SkateGM engine host is Apache-2.0 and retains its upstream notices. SkateGM's GPL-3.0 exporter and third-party notices are vendored in `tools/skategm-exporter/`. No EA source code or proprietary game assets are included. See [LICENSE](LICENSE) and [vendor/NOTICE.md](vendor/NOTICE.md).
