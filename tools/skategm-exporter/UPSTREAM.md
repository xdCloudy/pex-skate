# Skate 3 ISO converter provenance

This directory vendors SkateGM's exporter from [`the-schwilliam/SkateGM`](https://github.com/the-schwilliam/SkateGM), commit `ea59a34ab714498cbfb4ed9f22fb9a26ab07bd08` (2026-10-13). Its `convert.py` and `xiso.py` read a user-supplied Xbox 360 ISO locally and generate converted Skate 3 data. The executable and game data are not included in this source directory.

The exporter is GPL-3.0. Vendored component notices and licenses are retained in `tools/`; the repository's GPL-3.0 license applies to the adapted installer scripts too. See the project root's `LICENSE` and `LICENSE-THIRD-PARTY.md` for attribution and third-party license details.

PEX Skate invokes the upstream converter with its ISO-to-assets path, then installs the generated `assets` directory into the operator-selected PEX plugin data directory. Conversion occurs on the operator's Windows machine. No game data is uploaded or included in release artifacts.
