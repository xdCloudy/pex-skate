Experimental PEX plugin and Fabric client for Skate 3 style skating, powered by SkateGM's portable Rust simulation. This release includes a portable server/client bundle and a Windows helper that converts a Skate 3 Xbox 360 ISO supplied by the user. No game data is included.

### 0.1.0-alpha.82

- Correct the retail skater GLB exporter to build the four duplicate toe and hand joints from their matching source bones, instead of reusing left finger-joint transforms that tore the rendered mesh.
- Add a regression test for the toe/hand source-bone mapping.

### 0.1.0-alpha.81

- Add half of the 20 Hz server snapshot interval to local RTT prediction so the chase camera and skater compensate for snapshot age even on low-latency connections.
- Keep remote skaters unpredicted and cap local prediction to the existing 100 ms safety bound.

### 0.1.0-alpha.80

- Keep network-authoritative skaters in the render pass while the custom Skate camera is active, even when Minecraft's camera frustum rejects their entity bounds.
- Preserve the existing finite-vector guard for Minecraft's frustum offset loop.

### 0.1.0-alpha.79

- Log the first Minecraft frustum direction and abort camera-cube offsetting when the vector is invalid or cannot advance along the method's Z axis.
- Add regression coverage for non-finite and near-zero frustum direction vectors.

### 0.1.0-alpha.78

- Guard Minecraft's camera culling loop when it receives a non-finite view vector, which otherwise can hang the render thread while offsetting the frustum.
- Retain finite-pose checks for custom chase-camera updates and expand camera regression coverage.

### 0.1.0-alpha.77

- Guard both Minecraft-world and retail-district camera updates against non-finite pose vectors and rotations before updating Minecraft's camera frustum.
- Add camera math regression coverage for non-finite positions and rotations.

### 0.1.0-alpha.76

- Fix fallback skateboard model baking on Minecraft 26.3 by using a single item atlas and explicit in-bounds face UVs.
- Add resource regression coverage for the board model's texture atlas and UV bounds.

### 0.1.0-alpha.75

- Align the fallback Minecraft skater's torso heading with the chase camera's sampled skeletal heading so independent board trick rotation does not turn the avatar away from the camera.
- Add camera/avatar heading regression coverage.

### 0.1.0-alpha.74

- Give Space a dedicated PEX right-foot push binding and suppress Minecraft jump only while Skate mode is active, preventing a vanilla jump from competing with Skate physics.
- Keep W and gamepad A as alternate right-foot push inputs; vanilla jump works normally outside Skate mode.
- Add regression coverage and update the control table and install guide.

### 0.1.0-alpha.73

- Predict local skeletal joints relative to the same extrapolated simulation root used by the Minecraft-world chase camera and avatar renderer, preventing finite-difference joint velocity from pulling the retail skater model away during acceleration.
- Add regression coverage for root-relative joint prediction under mismatched snapshot and velocity estimates.
- Live camera and retail skater synchronization still needs validation in Minecraft; this remains an alpha build.

### 0.1.0-alpha.72

- Match the local skeletal prediction floor to the server's 50 ms pose broadcast interval to avoid overestimating motion when client packet tasks arrive together.
- Update regression coverage for burst-arrival position and rotation predictions.
- The camera change still needs live visual validation in Minecraft; this remains an alpha build.

### 0.1.0-alpha.71

- Correct shortest-arc quaternion prediction for local skeletal joints when equivalent quaternion signs differ.
- Add a regression test for shortest-arc rotation prediction.
- The camera change still needs live visual validation in Minecraft; this remains an alpha build.

### 0.1.0-alpha.70

- Bound local skeletal pose prediction to a minimum 25 ms sample interval so back-to-back client packet processing cannot amplify timing jitter into camera or skater jumps.
- Add regression tests for close-arrival joint position and rotation snapshots.
- The camera change still needs live visual validation in Minecraft; this remains an alpha build.

### 0.1.0-alpha.69

- Predict local skeletal joint positions and rotations between server snapshots using the same sampled pose as the chase camera. This keeps the visible skater's body heading and joint transforms moving in step with the predicted camera follow during turns and pushes.
- Add regression coverage for joint position prediction, its single-snapshot velocity fallback, and skeletal rotation prediction.
- The camera change still needs live visual validation in Minecraft; this remains an alpha build.

### 0.1.0-alpha.68

### Fixes in this build

