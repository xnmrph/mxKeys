//! Bluetooth (GATT) transport. ZMK serves Studio RPC on service
//! `00000000-0196-6107-c967-c5cfb1c2482a`, characteristic `00000001-…` (write +
//! indications); both require an encrypted link, i.e. the keyboard must already be
//! paired with this computer. Only the central half has the service.
//!
//! `btleplug` is async; it runs on a small runtime in its own thread and is bridged to
//! the blocking [`Transport`] with channels.

use std::io;
use std::sync::mpsc;
use std::time::Duration;

use btleplug::api::{Central, Manager as _, Peripheral as _, ScanFilter, WriteType};
use btleplug::platform::{Manager, Peripheral};
use futures::StreamExt;
use serde::Serialize;
use uuid::Uuid;

use crate::client::Transport;

pub const SERVICE: Uuid = Uuid::from_u128(0x00000000_0196_6107_c967_c5cfb1c2482a);
pub const RPC_CHAR: Uuid = Uuid::from_u128(0x00000001_0196_6107_c967_c5cfb1c2482a);
/// Conservative write size (ATT default MTU 23 − 3); the firmware reassembles frames
/// from any number of writes.
const CHUNK: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BleCandidate {
    /// Platform id of the device (pass back to [`BleTransport::connect`]).
    pub id: String,
    pub name: String,
    /// Already advertises/exposes the Studio service.
    pub has_studio: bool,
}

fn runtime() -> io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread().enable_all().build()
}

fn ble_err(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(format!("Bluetooth: {e}"))
}

async fn adapter() -> io::Result<btleplug::platform::Adapter> {
    let manager = Manager::new().await.map_err(ble_err)?;
    manager
        .adapters()
        .await
        .map_err(ble_err)?
        .into_iter()
        .next()
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "no Bluetooth adapter found (is Bluetooth on?)",
            )
        })
}

/// Look for keyboards for `wait`. Paired/connected keyboards are listed too.
pub fn scan(wait: Duration) -> io::Result<Vec<BleCandidate>> {
    runtime()?.block_on(async {
        let central = adapter().await?;
        central.start_scan(ScanFilter::default()).await.map_err(ble_err)?;
        tokio::time::sleep(wait).await;
        let _ = central.stop_scan().await;
        let mut out = Vec::new();
        for p in central.peripherals().await.map_err(ble_err)? {
            let Ok(Some(props)) = p.properties().await else {
                continue;
            };
            let Some(name) = props.local_name else { continue };
            let has_studio = props.services.contains(&SERVICE);
            out.push(BleCandidate {
                id: p.id().to_string(),
                name,
                has_studio,
            });
        }
        out.sort_by(|a, b| b.has_studio.cmp(&a.has_studio).then(a.name.cmp(&b.name)));
        Ok(out)
    })
}

enum Cmd {
    Send(Vec<u8>),
    Close,
}

pub struct BleTransport {
    to_task: tokio::sync::mpsc::UnboundedSender<Cmd>,
    from_task: mpsc::Receiver<io::Result<Vec<u8>>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl BleTransport {
    /// Connect to the keyboard with platform id `id` (from [`scan`]).
    pub fn connect(id: &str) -> io::Result<BleTransport> {
        let (to_task, mut cmds) = tokio::sync::mpsc::unbounded_channel::<Cmd>();
        let (tx, from_task) = mpsc::channel::<io::Result<Vec<u8>>>();
        let (ready_tx, ready_rx) = mpsc::channel::<io::Result<()>>();
        let id = id.to_string();
        let thread = std::thread::spawn(move || {
            let rt = match runtime() {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            rt.block_on(async move {
                let setup = async {
                    let central = adapter().await?;
                    central.start_scan(ScanFilter::default()).await.map_err(ble_err)?;
                    let mut found: Option<Peripheral> = None;
                    for _ in 0..50 {
                        if let Some(p) = central
                            .peripherals()
                            .await
                            .map_err(ble_err)?
                            .into_iter()
                            .find(|p| p.id().to_string() == id)
                        {
                            found = Some(p);
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                    let _ = central.stop_scan().await;
                    let p = found.ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "keyboard not found over Bluetooth"))?;
                    if !p.is_connected().await.unwrap_or(false) {
                        p.connect().await.map_err(ble_err)?;
                    }
                    p.discover_services().await.map_err(ble_err)?;
                    let ch = p
                        .characteristics()
                        .into_iter()
                        .find(|c| c.uuid == RPC_CHAR && c.service_uuid == SERVICE)
                        .ok_or_else(|| {
                            io::Error::new(
                                io::ErrorKind::Unsupported,
                                "this keyboard has no ZMK Studio service over Bluetooth (wrong half, or Studio not enabled)",
                            )
                        })?;
                    p.subscribe(&ch).await.map_err(ble_err)?;
                    let stream = p.notifications().await.map_err(ble_err)?;
                    Ok::<_, io::Error>((p, ch, stream))
                };
                let (p, ch, mut stream) = match setup.await {
                    Ok(v) => {
                        let _ = ready_tx.send(Ok(()));
                        v
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                loop {
                    tokio::select! {
                        cmd = cmds.recv() => match cmd {
                            Some(Cmd::Send(bytes)) => {
                                for chunk in bytes.chunks(CHUNK) {
                                    if let Err(e) = p.write(&ch, chunk, WriteType::WithResponse).await {
                                        let _ = tx.send(Err(ble_err(e)));
                                        return;
                                    }
                                }
                            }
                            Some(Cmd::Close) | None => {
                                let _ = p.unsubscribe(&ch).await;
                                return;
                            }
                        },
                        n = stream.next() => match n {
                            Some(v) if v.uuid == RPC_CHAR => { let _ = tx.send(Ok(v.value)); }
                            Some(_) => {}
                            None => {
                                let _ = tx.send(Err(io::Error::new(io::ErrorKind::ConnectionAborted, "the keyboard disconnected")));
                                return;
                            }
                        },
                    }
                }
            });
        });
        match ready_rx.recv_timeout(Duration::from_secs(20)) {
            Ok(Ok(())) => Ok(BleTransport {
                to_task,
                from_task,
                thread: Some(thread),
            }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "connecting over Bluetooth took too long",
            )),
        }
    }
}

impl Transport for BleTransport {
    fn send(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.to_task
            .send(Cmd::Send(bytes.to_vec()))
            .map_err(|_| io::Error::new(io::ErrorKind::ConnectionAborted, "Bluetooth connection closed"))
    }

    fn recv(&mut self, timeout: Duration) -> io::Result<Vec<u8>> {
        match self.from_task.recv_timeout(timeout) {
            Ok(r) => r,
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(vec![]),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "Bluetooth connection closed",
            )),
        }
    }
}

impl Drop for BleTransport {
    fn drop(&mut self) {
        let _ = self.to_task.send(Cmd::Close);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuids_match_zmk() {
        assert_eq!(SERVICE.to_string(), "00000000-0196-6107-c967-c5cfb1c2482a");
        assert_eq!(RPC_CHAR.to_string(), "00000001-0196-6107-c967-c5cfb1c2482a");
    }
}
