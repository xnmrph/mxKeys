//! ZMK Studio RPC for mxKeys (Phase 10, see `docs/live-sync.md`).
//!
//! - [`framing`]: `SOF … EOF` frames with escaping, as in ZMK's `msg_framing.c`
//! - [`proto`]: the protobuf messages (hand-written prost structs, no `protoc`)
//! - [`client`]: request/response client over any [`client::Transport`]
//! - [`sim`]: a simulated keyboard following the firmware's rules, for tests

pub mod client;
pub mod framing;
pub mod proto;
pub mod sim;

pub use client::{Client, DeviceInfo, Error, Notification, Transport};
