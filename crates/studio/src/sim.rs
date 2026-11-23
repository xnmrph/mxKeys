//! A simulated ZMK keyboard speaking Studio RPC, for tests and CI. It follows the
//! firmware's rules (ZMK 4de3133):
//! - only device info, lock state and the behavior list work while locked;
//! - `lock` isn't implemented (`RPC_NOT_FOUND`);
//! - live changes stay in RAM until `save_changes`; `discard_changes` reloads the saved
//!   state;
//! - saved changes are per-key overrides that survive flashing new firmware;
//! - `reset_settings` drops the overrides (and nothing else).

use std::collections::{BTreeMap, VecDeque};
use std::io;
use std::time::Duration;

use prost::Message;

use crate::client::Transport;
use crate::framing::{self, Decoder};
use crate::proto::{behaviors, core, keymap, meta, studio};

#[derive(Debug, Clone, PartialEq)]
pub enum ParamKind {
    Nil,
    HidUsage,
    LayerId,
    /// Named constants (e.g. `&bt` commands).
    Constants(Vec<(String, u32)>),
    Range(i32, i32),
}

#[derive(Debug, Clone)]
pub struct SimBehavior {
    pub id: u32,
    pub name: String,
    pub param1: ParamKind,
    pub param2: ParamKind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SimLayer {
    pub id: u32,
    pub name: String,
    pub bindings: Vec<keymap::BehaviorBinding>,
}

#[derive(Debug, Clone)]
pub struct SimKeyboard {
    pub name: String,
    pub serial: Vec<u8>,
    pub unlocked: bool,
    pub behaviors: Vec<SimBehavior>,
    /// The keymap compiled into the firmware.
    pub compiled: Vec<SimLayer>,
    /// Saved per-key overrides and layer names (settings storage).
    pub saved_bindings: BTreeMap<(u32, u32), keymap::BehaviorBinding>,
    pub saved_names: BTreeMap<u32, String>,
    /// What the keyboard uses right now (RAM).
    pub live: Vec<SimLayer>,
    /// Bytes written to the settings storage so far (to see how often we save).
    pub storage_writes: usize,
    /// For demos: unlock by itself after this many lock-state queries.
    pub auto_unlock_after: Option<u32>,
    decoder: Decoder,
    outbox: VecDeque<u8>,
}

fn sub_resp(id: u32, s: studio::request_response::Subsystem) -> studio::Response {
    studio::Response {
        r#type: Some(studio::response::Type::RequestResponse(studio::RequestResponse {
            request_id: id,
            subsystem: Some(s),
        })),
    }
}

impl SimKeyboard {
    pub fn new(name: &str, behaviors: Vec<SimBehavior>, compiled: Vec<SimLayer>) -> Self {
        let mut k = SimKeyboard {
            name: name.into(),
            serial: vec![0xde, 0xad, 0xbe, 0xef],
            unlocked: false,
            behaviors,
            live: compiled.clone(),
            compiled,
            saved_bindings: BTreeMap::new(),
            saved_names: BTreeMap::new(),
            storage_writes: 0,
            auto_unlock_after: None,
            decoder: Decoder::default(),
            outbox: VecDeque::new(),
        };
        k.reload();
        k
    }

    /// RAM = compiled keymap + saved overrides (what happens at boot or on discard).
    fn reload(&mut self) {
        self.live = self.compiled.clone();
        for l in &mut self.live {
            if let Some(n) = self.saved_names.get(&l.id) {
                l.name = n.clone();
            }
            for (pos, b) in l.bindings.iter_mut().enumerate() {
                if let Some(o) = self.saved_bindings.get(&(l.id, pos as u32)) {
                    *b = *o;
                }
            }
        }
    }

    fn tick_unlock(&mut self) -> bool {
        if let Some(n) = self.auto_unlock_after.as_mut() {
            if *n == 0 {
                self.auto_unlock_after = None;
                self.unlocked = true;
            } else {
                *n -= 1;
            }
        }
        self.unlocked
    }

    /// Press the `&studio_unlock` key.
    pub fn press_unlock(&mut self) {
        self.unlocked = true;
        self.notify(studio::notification::Subsystem::Core(core::Notification {
            notification_type: Some(core::notification::NotificationType::LockStateChanged(
                core::LockState::Unlocked as i32,
            )),
        }));
    }

    /// Flash new firmware: the compiled keymap changes, the settings (overrides) stay;
    /// the keyboard restarts locked.
    pub fn flash(&mut self, compiled: Vec<SimLayer>) {
        self.compiled = compiled;
        self.unlocked = false;
        self.reload();
    }

