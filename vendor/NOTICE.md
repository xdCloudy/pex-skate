# Vendored source and notices

## PEX plugin API

`pumpkin-plugin-api` and `pumpkin-plugin-wit/v0.1` are copied from
[Conman1533/PEX](https://github.com/Conman1533/PEX), commit
`74d1a6ad4b4094f7ac397d6fe3aba5992c660356` (PEX 0.2.0+26.3-26.51).
The API manifest is adjusted to use explicit compatible dependencies. The WIT
files remain byte-for-byte compatible with PEX and define the server plugin
ABI. See `LICENSE-PEX` and the API crate's adjacent MIT/Apache license files.

## SkateGM engine host

`skate-engine/` contains the SkateGM engine, data, networking, and portable
host crates from [the-schwilliam/SkateGM](https://github.com/the-schwilliam/SkateGM),
repository commit `ea59a34ab714498cbfb4ed9f22fb9a26ab07bd08`. SkateGM identifies
its `engine/` sources as derived from the 2010 Rust Rewrite Mashup commit
`65d9e117bdec651146e4fe26a903893b4b86e486` and the
[SK8-ENGINE/skate-3-rust-engine](https://github.com/SK8-ENGINE/skate-3-rust-engine)
commit `cb79689`. The files retain their upstream Apache-2.0 license and
Mashup attribution in `skate-engine/LICENSE` and `skate-engine/NOTICE-mashup`.

Local adapter changes are limited to exposing the board deck transform in the
host's `Pose` so PEX can publish it to its clients. `src/engine.rs` adapts PEX
controls and host pose/scoring data; it does not change the upstream simulation.

## Legacy engine-core snapshot

`vendor/skate-core/` and `LICENSE-SKATE-ENGINE` are a retained source snapshot
from [SK8-ENGINE/skate-3-rust-engine](https://github.com/SK8-ENGINE/skate-3-rust-engine),
commit `2e16697797c8358d16f747a8259354ff2921041a`. This GPL-3.0-only copy was
used in an earlier prototype and is no longer a Cargo dependency; the current
server links the SkateGM host tree above. Its upstream license is retained in
`LICENSE-SKATE-ENGINE`.

## Skate 3 asset converter

`tools/skategm-exporter/` vendors the ISO-aware data converter from SkateGM at
commit `ea59a34ab714498cbfb4ed9f22fb9a26ab07bd08`. It is GPL-3.0 and includes
its upstream exporter README, the full `LICENSE-THIRD-PARTY.md`, component
license files, and our `UPSTREAM.md` provenance note. PEX Skate adds the
Windows picker/install scripts under `tools/`; the portable Windows release
converter is built from the vendored source at release time.

## Game content

No Skate 3 game code, assets, or converted data are included. The engine reads
data converted from a copy supplied by the user. Do not add converted game
content to this repository or its release archives.
