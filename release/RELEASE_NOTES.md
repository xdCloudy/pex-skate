Experimental PEX plugin and Fabric client for Skate 3 style skating, powered by SkateGM's portable Rust simulation. This release includes a portable server/client bundle and a Windows helper that converts a Skate 3 Xbox 360 ISO supplied by the user. No game data is included.

### What's included

- Server-authoritative skating physics, tricks, animations, score state, and third-person camera presentation.
- Server-authoritative DMO prop translation and yaw rotation with client-side interpolation between state snapshots.
- Off-center board impacts add bounded yaw rotation around a shared prop pivot.
- Prop acceleration and yaw inertia scale with a capped, volume-derived mass estimate; authored material mass is unavailable.
- Frame-consistent prop rendering with one sampled transform per instance per render frame.
- Fabric client controls, board/skater presentation, and map rendering.
- Portable PEX WebAssembly plugin and Windows ISO conversion/setup helper.
- Keyboard and standard gamepad controls for skating and tricks.
- Gamepad axes and buttons are refreshed through SDL's supported manual update path inside Minecraft's client tick.
- SHA-256 manifests for the downloadable release files.

### Install

See `INSTALL.md` in `PEX-Skate-26.3.zip`. Each player needs the Fabric client and their own converted map data. ISO conversion runs locally; use a legally obtained copy of Skate 3.

### Known limitations

This is an alpha and is not production-ready. It has not been validated in a live PEX server/client session using converted Skate 3 data. Board impacts can translate and yaw-rotate authored district props around a shared collision pivot, damp their motion, and synchronize transforms to clients; clients interpolate the received poses between snapshots. Authored mass and hinged doors are not simulated. NPC skaters, several retail visual effects, and some Skate 3 interactions are not implemented. Multiplayer behavior and long-distance skating still need live gameplay validation.
