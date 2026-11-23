//! Remembering the confirmed hardware profile per config folder, in the app's data
//! directory (`profiles.json`) — not in the user's repo.

use std::collections::BTreeMap;
use std::path::Path;

use mxkeys_hardware::HardwareProfile;

const FILE: &str = "profiles.json";

fn load_all(data_dir: &Path) -> BTreeMap<String, HardwareProfile> {
    std::fs::read_to_string(data_dir.join(FILE))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Stable key for a repo folder (canonical path when possible).
fn key(repo: &Path) -> String {
    std::fs::canonicalize(repo)
        .unwrap_or_else(|_| repo.to_path_buf())
        .display()
        .to_string()
}

pub fn load(data_dir: &Path, repo: &Path) -> Option<HardwareProfile> {
    load_all(data_dir).remove(&key(repo))
}

pub fn save(data_dir: &Path, repo: &Path, profile: HardwareProfile) -> std::io::Result<()> {
    std::fs::create_dir_all(data_dir)?;
    let mut all = load_all(data_dir);
    all.insert(key(repo), profile);
    let json = serde_json::to_string_pretty(&all).map_err(std::io::Error::other)?;
    // Write atomically so a crash never leaves a half-written file.
    let tmp = data_dir.join(format!("{FILE}.tmp"));
    std::fs::write(&tmp, json)?;
    std::fs::rename(tmp, data_dir.join(FILE))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mxkeys_hardware::catalog::DisplayKind;

    #[test]
    fn round_trip_per_repo() {
        let base = std::env::temp_dir().join(format!("mxkeys-profiles-{}", std::process::id()));
        let (data, a, b) = (base.join("data"), base.join("a"), base.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        assert_eq!(load(&data, &a), None);
        let p = HardwareProfile::CORNE_NICE_NANO_V2_NICE_VIEW;
        save(&data, &a, p).unwrap();
        let q = HardwareProfile {
            display: DisplayKind::None,
            ..p
        };
        save(&data, &b, q).unwrap();
        assert_eq!(load(&data, &a), Some(p));
        assert_eq!(load(&data, &b), Some(q));
        std::fs::write(data.join(FILE), "garbage").unwrap();
        assert_eq!(load(&data, &a), None, "corrupt file → start over, don't crash");
        std::fs::remove_dir_all(&base).unwrap();
    }
}
