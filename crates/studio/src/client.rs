//! Request/response client over any [`Transport`]: request ids, timeouts,
//! notifications, and typed calls.

use std::collections::VecDeque;
use std::io;
use std::time::{Duration, Instant};

use prost::Message;

use crate::framing::{self, Decoder};
use crate::proto::{behaviors, core, keymap, meta, studio};

/// Byte transport to the keyboard (USB serial, BLE, or the simulator).
pub trait Transport: Send {
    fn send(&mut self, bytes: &[u8]) -> io::Result<()>;
    /// Wait up to `timeout` for incoming bytes; an empty result means nothing arrived.
    fn recv(&mut self, timeout: Duration) -> io::Result<Vec<u8>>;
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("lost the connection to the keyboard: {0}")]
    Io(#[from] io::Error),
    #[error("the keyboard didn't answer in time")]
    Timeout,
    #[error("the keyboard is locked — press its Studio unlock key")]
    Locked,
    #[error("the keyboard's firmware doesn't support this ({0})")]
    NotSupported(&'static str),
    #[error("the keyboard sent something mxKeys couldn't read: {0}")]
    Decode(String),
    #[error("the keyboard refused: {0}")]
    Refused(String),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notification {
    LockChanged { unlocked: bool },
    UnsavedChanges(bool),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub name: String,
    pub serial: Vec<u8>,
}

impl DeviceInfo {
    pub fn serial_hex(&self) -> String {
        self.serial.iter().map(|b| format!("{b:02x}")).collect()
    }
}

pub struct Client<T: Transport> {
    transport: T,
    decoder: Decoder,
    next_id: u32,
    pending: VecDeque<studio::Response>,
    notifications: Vec<Notification>,
    pub timeout: Duration,
}

type Sub = studio::request::Subsystem;
type RespSub = studio::request_response::Subsystem;

fn to_notification(n: studio::Notification) -> Option<Notification> {
    use studio::notification::Subsystem as N;
    match n.subsystem? {
        N::Core(c) => match c.notification_type? {
            core::notification::NotificationType::LockStateChanged(s) => Some(Notification::LockChanged {
                unlocked: s == core::LockState::Unlocked as i32,
            }),
        },
        N::Keymap(k) => match k.notification_type? {
            keymap::notification::NotificationType::UnsavedChangesStatusChanged(b) => {
                Some(Notification::UnsavedChanges(b))
            }
        },
    }
}

impl<T: Transport> Client<T> {
    pub fn new(transport: T) -> Self {
        Client {
            transport,
            decoder: Decoder::default(),
            next_id: 1,
            pending: VecDeque::new(),
            notifications: Vec::new(),
            timeout: Duration::from_secs(3),
        }
    }

    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    /// Notifications received so far (lock changes, unsaved-changes status).
    pub fn take_notifications(&mut self) -> Vec<Notification> {
        std::mem::take(&mut self.notifications)
    }

    /// Read whatever has arrived (e.g. to pick up notifications while idle).
    pub fn poll(&mut self, wait: Duration) -> Result<()> {
        let bytes = self.transport.recv(wait)?;
        self.absorb(&bytes)
    }

    fn absorb(&mut self, bytes: &[u8]) -> Result<()> {
        for frame in self.decoder.feed(bytes) {
            let resp =
                studio::Response::decode(frame.as_slice()).map_err(|e| Error::Decode(e.to_string()))?;
            match resp.r#type {
                Some(studio::response::Type::Notification(n)) => {
                    if let Some(n) = to_notification(n) {
                        self.notifications.push(n);
                    }
                }
                _ => self.pending.push_back(resp),
            }
        }
        Ok(())
    }

    fn call(&mut self, sub: Sub) -> Result<RespSub> {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let req = studio::Request {
            request_id: id,
            subsystem: Some(sub),
        };
        self.transport.send(&framing::encode(&req.encode_to_vec()))?;
        let deadline = Instant::now() + self.timeout;
        loop {
            while let Some(r) = self.pending.pop_front() {
                if let Some(studio::response::Type::RequestResponse(rr)) = r.r#type {
                    if rr.request_id != id {
                        continue; // late answer to an earlier, timed-out request
                    }
                    return match rr.subsystem {
                        Some(RespSub::Meta(m)) => Err(meta_error(m)),
                        Some(s) => Ok(s),
                        None => Err(Error::Decode("empty response".into())),
                    };
                }
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(Error::Timeout);
            }
            let bytes = self
                .transport
                .recv((deadline - now).min(Duration::from_millis(100)))?;
            self.absorb(&bytes)?;
        }
    }

    pub fn device_info(&mut self) -> Result<DeviceInfo> {
        match self.call(Sub::Core(core::Request {
            request_type: Some(core::request::RequestType::GetDeviceInfo(true)),
        }))? {
            RespSub::Core(core::Response {
                response_type: Some(core::response::ResponseType::GetDeviceInfo(i)),
            }) => Ok(DeviceInfo {
                name: i.name,
                serial: i.serial_number,
            }),
            other => Err(unexpected(other)),
        }
    }

    pub fn is_unlocked(&mut self) -> Result<bool> {
        match self.call(Sub::Core(core::Request {
            request_type: Some(core::request::RequestType::GetLockState(true)),
        }))? {
            RespSub::Core(core::Response {
                response_type: Some(core::response::ResponseType::GetLockState(s)),
            }) => Ok(s == core::LockState::Unlocked as i32),
            other => Err(unexpected(other)),
        }
    }

    /// Clear the keyboard's saved keymap changes (ZMK: only the keymap registers a
    /// settings-reset hook — Bluetooth pairings are kept).
    pub fn reset_settings(&mut self) -> Result<()> {
        match self.call(Sub::Core(core::Request {
            request_type: Some(core::request::RequestType::ResetSettings(true)),
        }))? {
            RespSub::Core(core::Response {
                response_type: Some(core::response::ResponseType::ResetSettings(true)),
            }) => Ok(()),
            RespSub::Core(_) => Err(Error::Refused("couldn't clear the saved changes".into())),
            other => Err(unexpected(other)),
        }
    }

    pub fn list_behaviors(&mut self) -> Result<Vec<u32>> {
        match self.call(Sub::Behaviors(behaviors::Request {
            request_type: Some(behaviors::request::RequestType::ListAllBehaviors(true)),
        }))? {
            RespSub::Behaviors(behaviors::Response {
                response_type: Some(behaviors::response::ResponseType::ListAllBehaviors(l)),
            }) => Ok(l.behaviors),
            other => Err(unexpected(other)),
        }
    }

    pub fn behavior_details(&mut self, id: u32) -> Result<behaviors::GetBehaviorDetailsResponse> {
        match self.call(Sub::Behaviors(behaviors::Request {
            request_type: Some(behaviors::request::RequestType::GetBehaviorDetails(
                behaviors::GetBehaviorDetailsRequest { behavior_id: id },
            )),
        }))? {
            RespSub::Behaviors(behaviors::Response {
                response_type: Some(behaviors::response::ResponseType::GetBehaviorDetails(d)),
            }) => Ok(d),
            other => Err(unexpected(other)),
        }
    }

    fn keymap_call(&mut self, r: keymap::request::RequestType) -> Result<keymap::response::ResponseType> {
        match self.call(Sub::Keymap(keymap::Request {
            request_type: Some(r),
        }))? {
            RespSub::Keymap(keymap::Response {
                response_type: Some(t),
            }) => Ok(t),
            other => Err(unexpected(other)),
        }
    }

    pub fn keymap(&mut self) -> Result<keymap::Keymap> {
        use keymap::response::ResponseType as R;
        match self.keymap_call(keymap::request::RequestType::GetKeymap(true))? {
            R::GetKeymap(k) => Ok(k),
            other => Err(Error::Decode(format!("unexpected keymap answer {other:?}"))),
        }
    }

    pub fn physical_layouts(&mut self) -> Result<keymap::PhysicalLayouts> {
        use keymap::response::ResponseType as R;
        match self.keymap_call(keymap::request::RequestType::GetPhysicalLayouts(true))? {
            R::GetPhysicalLayouts(l) => Ok(l),
            other => Err(Error::Decode(format!("unexpected layouts answer {other:?}"))),
        }
    }

    pub fn set_binding(
        &mut self,
        layer_id: u32,
        position: u32,
        binding: keymap::BehaviorBinding,
    ) -> Result<()> {
        use keymap::response::ResponseType as R;
        use keymap::SetLayerBindingResponse as S;
        match self.keymap_call(keymap::request::RequestType::SetLayerBinding(
            keymap::SetLayerBindingRequest {
                layer_id,
                key_position: position as i32,
                binding: Some(binding),
            },
        ))? {
            R::SetLayerBinding(code) => match S::try_from(code) {
                Ok(S::Ok) => Ok(()),
                Ok(S::InvalidLocation) => Err(Error::Refused(
                    "that key or layer doesn't exist on the keyboard".into(),
                )),
                Ok(S::InvalidBehavior) => {
                    Err(Error::Refused("the keyboard doesn't have that behavior".into()))
                }
                Ok(S::InvalidParameters) => Err(Error::Refused(
                    "the keyboard doesn't accept those parameters".into(),
                )),
                Err(_) => Err(Error::Refused(format!("error {code}"))),
            },
            other => Err(Error::Decode(format!("unexpected answer {other:?}"))),
        }
    }

    pub fn set_layer_name(&mut self, layer_id: u32, name: &str) -> Result<()> {
        use keymap::response::ResponseType as R;
        match self.keymap_call(keymap::request::RequestType::SetLayerProps(
            keymap::SetLayerPropsRequest {
                layer_id,
                name: name.to_string(),
            },
        ))? {
            R::SetLayerProps(0) => Ok(()),
            R::SetLayerProps(c) => Err(Error::Refused(format!("couldn't rename the layer (error {c})"))),
            other => Err(Error::Decode(format!("unexpected answer {other:?}"))),
        }
    }

    pub fn has_unsaved_changes(&mut self) -> Result<bool> {
        use keymap::response::ResponseType as R;
        match self.keymap_call(keymap::request::RequestType::CheckUnsavedChanges(true))? {
            R::CheckUnsavedChanges(b) => Ok(b),
            other => Err(Error::Decode(format!("unexpected answer {other:?}"))),
        }
    }

    pub fn save(&mut self) -> Result<()> {
        use keymap::response::ResponseType as R;
        use keymap::save_changes_response::Result as SR;
        match self.keymap_call(keymap::request::RequestType::SaveChanges(true))? {
            R::SaveChanges(keymap::SaveChangesResponse {
                result: Some(SR::Ok(_)),
            }) => Ok(()),
            R::SaveChanges(keymap::SaveChangesResponse {
                result: Some(SR::Err(c)),
            }) => Err(Error::Refused(match keymap::SaveChangesErrorCode::try_from(c) {
                Ok(keymap::SaveChangesErrorCode::NoSpace) => "the keyboard's storage is full".into(),
                Ok(keymap::SaveChangesErrorCode::NotSupported) => "this firmware can't save changes".into(),
                _ => format!("saving failed (error {c})"),
            })),
            other => Err(Error::Decode(format!("unexpected answer {other:?}"))),
        }
    }

    pub fn discard(&mut self) -> Result<()> {
        use keymap::response::ResponseType as R;
        match self.keymap_call(keymap::request::RequestType::DiscardChanges(true))? {
            R::DiscardChanges(true) => Ok(()),
            R::DiscardChanges(false) => Err(Error::Refused("couldn't discard the changes".into())),
            other => Err(Error::Decode(format!("unexpected answer {other:?}"))),
        }
    }
}

fn meta_error(m: meta::Response) -> Error {
    use meta::response::ResponseType as M;
    match m.response_type {
        Some(M::SimpleError(c)) => match meta::ErrorConditions::try_from(c) {
            Ok(meta::ErrorConditions::UnlockRequired) => Error::Locked,
            Ok(meta::ErrorConditions::RpcNotFound) => Error::NotSupported("unknown request"),
            Ok(other) => Error::Refused(format!("{other:?}")),
            Err(_) => Error::Refused(format!("error {c}")),
        },
        Some(M::NoResponse(_)) | None => Error::Decode("empty answer".into()),
    }
}

fn unexpected(s: RespSub) -> Error {
    Error::Decode(format!("unexpected answer {s:?}"))
}
