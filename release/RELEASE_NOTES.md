Experimental PEX plugin and Fabric client for Skate 3 style skating, powered by SkateGM's portable Rust simulation. This release includes a portable server/client bundle and a Windows helper that converts a Skate 3 Xbox 360 ISO supplied by the user. No game data is included.

### Fixes in this build

- Register the `/skate` command behind a dedicated permission granted to players by default. Previously, PEX hid the command from regular players because the permission node was empty.
- Raise the WASM guest stack to 8 MiB so SkateGM's stock physics and animation data can initialize inside the PEX runtime.
- The PEX runtime smoke client now replies to Minecraft 26.3 teleport requests with the accepted position and rotation. It also reports system chat and Skate state payloads when diagnosing gameplay activation.
- The data-backed runtime smoke test timeout can be shortened with `PEX_SKATE_SMOKE_TIMEOUT_SECONDS`.

### What's included

- Server-authoritative skating physics, tricks, animations, score state, and third-person camera presentation.
- Server-authoritative DMO prop translation and yaw rotation with client-side interpolation between state snapshots.
- Off-center board impacts add bounded yaw rotation around a shared prop pivot.
- Prop acceleration and yaw inertia scale with a capped, volume-derived mass estimate; authored material mass is unavailable.
- Rust server and Fabric client state-v11 codecs share a golden payload fixture, with byte-for-byte round-trip and legacy v10 prop decoding checks.
- Fabric v9 control encoding now has a shared golden packet decoded by the Rust server path, covering trick axes, triggers, action bits, modifiers and sequence decoding.
- Frame-consistent prop rendering with one sampled transform per instance per render frame.
- Fabric client controls, board/skater presentation, and map rendering.
- Converted normal maps contribute tangent-frame-aware per-vertex diffuse lighting.
- The client map loader has a synthetic SKATE14 package regression fixture covering authored materials, tangent-space normal mapping, and triangle intersection parsing without requiring game assets.
- The ISO converter extracts the optional `worlddmo.big` catalog and exports authored prop packages and collision sidecars when the archive is present.
- The Fabric SKATE14 reader now follows the writer's lightmap/normal-map field order and accommodates the real University district texture set.
- Incandescent and unlit retail materials preserve their base albedo as emissive light.
- The Skate chase camera clips against converted district triangles and moving props as well as Minecraft blocks.
- Camera obstruction queries use a spatial index so static district meshes outside the camera segment are skipped.
- Portable PEX WebAssembly plugin and Windows ISO conversion/setup helper.
- Keyboard and standard gamepad controls for skating and tricks.
- Active Skate key bindings no longer trigger overlapping Minecraft drop, inventory or sneak actions; unrelated rebound Minecraft controls remain available.
- Gamepad axes and buttons are refreshed through SDL's supported manual update path inside Minecraft's client tick.
- SHA-256 manifests for the downloadable release files.
- CI verifies plugin startup and performs the Minecraft 26.3 status handshake against the pinned PEX server.
- The client waits for a protocol-8 server acknowledgement before sending controls; CI verifies the complete offline player handshake and bidirectional custom payloads against PEX 1.0.0.

### Install

See `INSTALL.md` in `PEX-Skate-26.3.zip`. Each player needs the Fabric client and their own converted map data. ISO conversion runs locally; use a legally obtained copy of Skate 3.

### Known limitations

This is an alpha and is not production-ready. The PEX client/server handshake is tested against a live PEX process, and all 10 districts from one Xbox 360 ISO converted and parsed with their available prop packages, but gameplay has not been validated in a live Minecraft session. Some authored DMO template IDs can be absent from the supplied archive and are reported/skipped. Board impacts can translate and yaw-rotate authored district props around a shared collision pivot, damp their motion, and synchronize transforms to clients; clients interpolate the received poses between snapshots. Authored mass and hinged doors are not simulated. NPC skaters, several retail visual effects, and some Skate 3 interactions are not implemented. Multiplayer behavior and long-distance skating still need live gameplay validation.
