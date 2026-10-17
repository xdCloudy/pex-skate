# PEX Skate 26.3 installation

This bundle contains a server component, a client companion, checksums, and the project and vendored-source license notices. Install both mods for the full skating experience.

## Server

1. Install the PEX/Pumpkin server version listed in the project README.
2. Copy `pex-skate.wasm` into the server's configured plugin directory.
3. Start the server once so PEX creates the plugin data directory.
4. Download and extract the separate `PEX-Skate-ISO-Setup-Windows.zip` asset from the GitHub release. Run `Install-PEXSkateData.bat` and select your legally obtained Skate 3 Xbox 360 ISO. Select `<server>/plugins/data/pex-skate/` to install server gameplay data, or cancel that picker for client-only model setup. Retail Skate 3 district maps are optional: the server starts sessions in the existing Minecraft world by default. In the optional next picker, select the local Minecraft instance's `config/` folder to install the skater model at `config/pex-skate/private/skater.glb`; each client needs its own local model. The converter does not upload or bundle game data. The setup ZIP includes the converter's corresponding source and license notices.
5. Restart the server and check its log for plugin load errors.

Optional: set the Skate 3 physics mode for new sessions in `<server>/plugins/data/pex-skate/settings/difficulty.txt` to `easy`, `normal`, or `hardcore` (case-insensitive). The default is `easy`. Toggle Skate mode off and on to apply a changed setting.

## Client

Install Fabric Loader, Fabric API, and `pex-skate-client-26.3.jar` on each Java Edition client. The client version must match the Minecraft version in the filename.

For a CurseForge profile, open the profile's menu and choose **Open Folder**, then put `pex-skate-client-26.3.jar` directly in that profile's `mods/` folder beside `fabric-api-*.jar`. Do not use **Add Content** or **Import** for this standalone jar; those actions expect a CurseForge project or modpack. Fully close Minecraft and relaunch the profile so Fabric scans the new jar at startup. Every player needs the Fabric client mod and Fabric API. Each player also needs their own converted `skater.glb` in `config/pex-skate/private/` to see the Skate 3 character mesh instead of the fallback Minecraft player model. Converted map packages are only needed by clients when the server explicitly selects a retail district.

The PEX Skate keybinds can appear in Controls in any world, but the skating session and `/skate` command require joining a PEX server that has `pex-skate.wasm` installed. An ordinary single-player world does not provide that server plugin and will report `/skate` as an unknown command.

## Play

Join the server and press **G** to toggle Skate mode. Use **W** or **Space** to push, **Shift** for a left-foot push, **S** to brake, **A/D** to steer, **F** to get on/off the board, **Q/E** to grab, and the arrow keys for trick flicks. Standard mapped gamepads use **A** for right-foot push, **X** for left-foot push, **B** to brake, **Y** to get on/off the board, the left stick for movement/steering, and the right stick for trick flicks. Press **G** again to exit. See the repository README for the full keyboard/gamepad mapping and unsupported SkateGM controls.

Use the release's `RELEASE-SHA256SUMS` to verify downloaded release assets (on Linux, run `sha256sum --ignore-missing --check RELEASE-SHA256SUMS` in the folder containing the manifest and downloaded files). The `SHA256SUMS` file inside `PEX-Skate-26.3.zip` separately verifies the server and client files. `LICENSE`, `NOTICE.md`, and the `LICENSE-*`/`NOTICE-*` files preserve the project and vendored-source licenses and attribution. Gameplay remains experimental and requires user-provided converted game data; see the project README for the validation boundary and known limitations.
