//! Live editing over ZMK Studio (Phase 10, see docs/live-sync.md): one connection at a
//! time; every live change is also written to the keymap file (through the editor, so
//! undo works), and nothing stays only on the keyboard.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use mxkeys_studio::ble::{self, BleCandidate};
use mxkeys_studio::live::{Live, LiveStatus};
use mxkeys_studio::mapping::{self, DeviceBehaviors, Mapper};
use mxkeys_studio::serial::{self, SerialCandidate};
use mxkeys_studio::sync::{self, Comparison};
use mxkeys_studio::Transport;
use serde::{Deserialize, Serialize};
use zmk_config::dts::Document;
use zmk_config::keymap::Keymap;
use zmk_config::repo;

use crate::editor::{self, EditOp, EditResult, Editor};

type Session = Live<Box<dyn Transport>>;

#[derive(Default)]
pub struct LiveState {
    session: Mutex<Option<Connected>>,
}

struct Connected {
    live: Session,
    keymap_path: PathBuf,
    via: String,
    /// Keymap text when the keyboard's state was last saved/loaded (for Discard).
    file_at_save: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Via {
    Usb {
        path: String,
    },
    Ble {
        id: String,
    },
    /// Developer/demo mode (`MXKEYS_SIM_KEYBOARD=1`): a simulated keyboard built from
    /// the keymap file itself.
    Sim,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Devices {
    pub usb: Vec<SerialCandidate>,
    /// Offer the simulated keyboard (developer mode).
    pub sim: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveView {
    pub connected: bool,
    pub status: Option<LiveStatus>,
    pub via: Option<String>,
    /// Where the `&studio_unlock` key is (layer title, key number) — shown when locked.
    pub unlock_key: Option<(String, usize)>,
    /// The keyboard has saved live changes from before the last firmware install.
    pub offer_clear: bool,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ---- remembered facts (app data, never the repo) ----

#[derive(Debug, Default, Serialize, Deserialize)]
struct Memory {
    /// keyboard serial → when live changes were last saved on it.
    saved_at: HashMap<String, u64>,
    /// repo path → when firmware was last installed from the app.
    installed_at: HashMap<String, u64>,
}

fn memory_path(data: &Path) -> PathBuf {
    data.join("live.json")
}

fn load_memory(data: &Path) -> Memory {
    std::fs::read_to_string(memory_path(data))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn store_memory(data: &Path, m: &Memory) {
    let _ = std::fs::create_dir_all(data);
    if let Ok(t) = serde_json::to_string_pretty(m) {
        let _ = std::fs::write(memory_path(data), t);
    }
}

/// Called when the flash wizard finishes installing firmware for `repo`.
pub fn record_install(data: &Path, repo_dir: &Path) {
    let mut m = load_memory(data);
    m.installed_at.insert(repo_dir.display().to_string(), now());
    store_memory(data, &m);
}

fn offer_clear(data: &Path, repo_dir: &Path, serial: &str) -> bool {
    let m = load_memory(data);
    match (
        m.saved_at.get(serial),
        m.installed_at.get(&repo_dir.display().to_string()),
    ) {
        (Some(saved), Some(installed)) => installed > saved,
        _ => false,
    }
}

// ---- helpers ----

fn base_mapper(data: &Path, repo_dir: &Path, keymap_path: &Path) -> Result<Mapper, String> {
    let ws = mxkeys_toolchain::pipeline::workspace_for(data, repo_dir).ok_or(
        "Build the firmware once first — mxKeys uses ZMK's files from the build to translate keys for the keyboard.",
    )?;
    let text = repo::read(keymap_path).map_err(|e| e.to_string())?;
    let config_dir = keymap_path.parent().unwrap_or(repo_dir);
    let macros = mapping::macros_for(&text, &mapping::search_paths(&ws.dir, config_dir));
    let names = mapping::behavior_names(&ws.dir, &Document::parse(text));
    Ok(Mapper {
        macros,
        names,
        device: DeviceBehaviors::default(),
    })
}

fn layer_tokens(doc: &Document, km: &Keymap) -> Vec<String> {
    zmk_config::catalog::layer_refs(doc, km)
        .iter()
        .map(|r| r.define.clone().unwrap_or_else(|| r.index.to_string()))
        .collect()
}

fn unlock_key(km: &Keymap) -> Option<(String, usize)> {
    km.layers.iter().find_map(|l| {
        let pos = l
            .bindings
            .as_ref()?
            .bindings
            .iter()
            .position(|b| b.behavior == "&studio_unlock")?;
        Some((l.title(), pos + 1))
    })
}

fn load_keymap(path: &Path) -> Result<(Document, Keymap, String), String> {
    let text = repo::read(path).map_err(|e| e.to_string())?;
    let doc = Document::parse(text.clone());
    let km = Keymap::from_document(&doc).map_err(|e| e.to_string())?;
    Ok((doc, km, text))
}

impl LiveState {
    fn view(&self, data: &Path, repo_dir: &Path) -> Result<LiveView, String> {
        let mut guard = self.session.lock().unwrap();
        let Some(c) = guard.as_mut() else {
            return Ok(LiveView {
                connected: false,
                status: None,
                via: None,
                unlock_key: None,
                offer_clear: false,
            });
        };
        let status = match c.live.poll() {
            Ok(s) => s,
            Err(e) => {
                *guard = None;
                return Err(format!("Lost the connection: {e}"));
            }
        };
        let unlock = load_keymap(&c.keymap_path)
            .ok()
            .and_then(|(_, km, _)| unlock_key(&km));
        Ok(LiveView {
            connected: true,
            offer_clear: offer_clear(data, repo_dir, &status.serial),
            status: Some(status),
            via: Some(c.via.clone()),
            unlock_key: unlock,
        })
    }
}

pub fn sim_enabled() -> bool {
    std::env::var("MXKEYS_SIM_KEYBOARD").is_ok_and(|v| v == "1")
}

pub fn devices() -> Devices {
    Devices {
        usb: serial::find_keyboards(),
        sim: sim_enabled(),
    }
}

/// A simulated keyboard whose firmware matches the keymap file: behaviors from the
/// mapper's names (ids assigned like a fresh settings table), layers from the file.
fn simulated(base: &Mapper, keymap_path: &Path) -> Result<mxkeys_studio::sim::SimKeyboard, String> {
    use mxkeys_studio::sim::{ParamKind, SimBehavior, SimKeyboard, SimLayer};
    let (_, km, _) = load_keymap(keymap_path)?;
    let mut names: Vec<String> = base.names.values().cloned().collect();
    names.sort();
    names.dedup();
    let behaviors: Vec<SimBehavior> = names
        .iter()
        .enumerate()
        .map(|(i, n)| SimBehavior {
            id: i as u32 + 1,
            name: n.clone(),
            param1: ParamKind::HidUsage,
            param2: ParamKind::Nil,
        })
        .collect();
    let mut m = base.clone();
    m.device = DeviceBehaviors::new(behaviors.iter().map(|b| (b.id, b.name.clone())));
    let layers = sync::expected(&km, &m)
        .into_iter()
        .map(|l| SimLayer {
            id: l.index as u32,
            name: l.device_name,
            bindings: l
                .keys
                .into_iter()
                .map(|k| k.binding.unwrap_or_default())
                .collect(),
        })
        .collect();
    let mut kb = SimKeyboard::new("Simulated keyboard", behaviors, layers);
    // "Presses" the unlock key after a few lock-state checks (~1.5 s each);
    // MXKEYS_SIM_UNLOCK_AFTER sets how many (0 = never).
    let after = std::env::var("MXKEYS_SIM_UNLOCK_AFTER")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3);
    kb.auto_unlock_after = (after > 0).then_some(after);
    Ok(kb)
}

pub fn scan_ble(seconds: u64) -> Result<Vec<BleCandidate>, String> {
    ble::scan(std::time::Duration::from_secs(seconds.clamp(1, 15))).map_err(|e| e.to_string())
}

pub fn connect(
    state: &LiveState,
    data: &Path,
    repo_dir: &Path,
    keymap_path: &Path,
    via: &Via,
) -> Result<LiveView, String> {
    let base = base_mapper(data, repo_dir, keymap_path)?;
    let (transport, label): (Box<dyn Transport>, String) = match via {
        Via::Usb { path } => (
            Box::new(serial::SerialTransport::open(path).map_err(|e| e.to_string())?),
            format!("USB ({path})"),
        ),
        Via::Ble { id } => (
            Box::new(ble::BleTransport::connect(id).map_err(|e| e.to_string())?),
            "Bluetooth".to_string(),
        ),
        Via::Sim if sim_enabled() => (Box::new(simulated(&base, keymap_path)?), "simulation".to_string()),
        Via::Sim => return Err("The simulated keyboard is only available in developer mode.".into()),
    };
    let live = Live::connect(transport, base).map_err(|e| e.to_string())?;
    let file_at_save = repo::read(keymap_path).map_err(|e| e.to_string())?;
    *state.session.lock().unwrap() = Some(Connected {
        live,
        keymap_path: keymap_path.to_path_buf(),
        via: label,
        file_at_save,
    });
    state.view(data, repo_dir)
}

/// For tests: connect over any transport (the simulator).
pub fn connect_with(
    state: &LiveState,
    transport: Box<dyn Transport>,
    base: Mapper,
    keymap_path: &Path,
) -> Result<(), String> {
    let live = Live::connect(transport, base).map_err(|e| e.to_string())?;
    let file_at_save = repo::read(keymap_path).map_err(|e| e.to_string())?;
    *state.session.lock().unwrap() = Some(Connected {
        live,
        keymap_path: keymap_path.to_path_buf(),
        via: "test".into(),
        file_at_save,
    });
    Ok(())
}

pub fn disconnect(state: &LiveState) {
    *state.session.lock().unwrap() = None;
}

pub fn view(state: &LiveState, data: &Path, repo_dir: &Path) -> Result<LiveView, String> {
    state.view(data, repo_dir)
}

/// Compare the keyboard with the file.
pub fn compare(state: &LiveState) -> Result<Comparison, String> {
    let mut guard = state.session.lock().unwrap();
    let c = guard.as_mut().ok_or("Not connected.")?;
    let (doc, km, _) = load_keymap(&c.keymap_path)?;
    let device = c.live.keymap().map_err(|e| e.to_string())?;
    let mapper = c.live.mapper.as_ref().ok_or("Unlock the keyboard first.")?;
    let tokens = layer_tokens(&doc, &km);
    Ok(sync::compare(
        &device,
        &sync::expected(&km, mapper),
        mapper,
        &tokens,
    ))
}

/// A live key change: keyboard first, then the file (rolled back on the keyboard if
/// the file edit fails).
pub fn set_key(
    state: &LiveState,
    editor: &Editor,
    layer: usize,
    position: usize,
    binding: &str,
) -> Result<EditResult, String> {
    let mut guard = state.session.lock().unwrap();
    let c = guard.as_mut().ok_or("Not connected.")?;
    let before = c.live.keymap().map_err(|e| e.to_string())?;
    let old = before
        .layers
        .iter()
        .find(|l| l.id as usize == layer)
        .and_then(|l| l.bindings.get(position).copied())
        .ok_or("That key doesn't exist on the keyboard.")?;
    c.live
        .set_key(layer, position, binding)
        .map_err(|e| e.to_string())?;
    match editor.edit(
        &c.keymap_path,
        &EditOp::SetBinding {
            layer,
            position,
            binding: binding.to_string(),
        },
    ) {
        Ok(r) => Ok(r),
        Err(e) => {
            let _ = c.live.restore_key(layer, position, old);
            Err(e)
        }
    }
}

pub fn rename_layer(
    state: &LiveState,
    editor: &Editor,
    layer: usize,
    name: &str,
) -> Result<EditResult, String> {
    let mut guard = state.session.lock().unwrap();
    let c = guard.as_mut().ok_or("Not connected.")?;
    c.live
        .rename_layer(layer, name.trim())
        .map_err(|e| e.to_string())?;
    editor.edit(
        &c.keymap_path,
        &EditOp::RenameLayer {
            layer,
            name: name.to_string(),
        },
    )
}

pub fn save(state: &LiveState, data: &Path) -> Result<(), String> {
    let mut guard = state.session.lock().unwrap();
    let c = guard.as_mut().ok_or("Not connected.")?;
    c.live.save().map_err(|e| e.to_string())?;
    c.file_at_save = repo::read(&c.keymap_path).map_err(|e| e.to_string())?;
    let mut m = load_memory(data);
    m.saved_at.insert(c.live.info.serial_hex(), now());
    store_memory(data, &m);
    Ok(())
}

/// Undo everything since the last save: on the keyboard and in the file (the file
/// change is one undo step).
pub fn discard(state: &LiveState, editor: &Editor) -> Result<(), String> {
    let mut guard = state.session.lock().unwrap();
    let c = guard.as_mut().ok_or("Not connected.")?;
    c.live.discard().map_err(|e| e.to_string())?;
    let current = repo::read(&c.keymap_path).map_err(|e| e.to_string())?;
    if current != c.file_at_save {
        editor::write(&c.keymap_path, &c.file_at_save)?;
        editor.record(&c.keymap_path, current);
    }
    Ok(())
}

/// Clear the keyboard's saved live changes (it then uses the installed firmware's keymap).
pub fn clear_saved(state: &LiveState, data: &Path) -> Result<(), String> {
    let mut guard = state.session.lock().unwrap();
    let c = guard.as_mut().ok_or("Not connected.")?;
    c.live.clear_saved().map_err(|e| e.to_string())?;
    let mut m = load_memory(data);
    m.saved_at.remove(&c.live.info.serial_hex());
    store_memory(data, &m);
    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyRef {
    pub layer: usize,
    pub position: usize,
}

/// Resolve differences: `keep_keyboard` writes the keyboard's keys into the file,
/// otherwise the file's keys are sent to the keyboard (RAM; save afterwards).
pub fn resolve(
    state: &LiveState,
    editor: &Editor,
    keys: &[KeyRef],
    keep_keyboard: bool,
) -> Result<(), String> {
    let cmp = compare(state)?;
    let mut guard = state.session.lock().unwrap();
    let c = guard.as_mut().ok_or("Not connected.")?;
    for d in cmp.diffs.iter().filter(|d| {
        keys.iter()
            .any(|k| k.layer == d.layer && k.position == d.position)
    }) {
        if keep_keyboard {
            editor.edit(
                &c.keymap_path,
                &EditOp::SetBinding {
                    layer: d.layer,
                    position: d.position,
                    binding: d.device_text.clone(),
                },
            )?;
        } else {
            c.live
                .set_key(d.layer, d.position, &d.file_text)
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mxkeys_studio::proto::keymap::BehaviorBinding;
    use mxkeys_studio::sim::{ParamKind, SimBehavior, SimKeyboard, SimLayer};
    use zmk_config::cpp::Macros;

    const KEYMAP: &str = "#define A 0x70004\n#define B 0x70005\n#define ESC 0x70029\n\
/ { keymap { compatible = \"zmk,keymap\";\n\
    base { display-name = \"Base\"; bindings = <&kp A &kp B &studio_unlock>; };\n\
}; };\n";

    fn setup(name: &str) -> (PathBuf, LiveState, Editor) {
        let dir = std::env::temp_dir().join(format!("mxkeys-live-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let km = dir.join("corne.keymap");
        std::fs::write(&km, KEYMAP).unwrap();
        let kp = |p: u32| BehaviorBinding {
            behavior_id: 7,
            param1: p,
            param2: 0,
        };
        let mut sim = SimKeyboard::new(
            "Corne",
            vec![
                SimBehavior {
                    id: 7,
                    name: "Key Press".into(),
                    param1: ParamKind::HidUsage,
                    param2: ParamKind::Nil,
                },
                SimBehavior {
                    id: 8,
                    name: "Studio Unlock".into(),
                    param1: ParamKind::Nil,
                    param2: ParamKind::Nil,
                },
            ],
            vec![SimLayer {
                id: 0,
                name: "Base".into(),
                bindings: vec![
                    kp(0x70004),
                    kp(0x70005),
                    BehaviorBinding {
                        behavior_id: 8,
                        param1: 0,
                        param2: 0,
                    },
                ],
            }],
        );
        sim.press_unlock();
        let mut macros = Macros::default();
        macros.add_source(KEYMAP, true);
        let names: HashMap<String, String> = [("kp", "Key Press"), ("studio_unlock", "Studio Unlock")]
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .into();
        let state = LiveState::default();
        connect_with(
            &state,
            Box::new(sim),
            Mapper {
                macros,
                names,
                device: Default::default(),
            },
            &km,
        )
        .unwrap();
        (km, state, Editor::default())
    }

    #[test]
    fn live_key_change_goes_to_keyboard_and_file() {
        let (km, state, editor) = setup("edit");
        let data = km.parent().unwrap().join("appdata");
        let v = view(&state, &data, km.parent().unwrap()).unwrap();
        assert!(v.connected && v.status.as_ref().unwrap().unlocked);
        assert_eq!(v.unlock_key, Some(("Base".into(), 3)));
        assert!(compare(&state).unwrap().in_sync());

        let r = set_key(&state, &editor, 0, 1, "&kp ESC").unwrap();
        assert_eq!(r.keymap.layers[0].bindings[1], "&kp ESC");
        assert!(std::fs::read_to_string(&km).unwrap().contains("&kp ESC"));
        assert!(
            compare(&state).unwrap().in_sync(),
            "keyboard and file changed together"
        );
        assert!(
            view(&state, &data, km.parent().unwrap())
                .unwrap()
                .status
                .unwrap()
                .unsaved
        );

        // A key the keyboard can't take doesn't touch the file.
        assert!(set_key(&state, &editor, 0, 0, "&nope").is_err());
        assert!(!std::fs::read_to_string(&km).unwrap().contains("&nope"));

        // Discard: keyboard and file go back to the last saved state.
        discard(&state, &editor).unwrap();
        assert_eq!(std::fs::read_to_string(&km).unwrap(), KEYMAP);
        assert!(compare(&state).unwrap().in_sync());

        // Save, then a firmware install → offer to clear the saved changes.
        set_key(&state, &editor, 0, 1, "&kp ESC").unwrap();
        save(&state, &data).unwrap();
        assert!(!view(&state, &data, km.parent().unwrap()).unwrap().offer_clear);
        std::thread::sleep(std::time::Duration::from_millis(1100));
        record_install(&data, km.parent().unwrap());
        assert!(view(&state, &data, km.parent().unwrap()).unwrap().offer_clear);
        clear_saved(&state, &data).unwrap();
        assert!(!view(&state, &data, km.parent().unwrap()).unwrap().offer_clear);
        std::fs::remove_dir_all(km.parent().unwrap()).unwrap();
    }

    #[test]
    fn resolving_differences() {
        let (km, state, editor) = setup("resolve");
        // The file changes behind the keyboard's back (e.g. git pull).
        std::fs::write(&km, KEYMAP.replace("&kp A &kp B", "&kp B &kp A")).unwrap();
        let c = compare(&state).unwrap();
        assert_eq!(c.diffs.len(), 2);
        // Send the first key to the keyboard, keep the keyboard's version of the second.
        resolve(
            &state,
            &editor,
            &[KeyRef {
                layer: 0,
                position: 0,
            }],
            false,
        )
        .unwrap();
        resolve(
            &state,
            &editor,
            &[KeyRef {
                layer: 0,
                position: 1,
            }],
            true,
        )
        .unwrap();
        assert!(compare(&state).unwrap().in_sync());
        assert!(std::fs::read_to_string(&km).unwrap().contains("&kp B &kp B"));
        disconnect(&state);
        assert!(compare(&state).is_err());
        std::fs::remove_dir_all(km.parent().unwrap()).unwrap();
    }
}
