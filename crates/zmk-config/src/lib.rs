//! Parsing and lossless editing of ZMK config repositories.
//!
//! - [`dts`]: lossless devicetree lexer + CST (`.keymap`, `.dtsi`, `.overlay`)
//! - [`keymap`]: typed keymap model (layers, bindings, behaviors) on top of the CST
//! - [`build_yaml`]: `build.yaml` firmware target list
//! - [`kconfig`]: lossless `.conf` (Kconfig fragment) model
//! - [`west_yml`]: `config/west.yml` manifest (ZMK revision, extra modules)
//! - [`repo`]: locating the files of a zmk-config checkout

pub mod build_yaml;
pub mod dts;
pub mod kconfig;
pub mod keymap;
pub mod repo;
pub mod west_yml;