- Index converted asset collection and field identities once instead of linearly searching the full collection list for every engine setting. On the same two-client converted-data smoke, the `IW4L_SKATE_LOAD skater` timing fell from 3.85/2.93 seconds to 0.43/0.30 seconds; the updated run emitted no server catch-up warning.
- Print engine asset/session load timings and server catch-up warnings on successful data-backed smoke runs, making startup-cost changes measurable.
- Remove the local 25 ms Skate-pose delay so the local camera and skater follow the newest shared pose sample.
- Apply RTT-derived local pose prediction at packet receipt and advance it with render time, instead of using the RTT only as a post-receipt extrapolation cap.
- Pace the gameplay smoke inputs against server acknowledgements so delayed state packets cannot skip stock Ollie gesture points.
- Strengthen the data-backed runtime smoke to verify movement and a recognized trick with simultaneous clients in Minecraft-world mode.
- Apply the Skate chase camera after Minecraft's camera extraction so vanilla camera setup cannot overwrite the Skate-follow position; synchronize the final render pose, projection, and culling frustum.
- Anchor Minecraft-world chase view and look-at point to the same sampled simulation root used by the rendered player, instead of following animated hip motion; add camera-math regression coverage. Live camera feel still needs in-game validation.
- Prewarm up to four sessions for the selected retail district at plugin startup, with a `0`–`4` setting to trade startup memory/time for faster `/skate` activation. Additional players beyond the pool load sessions on demand.
- Keep OPAQUE GLB materials opaque when source textures carry packed alpha data, preventing sparse or flickering skater and board meshes.
- Decode each skater's interpolated joint positions and rotations once per render submission and reuse that snapshot across the GLB primitives.
- Keep Minecraft-world mode as the default when no retail district is selected, and make the installer describe retail maps as optional.
- Run the PEX runtime smoke with two simultaneous clients in CI; both must activate Skate and have a control sequence consumed.
- Keep the skateboard model's cuboids inside Minecraft's 16×16 model bounds so the item model bakes without out-of-range face UVs.
- Add a regression check covering every skateboard cuboid coordinate.
- Coalesce repeated `/skate` key presses while waiting for the server's state acknowledgement, avoiding duplicate toggles during synchronous district loading.
- Add focused client tests for duplicate suppression, server acknowledgement, retry timeout, and connection reset.
- Keep every skateboard cuboid texture in Minecraft's item atlas so the client can bake the skateboard model without crossing into the block atlas.
- Add a regression test for the skateboard item-atlas contract.
- Register the `/skate` command behind a dedicated permission granted to players by default. Previously, PEX hid the command from regular players because the permission node was empty.
- Raise the WASM guest stack to 8 MiB so SkateGM's stock physics and animation data can initialize inside the PEX runtime.
- The PEX runtime smoke client now replies to Minecraft 26.3 teleport requests with the accepted position and rotation. It also reports system chat and Skate state payloads when diagnosing gameplay activation.
- The data-backed runtime smoke test timeout can be shortened with `PEX_SKATE_SMOKE_TIMEOUT_SECONDS`.
- The data-backed smoke harness can restart PEX across a semicolon-separated `PEX_SKATE_SMOKE_MAPS` list while reusing one converted data copy.
- The smoke harness can keep its disposable server alive after the protocol check for a local Fabric client visual QA session.

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
- Local skater and chase-camera presentation now predict forward from the measured input round trip at packet receipt, then advance from that snapshot time. Both render from the same predicted pose.
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

This is an alpha and is not production-ready. All 10 districts converted from the supplied Xbox 360 ISO have each passed a PEX runtime smoke test. A separate data-backed smoke drives movement and the stock Ollie gesture for two simultaneous clients in Minecraft-world mode. On the same local two-client run, indexed Skate data lookups reduced engine load timing from 3.85/2.93 seconds to 0.43/0.30 seconds, with no server catch-up warning in the updated run. A four-client University run consumed all four prewarmed sessions; startup took about 7.2 seconds and produced a one-time catch-up warning. Session creation is synchronous during startup prewarming, and players beyond the configured pool load a session on demand; memory and startup performance vary by host. These checks do not validate Fabric rendering, camera feel, controller input, or long-distance gameplay in a real Minecraft client. Some authored DMO template IDs can be absent from the supplied archive and are reported/skipped. Board impacts can translate and yaw-rotate authored district props around a shared collision pivot, damp their motion, and synchronize transforms to clients; clients interpolate the received poses between snapshots. Authored mass and hinged doors are not simulated. NPC skaters, several retail visual effects, and some Skate 3 interactions remain unimplemented.
