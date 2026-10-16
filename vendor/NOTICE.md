# Vendored PEX plugin API

`pumpkin-plugin-api` and `pumpkin-plugin-wit/v0.1` are copied from
[Conman1533/PEX](https://github.com/Conman1533/PEX), commit
`74d1a6ad4b4094f7ac397d6fe3aba5992c660356` (PEX 0.2.0+26.3-26.51).

The API crate manifest is adjusted from workspace-inherited dependencies to
explicit compatible versions so this mod builds independently. The WIT files
are kept byte-for-byte compatible with PEX because their names and definitions
form the server plugin ABI. Build the plugin with Rust's native
`wasm32-wasip2` target; a separately installed componentizer can emit an
incompatible export signature. `LICENSE-PEX` retains the PEX repository
license. The API crate's MIT and Apache-2.0 licenses are also included next to
its source.
