# Skate 3 data exporter

Turns a player's own Skate 3 (Xbox 360) ISO or extracted game folder into PEX
data: gameplay and character files under `<out>/assets/private/`, plus retail
district packages under `<out>/maps/` and a `<out>/maps.json` catalog. Map files
use the engine's portable `SKATE14` format and include render geometry, textures,
retail collision, authored grind splines, and the district's client-rendered
retail sky dome when its source assets resolve.

Resolved DMO placements also produce `assets/private/native-props/<Map>.skate`
for client rendering and an `<Map>.instances` collision sidecar in the same
directory. The `PEXCOL3` sidecar stores each
instance ID, its authored transform, and its local opaque-mesh triangles; the
server expands these into the initial collision layer. It preserves instance
boundaries for a future movement protocol, but the current runtime does not
simulate prop movement or impact response. `<Map>.collision` remains as a
bounded compatibility/fallback package. `maps.json` records and hashes each
available package.

The ISO mode reads the required files and `data/content/worldDIST_*.big` map
archives from the disc image into a temporary folder, converts them, then removes
that temporary extraction. The XDVDFS
reader checks image extents, rejects unsafe filenames, and bounds directory
size/depth before extracting. Nothing is downloaded by the exporter and no game
files are included here.

## Use

From source (Python 3.13+, NumPy, Pillow, and Rust stable on `PATH`):

```
python tools/skategm-exporter/convert.py --xex "D:/Games/Skate 3.iso" --out C:/pex-skate-data
```

`--xex` accepts either a dumped Xbox 360 ISO or `default.xex` beside the game's
`data/` folder. The ISO path extracts only the required disc paths and world
archives into a temporary folder, not a full disc copy. Copy `<out>/assets/private/`
to `<plugin-data>/private/`, then copy `<out>/maps/` and `maps.json` into the PEX
plugin data folder. Progress goes to the console and `<out>/conversion.log`.
The converter builds `pex-skate-map-check` and constructs a gameplay session for
each generated district to validate its map, retail collision and rails. For
speed, build the native RefPack
decoder first (otherwise a slower Python decoder is used):

```
rustc --edition 2024 --crate-type cdylib -C opt-level=3 -C panic=abort tools/skategm-exporter/tools/asset_pipeline/refpack_native.rs -o tools/skategm-exporter/tools/asset_pipeline/refpack.dll
```

As a single `skategm-convert.exe` (needs PyInstaller, NumPy, Pillow and rustc):

```
pwsh tools/skategm-exporter/build.ps1 -Out <folder>
```

## Where it comes from

- `tools/` is the converter tree of
  [SK8-ENGINE/skate-3-rust-engine](https://github.com/SK8-ENGINE/skate-3-rust-engine)
  at commit `cb7968930f14dad38457e98720d1a274e469eec2` (credited in the
  top-level README). It's the same
  selection the upstream converter build makes: Python, JSON, text and
  licence files, without tests, Blender scripts or Mixamo sample data.
  `import_upstream.py <checkout>` repeats the copy.
- Bundled within it, under their own licences: `tools/vendor/utt` (MIT,
  duckyinnit), `tools/vendor/university` and `tools/owned_game` (MIT, Skate 3
  Custom Engine Layer contributors), `tools/vendor/skate3_ui` (MIT),
  `tools/vendor/skate3_anim` (from chasmlol/skate-3-trick-toolkit, see its
  PROVENANCE.md).
- `convert.py` and `build.ps1` are adapted from
  [2010 Rust Rewrite Mashup](https://github.com/chasmlol/2010-rust-rewrite-mashup)'s
  `skate/converter` (Apache-2.0).
