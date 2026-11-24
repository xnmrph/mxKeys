//! Hardware profiles and detection for mxKeys.
//!
//! - [`catalog`]: keyboards, controllers and displays the app knows about
//! - [`profile`]: the confirmed [`profile::HardwareProfile`] and the build targets it needs
//! - [`detect`]: pure detection from config files, bootloader info, USB lists and quiz answers

pub mod catalog;
pub mod detect;
pub mod layout;
pub mod profile;

pub use profile::HardwareProfile;
