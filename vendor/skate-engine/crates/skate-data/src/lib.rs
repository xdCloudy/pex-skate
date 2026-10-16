//! Runtime stock data, validation and bank storage. Stateless codec math is in core.
#![forbid(unsafe_code)]
mod manifest;
pub use manifest::{AssetError, GameAssets};
pub mod abin;
pub mod animation_banks;
pub mod animation_frames;
pub mod animation_metadata;
pub mod attrib_hash;
pub mod collections;
pub mod gesture_patterns;
pub mod input_config;
pub mod input_recording;
pub mod physics_skeleton;
pub mod scoring;
mod scoring_fields;
mod sha256;

/// SHA-256 digest for exchanging identity of locally installed game data.
pub fn sha256_digest(bytes: &[u8]) -> String {
    sha256::digest(bytes)
}
pub mod retail_collision;
pub mod skate_map;
pub mod state_graph;
