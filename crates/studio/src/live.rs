//! A live-editing session with one keyboard: connect, wait for the unlock key, map
//! behaviors, set keys, save/discard, clear saved overrides.

use std::time::Duration;

use serde::Serialize;

use crate::client::{Client, DeviceInfo, Error, Notification, Transport};
use crate::mapping::{DeviceBehaviors, Mapper, Unmappable};
use crate::proto::keymap::{BehaviorBinding, Keymap as DeviceKeymap};

impl<T: Transport + ?Sized> Transport for Box<T> {
    fn send(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        (**self).send(bytes)
    }
    fn recv(&mut self, timeout: Duration) -> std::io::Result<Vec<u8>> {
        (**self).recv(timeout)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LiveError {
    #[error(transparent)]
    Device(#[from] Error),
    #[error("this key can't be changed live: {0}. Save it in your files and build instead.")]
    Unmappable(Unmappable),
    #[error("connect to the keyboard and unlock it first")]
    NotReady,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveStatus {
    pub name: String,
    pub serial: String,
    pub unlocked: bool,
    /// Changes on the keyboard that aren't saved on it yet.
    pub unsaved: bool,
    /// Behavior names the keyboard reports more than once (keys using them are read-only).
    pub ambiguous: Vec<String>,
}

pub struct Live<T: Transport> {
    pub client: Client<T>,
    pub info: DeviceInfo,
    unlocked: bool,
    unsaved: bool,
    /// Ready after unlocking (needs behavior details, which are locked).
    pub mapper: Option<Mapper>,
    base: Mapper,
}

impl<T: Transport> Live<T> {
    /// `base` carries the macros and label → display-name table for the repo; its
    /// device behaviors are filled in once the keyboard is unlocked.
    pub fn connect(transport: T, base: Mapper) -> Result<Self, LiveError> {
        let mut client = Client::new(transport);
        let info = client.device_info()?;
        let unlocked = client.is_unlocked()?;
        let mut live = Live {
            client,
            info,
            unlocked,
            unsaved: false,
            mapper: None,
            base,
        };
        if unlocked {
            live.load()?;
        }
        Ok(live)
    }

    /// Pick up notifications (lock changes, unsaved status); loads the behavior table
    /// once the keyboard gets unlocked.
    pub fn poll(&mut self) -> Result<LiveStatus, LiveError> {
        self.client.poll(Duration::from_millis(20))?;
        for n in self.client.take_notifications() {
            match n {
                Notification::LockChanged { unlocked } => self.unlocked = unlocked,
                Notification::UnsavedChanges(u) => self.unsaved = u,
            }
        }
        if !self.unlocked {
            // The keyboard locks itself when idle; ask now and then.
            self.unlocked = self.client.is_unlocked()?;
        }
        if self.unlocked && self.mapper.is_none() {
            self.load()?;
        }
        Ok(self.status())
    }

    pub fn status(&self) -> LiveStatus {
        LiveStatus {
            name: self.info.name.clone(),
            serial: self.info.serial_hex(),
            unlocked: self.unlocked,
            unsaved: self.unsaved,
            ambiguous: self
                .mapper
                .as_ref()
                .map(|m| m.device.ambiguous.clone())
                .unwrap_or_default(),
        }
    }

    fn load(&mut self) -> Result<(), LiveError> {
        let ids = self.client.list_behaviors()?;
        let mut list = Vec::with_capacity(ids.len());
        for id in ids {
            list.push((id, self.client.behavior_details(id)?.display_name));
        }
        let mut m = self.base.clone();
        m.device = DeviceBehaviors::new(list);
        self.mapper = Some(m);
        self.unsaved = self.client.has_unsaved_changes()?;
        Ok(())
    }

    fn guard<R>(&mut self, r: Result<R, Error>) -> Result<R, LiveError> {
        if matches!(r, Err(Error::Locked)) {
            self.unlocked = false;
        }
        r.map_err(LiveError::from)
    }

    pub fn keymap(&mut self) -> Result<DeviceKeymap, LiveError> {
        let r = self.client.keymap();
        self.guard(r)
    }

    /// Map `text` and put it on the keyboard (RAM only until [`Live::save`]).
    pub fn set_key(
        &mut self,
        layer: usize,
        position: usize,
        text: &str,
    ) -> Result<BehaviorBinding, LiveError> {
        let b = self
            .mapper
            .as_ref()
            .ok_or(LiveError::NotReady)?
            .encode(text)
            .map_err(LiveError::Unmappable)?;
        let r = self.client.set_binding(layer as u32, position as u32, b);
        self.guard(r)?;
        self.unsaved = true;
        Ok(b)
    }

    /// Put a binding back exactly (used to roll back when the file edit fails).
    pub fn restore_key(
        &mut self,
        layer: usize,
        position: usize,
        b: BehaviorBinding,
    ) -> Result<(), LiveError> {
        let r = self.client.set_binding(layer as u32, position as u32, b);
        self.guard(r)
    }

    pub fn rename_layer(&mut self, layer: usize, name: &str) -> Result<(), LiveError> {
        let r = self.client.set_layer_name(layer as u32, name);
        self.guard(r)?;
        self.unsaved = true;
        Ok(())
    }

    pub fn save(&mut self) -> Result<(), LiveError> {
        let r = self.client.save();
        self.guard(r)?;
        self.unsaved = false;
        Ok(())
    }

    pub fn discard(&mut self) -> Result<(), LiveError> {
        let r = self.client.discard();
        self.guard(r)?;
        self.unsaved = false;
        Ok(())
    }

    /// Drop the keyboard's saved live changes so the installed firmware's keymap applies.
    pub fn clear_saved(&mut self) -> Result<(), LiveError> {
        let r = self.client.reset_settings();
        self.guard(r)?;
        self.unsaved = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::{ParamKind, SimBehavior, SimKeyboard, SimLayer};
    use std::collections::HashMap;
    use zmk_config::cpp::Macros;

    fn sim() -> SimKeyboard {
        let kp = |p: u32| BehaviorBinding {
            behavior_id: 40,
            param1: p,
            param2: 0,
        };
        SimKeyboard::new(
            "Corne",
            vec![
                // Ids are arbitrary per keyboard (settings table) — mapped by name.
                SimBehavior {
                    id: 40,
                    name: "Key Press".into(),
                    param1: ParamKind::HidUsage,
                    param2: ParamKind::Nil,
                },
                SimBehavior {
                    id: 41,
                    name: "Momentary Layer".into(),
                    param1: ParamKind::LayerId,
                    param2: ParamKind::Nil,
                },
            ],
            vec![SimLayer {
                id: 0,
                name: "Base".into(),
                bindings: vec![kp(0x70004), kp(0x70005)],
            }],
        )
    }

    fn base() -> Mapper {
        let mut macros = Macros::default();
        macros.add_source(
            "#define A 0x70004\n#define B 0x70005\n#define ESC 0x70029\n",
            true,
        );
        let names: HashMap<String, String> = [("kp", "Key Press"), ("mo", "Momentary Layer")]
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .into();
        Mapper {
            macros,
            names,
            device: Default::default(),
        }
    }

    #[test]
    fn unlock_edit_save_and_clear() {
        let mut live = Live::connect(sim(), base()).unwrap();
        assert!(!live.status().unlocked);
        assert!(matches!(live.set_key(0, 0, "&kp ESC"), Err(LiveError::NotReady)));
        live.client.transport_mut().press_unlock();
        let st = live.poll().unwrap();
        assert!(st.unlocked && !st.unsaved);
        let b = live.set_key(0, 0, "&kp ESC").unwrap();
        assert_eq!(
            b,
            BehaviorBinding {
                behavior_id: 40,
                param1: 0x70029,
                param2: 0
            }
        );
        assert!(live.poll().unwrap().unsaved);
        assert!(matches!(
            live.set_key(0, 1, "&tog 1"),
            Err(LiveError::Unmappable(_))
        ));
        live.save().unwrap();
        assert!(!live.status().unsaved);
        assert_eq!(live.client.transport_mut().saved_bindings.len(), 1);
        live.clear_saved().unwrap();
        assert!(live.client.transport_mut().saved_bindings.is_empty());
        assert_eq!(live.keymap().unwrap().layers[0].bindings[0].param1, 0x70004);
        // Rolling back restores the old binding.
        let old = live.keymap().unwrap().layers[0].bindings[1];
        live.set_key(0, 1, "&kp A").unwrap();
        live.restore_key(0, 1, old).unwrap();
        assert_eq!(live.keymap().unwrap().layers[0].bindings[1], old);
        live.discard().unwrap();
    }

    #[test]
    fn notices_the_keyboard_locking_itself() {
        let mut live = Live::connect(sim(), base()).unwrap();
        live.client.transport_mut().press_unlock();
        live.poll().unwrap();
        live.client.transport_mut().unlocked = false; // idle timeout on the keyboard
        assert!(matches!(live.keymap(), Err(LiveError::Device(Error::Locked))));
        assert!(!live.status().unlocked);
    }
}