    pub fn unsaved(&self) -> bool {
        let mut saved = self.clone();
        saved.reload();
        saved.live != self.live
    }

    fn notify(&mut self, n: studio::notification::Subsystem) {
        let r = studio::Response {
            r#type: Some(studio::response::Type::Notification(studio::Notification {
                subsystem: Some(n),
            })),
        };
        self.outbox.extend(framing::encode(&r.encode_to_vec()));
    }

    fn unsaved_notification(&mut self, v: bool) {
        self.notify(studio::notification::Subsystem::Keymap(keymap::Notification {
            notification_type: Some(keymap::notification::NotificationType::UnsavedChangesStatusChanged(v)),
        }));
    }

    fn handle(&mut self, req: studio::Request) -> studio::Response {
        use studio::request::Subsystem as S;
        use studio::request_response::Subsystem as R;
        let id = req.request_id;
        let err = |c: meta::ErrorConditions| {
            sub_resp(
                id,
                R::Meta(meta::Response {
                    response_type: Some(meta::response::ResponseType::SimpleError(c as i32)),
                }),
            )
        };
        let unsecured = matches!(
            &req.subsystem,
            Some(S::Core(core::Request {
                request_type: Some(
                    core::request::RequestType::GetDeviceInfo(_)
                        | core::request::RequestType::GetLockState(_)
                )
            })) | Some(S::Behaviors(behaviors::Request {
                request_type: Some(behaviors::request::RequestType::ListAllBehaviors(_))
            }))
        );
        if !unsecured && !self.unlocked {
            // `lock` isn't a registered handler at all, locked or not.
            if let Some(S::Core(core::Request {
                request_type: Some(core::request::RequestType::Lock(_)),
            })) = &req.subsystem
            {
                return err(meta::ErrorConditions::RpcNotFound);
            }
            return err(meta::ErrorConditions::UnlockRequired);
        }
        match req.subsystem {
            Some(S::Core(c)) => {
                use core::request::RequestType as Q;
                use core::response::ResponseType as A;
                let a = match c.request_type {
                    Some(Q::GetDeviceInfo(_)) => A::GetDeviceInfo(core::GetDeviceInfoResponse {
                        name: self.name.clone(),
                        serial_number: self.serial.clone(),
                    }),
                    Some(Q::GetLockState(_)) => A::GetLockState(if self.tick_unlock() {
                        core::LockState::Unlocked as i32
                    } else {
                        core::LockState::Locked as i32
                    }),
                    Some(Q::ResetSettings(_)) => {
                        self.saved_bindings.clear();
                        self.saved_names.clear();
                        self.storage_writes += 1;
                        self.reload();
                        A::ResetSettings(true)
                    }
                    Some(Q::Lock(_)) | None => return err(meta::ErrorConditions::RpcNotFound),
                };
                sub_resp(
                    id,
                    R::Core(core::Response {
                        response_type: Some(a),
                    }),
                )
            }
            Some(S::Behaviors(b)) => {
                use behaviors::request::RequestType as Q;
                use behaviors::response::ResponseType as A;
                let a = match b.request_type {
                    Some(Q::ListAllBehaviors(_)) => {
                        A::ListAllBehaviors(behaviors::ListAllBehaviorsResponse {
                            behaviors: self.behaviors.iter().map(|b| b.id).collect(),
                        })
                    }
                    Some(Q::GetBehaviorDetails(d)) => {
                        match self.behaviors.iter().find(|b| b.id == d.behavior_id) {
                            Some(b) => A::GetBehaviorDetails(details(b)),
                            None => return err(meta::ErrorConditions::Generic),
                        }
                    }
                    None => return err(meta::ErrorConditions::RpcNotFound),
                };
                sub_resp(
                    id,
                    R::Behaviors(behaviors::Response {
                        response_type: Some(a),
                    }),
                )
            }
            Some(S::Keymap(k)) => {
                use keymap::request::RequestType as Q;
                use keymap::response::ResponseType as A;
                let a = match k.request_type {
                    Some(Q::GetKeymap(_)) => A::GetKeymap(keymap::Keymap {
                        layers: self
                            .live
                            .iter()
                            .map(|l| keymap::Layer {
                                id: l.id,
                                name: l.name.clone(),
                                bindings: l.bindings.clone(),
                            })
                            .collect(),
                        available_layers: 0,
                        max_layer_name_length: 20,
                    }),
                    Some(Q::SetLayerBinding(s)) => {
                        let b = s.binding.unwrap_or_default();
                        let code = if !self.behaviors.iter().any(|x| x.id as i32 == b.behavior_id) {
                            keymap::SetLayerBindingResponse::InvalidBehavior
                        } else {
                            match self.live.iter_mut().find(|l| l.id == s.layer_id) {
                                Some(l)
                                    if s.key_position >= 0
                                        && (s.key_position as usize) < l.bindings.len() =>
                                {
                                    l.bindings[s.key_position as usize] = b;
                                    keymap::SetLayerBindingResponse::Ok
                                }
                                _ => keymap::SetLayerBindingResponse::InvalidLocation,
                            }
                        };
                        if code == keymap::SetLayerBindingResponse::Ok {
                            let u = self.unsaved();
                            self.unsaved_notification(u);
                        }
                        A::SetLayerBinding(code as i32)
                    }
                    Some(Q::SetLayerProps(p)) => match self.live.iter_mut().find(|l| l.id == p.layer_id) {
                        Some(l) => {
                            l.name = p.name;
                            A::SetLayerProps(keymap::SetLayerPropsResponse::Ok as i32)
                        }
                        None => A::SetLayerProps(keymap::SetLayerPropsResponse::ErrInvalidId as i32),
                    },
                    Some(Q::CheckUnsavedChanges(_)) => A::CheckUnsavedChanges(self.unsaved()),
                    Some(Q::SaveChanges(_)) => {
                        for (l, c) in self.live.iter().zip(&self.compiled) {
                            if l.name != c.name {
                                self.saved_names.insert(l.id, l.name.clone());
                            }
                            for (pos, (b, cb)) in l.bindings.iter().zip(&c.bindings).enumerate() {
                                let key = (l.id, pos as u32);
                                if b != cb || self.saved_bindings.contains_key(&key) {
                                    self.saved_bindings.insert(key, *b);
                                    self.storage_writes += 1;
                                }
                            }
                        }
                        self.unsaved_notification(false);
                        A::SaveChanges(keymap::SaveChangesResponse {
                            result: Some(keymap::save_changes_response::Result::Ok(true)),
                        })
                    }
                    Some(Q::DiscardChanges(_)) => {
                        self.reload();
                        self.unsaved_notification(false);
                        A::DiscardChanges(true)
                    }
                    Some(Q::GetPhysicalLayouts(_)) => A::GetPhysicalLayouts(keymap::PhysicalLayouts {
                        active_layout_index: 0,
                        layouts: vec![keymap::PhysicalLayout {
                            name: "6-column".into(),
                            keys: vec![
                                keymap::KeyPhysicalAttrs {
                                    width: 100,
                                    height: 100,
                                    ..Default::default()
                                };
                                self.compiled.first().map_or(0, |l| l.bindings.len())
                            ],
                        }],
                    }),
                    _ => return err(meta::ErrorConditions::RpcNotFound),
                };
                sub_resp(
                    id,
                    R::Keymap(keymap::Response {
                        response_type: Some(a),
                    }),
                )
            }
            None => err(meta::ErrorConditions::MsgDecodeFailed),
        }
    }
}

