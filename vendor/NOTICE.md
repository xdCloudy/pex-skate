# Vendored PEX plugin API

`pumpkin-plugin-api` and `pumpkin-plugin-wit/v0.1` are copied from
[Conman1533/PEX](https://github.com/Conman1533/PEX), commit
`74d1a6ad4b4094f7ac397d6fe3aba5992c660356` (PEX 0.2.0+26.3-26.51).

The API crate manifest is adjusted from workspace-inherited dependencies to
explicit compatible versions so this mod builds independently. In
`pumpkin-plugin-wit/v0.1`, identifiers with numeric hyphen-separated segments
are renamed to valid WIT kebab-case names; declaration order and variant
indices are unchanged. This is needed because the upstream encoded world
contains names rejected by the current `cargo-component`/WIT parser. The plugin
does not use those affected APIs. `LICENSE-PEX` retains the PEX repository
license. The API crate's MIT and Apache-2.0 licenses are also included next to
its source.
