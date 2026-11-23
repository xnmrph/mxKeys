//! The managed ZMK build toolchain: everything needed to build firmware, downloaded into
//! the app's data directory — no Docker, Nix or system SDK.
//!
//! - [`manifest`]: pinned versions/URLs/checksums (`toolchain/manifest.toml`)
//! - [`download`] + [`extract`]: verified, resumable downloads and archive unpacking

pub mod build;
pub mod classify;
pub mod download;
pub mod extract;
pub mod host;
pub mod install;
pub mod manifest;
pub mod pipeline;
pub mod runner;
pub mod trim;
pub mod uf2;
pub mod workspace;