fn desc(name: &str, k: &ParamKind) -> Vec<behaviors::BehaviorParameterValueDescription> {
    use behaviors::value_description::ValueType as V;
    let one = |v: V| behaviors::BehaviorParameterValueDescription {
        name: name.into(),
        value_type: Some(v),
    };
    match k {
        ParamKind::Nil => vec![one(V::Nil(behaviors::BehaviorParameterNil {}))],
        ParamKind::HidUsage => vec![one(V::HidUsage(behaviors::BehaviorParameterHidUsage {
            keyboard_max: 0xff,
            consumer_max: 0xfff,
        }))],
        ParamKind::LayerId => vec![one(V::LayerId(behaviors::BehaviorParameterLayerId {}))],
        ParamKind::Range(a, b) => vec![one(V::Range(behaviors::BehaviorParameterValueDescriptionRange {
            min: *a,
            max: *b,
        }))],
        ParamKind::Constants(cs) => cs
            .iter()
            .map(|(n, v)| behaviors::BehaviorParameterValueDescription {
                name: n.clone(),
                value_type: Some(V::Constant(*v)),
            })
            .collect(),
    }
}

fn details(b: &SimBehavior) -> behaviors::GetBehaviorDetailsResponse {
    behaviors::GetBehaviorDetailsResponse {
        id: b.id,
        display_name: b.name.clone(),
        metadata: vec![behaviors::BehaviorBindingParametersSet {
            param1: desc("", &b.param1),
            param2: desc("", &b.param2),
        }],
    }
}

