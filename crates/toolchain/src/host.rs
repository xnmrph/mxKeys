//! The machine the app runs on.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Host {
    #[serde(rename = "linux-x86_64")]
    LinuxX86_64,
    #[serde(rename = "linux-aarch64")]
    LinuxAarch64,
    #[serde(rename = "macos-x86_64")]
    MacosX86_64,
    #[serde(rename = "macos-aarch64")]
    MacosAarch64,
    #[serde(rename = "windows-x86_64")]
    WindowsX86_64,
}

impl Host {
    pub const ALL: [Host; 5] = [
        Host::LinuxX86_64,
        Host::LinuxAarch64,
        Host::MacosX86_64,
        Host::MacosAarch64,
        Host::WindowsX86_64,
    ];

    pub fn from_parts(os: &str, arch: &str) -> Option<Host> {
        Some(match (os, arch) {
            ("linux", "x86_64") => Host::LinuxX86_64,
            ("linux", "aarch64") => Host::LinuxAarch64,
            ("macos", "x86_64") => Host::MacosX86_64,
            ("macos", "aarch64") => Host::MacosAarch64,
            ("windows", "x86_64") => Host::WindowsX86_64,
            _ => return None,
        })
    }

    /// This machine, if supported.
    pub fn current() -> Option<Host> {
        Host::from_parts(std::env::consts::OS, std::env::consts::ARCH)
    }

    pub fn is_windows(self) -> bool {
        self == Host::WindowsX86_64
    }

    pub fn label(self) -> &'static str {
        match self {
            Host::LinuxX86_64 => "Linux (x86-64)",
            Host::LinuxAarch64 => "Linux (ARM64)",
            Host::MacosX86_64 => "macOS (Intel)",
            Host::MacosAarch64 => "macOS (Apple silicon)",
            Host::WindowsX86_64 => "Windows (x86-64)",
        }
    }

    pub fn exe(self, name: &str) -> String {
        if self.is_windows() {
            format!("{name}.exe")
        } else {
            name.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapping() {
        assert_eq!(Host::from_parts("linux", "x86_64"), Some(Host::LinuxX86_64));
        assert_eq!(Host::from_parts("windows", "aarch64"), None);
        assert!(Host::current().is_some(), "CI hosts are all supported");
        assert_eq!(Host::WindowsX86_64.exe("cmake"), "cmake.exe");
    }
}
