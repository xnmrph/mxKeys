//! Parsing and lossless editing of ZMK config repositories.
//!
//! - [`dts`]: lossless devicetree lexer + CST (`.keymap`, `.dtsi`, `.overlay`)
//! - [`keymap`]: typed keymap model (layers, bindings, behaviors) on top of the CST
//! - [`behaviors`]: typed hold-taps, macros, combos, … with property-level edits
//! - [`build_yaml`]: `build.yaml` firmware target list
//! - [`catalog`]: keycodes (from ZMK's keys.h) and behaviors with parameter types
//! - [`kconfig`]: lossless `.conf` (Kconfig fragment) model
//! - [`west_yml`]: `config/west.yml` manifest (ZMK revision, extra modules)
//! - [`settings`]: the settings page catalog (plain names, units, defaults) over `.conf`
//! - [`repo`]: locating the files of a zmk-config checkout

pub mod behaviors;
pub mod build_yaml;
pub mod catalog;
pub mod dts;
pub mod kconfig;
pub mod keymap;
pub mod repo;
pub mod settings;
pub mod west_yml;