impl Transport for SimKeyboard {
    fn send(&mut self, bytes: &[u8]) -> io::Result<()> {
        for frame in self.decoder.feed(bytes) {
            let resp = match studio::Request::decode(frame.as_slice()) {
                Ok(req) => self.handle(req),
                Err(_) => sub_resp(
                    0,
                    studio::request_response::Subsystem::Meta(meta::Response {
                        response_type: Some(meta::response::ResponseType::SimpleError(
                            meta::ErrorConditions::MsgDecodeFailed as i32,
                        )),
                    }),
                ),
            };
            self.outbox.extend(framing::encode(&resp.encode_to_vec()));
        }
        Ok(())
    }

    fn recv(&mut self, _timeout: Duration) -> io::Result<Vec<u8>> {
        // Deliver in small chunks, like a serial port would.
        let n = self.outbox.len().min(17);
        Ok(self.outbox.drain(..n).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{Client, Error, Notification};

    fn kp(p: u32) -> keymap::BehaviorBinding {
        keymap::BehaviorBinding {
            behavior_id: 1,
            param1: p,
            param2: 0,
        }
    }

    pub fn keyboard() -> SimKeyboard {
        SimKeyboard::new(
            "Corne xnmrph",
            vec![
                SimBehavior {
                    id: 1,
                    name: "Key Press".into(),
                    param1: ParamKind::HidUsage,
                    param2: ParamKind::Nil,
                },
                SimBehavior {
                    id: 2,
                    name: "Momentary Layer".into(),
                    param1: ParamKind::LayerId,
                    param2: ParamKind::Nil,
                },
            ],
            vec![
                SimLayer {
                    id: 0,
                    name: "Base".into(),
                    bindings: vec![kp(0x70004), kp(0x70005), kp(0x70006)],
                },
                SimLayer {
                    id: 1,
                    name: "Nav".into(),
                    bindings: vec![kp(0x70050), kp(0x70051), kp(0x7004f)],
                },
            ],
        )
    }

    #[test]
    fn locked_until_the_unlock_key() {
        let mut c = Client::new(keyboard());
        let info = c.device_info().unwrap();
        assert_eq!(info.name, "Corne xnmrph");
        assert_eq!(info.serial_hex(), "deadbeef");
        assert!(!c.is_unlocked().unwrap());
        assert_eq!(c.list_behaviors().unwrap(), [1, 2]);
        assert!(matches!(c.keymap(), Err(Error::Locked)));
        assert!(matches!(c.behavior_details(1), Err(Error::Locked)));
        c.transport_mut().press_unlock();
        c.poll(Duration::ZERO).unwrap();
        c.poll(Duration::ZERO).unwrap();
        assert_eq!(
            c.take_notifications(),
            [Notification::LockChanged { unlocked: true }]
        );
        assert_eq!(c.behavior_details(2).unwrap().display_name, "Momentary Layer");
        assert_eq!(c.keymap().unwrap().layers.len(), 2);
    }

    #[test]
    fn live_save_discard_reset_and_flash() {
        let mut c = Client::new(keyboard());
        c.transport_mut().press_unlock();
        c.set_binding(0, 1, kp(0x70029)).unwrap(); // B → Esc
        assert!(c.has_unsaved_changes().unwrap());
        assert!(c
            .take_notifications()
            .contains(&Notification::UnsavedChanges(true)));
        c.discard().unwrap();
        assert_eq!(c.keymap().unwrap().layers[0].bindings[1], kp(0x70005));
        c.set_binding(0, 1, kp(0x70029)).unwrap();
        c.set_layer_name(1, "Arrows").unwrap();
        c.save().unwrap();
        assert!(!c.has_unsaved_changes().unwrap());
        // Flashing new firmware keeps the saved override (it hides the new key!).
        let mut newer = c.transport_mut().compiled.clone();
        newer[0].bindings[1] = kp(0x7002b); // the files now say Tab
        c.transport_mut().flash(newer);
        c.transport_mut().press_unlock();
        let km = c.keymap().unwrap();
        assert_eq!(
            km.layers[0].bindings[1],
            kp(0x70029),
            "the old live change still wins"
        );
        assert_eq!(km.layers[1].name, "Arrows");
        // Clearing the saved changes brings back the firmware's keymap.
        c.reset_settings().unwrap();
        let km = c.keymap().unwrap();
        assert_eq!(km.layers[0].bindings[1], kp(0x7002b));
        assert_eq!(km.layers[1].name, "Nav");
        // Errors.
        assert!(matches!(c.set_binding(9, 0, kp(4)), Err(Error::Refused(_))));
        assert!(matches!(
            c.set_binding(
                0,
                0,
                keymap::BehaviorBinding {
                    behavior_id: 77,
                    param1: 0,
                    param2: 0
                }
            ),
            Err(Error::Refused(_))
        ));
    }
}
