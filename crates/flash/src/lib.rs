//! Guided UF2 flashing.
//!
//! - [`uf2`]: reading/validating `.uf2` files (magic numbers, family id)
//! - [`verify`]: is this drive / file right for the confirmed controller?
//! - [`copy`]: copying onto the bootloader drive (tolerates the reset-while-closing)
//! - [`session`]: the step-by-step flow as a pure state machine

pub mod copy;
pub mod session;
pub mod uf2;
pub mod verify;
