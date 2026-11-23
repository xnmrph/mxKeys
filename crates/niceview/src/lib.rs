//! Pictures and animations for the nice!view (Phase 9): load a picture or GIF, fit
//! and dither it to the 1-bit screen, encode LVGL images, and generate the small ZMK
//! module that shows them.
//!
//! - [`render`]: decode, fit (cover/contain/stretch), adjust, dither, frame limits
//! - [`lvgl`]: 1-bit `I1` encoding in the display's orientation (checked against ZMK's art)
//! - [`module`]: the files written into the config repo
//! - [`repo`]: saving/removing the pictures in a config repo

pub mod lvgl;
pub mod module;
pub mod render;
pub mod repo;

pub use render::{Area, Dither, Fit, Frame, Options, Source};
