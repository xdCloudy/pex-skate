# Skate 3 data exporter

Turns a player's own extracted Skate 3 (Xbox 360) game folder into the data
folder the gm_skategm module reads (`<out>/assets`, holding `private/`): animation
banks, state graphs, input and physics settings, the skater collections and
physics skeletons, and the skater model.

Nothing is downloaded and nothing from the game is included here: it reads the
files you point it at.

## Use

From source (Python 3.13+, `pip install numpy Pillow`):

```
python tools/skategm-exporter/convert.py --xex "D:/Games/Skate 3.iso" --out C:/pex-skate-data
```

`--xex` accepts either a dumped Xbox 360 ISO or `default.xex` beside the game's
`data/` folder. The ISO path reads the required files directly from the disc
image without extracting a full copy first. Progress goes to the console and
the generated `<out>/conversion.log`. For speed, build the native RefPack
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
