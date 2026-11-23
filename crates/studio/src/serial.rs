//! USB serial (CDC-ACM) transport: ZMK exposes Studio RPC on a serial port when the
//! firmware is built with the `studio-rpc-usb-uart` snippet (the owner's left half).

use std::io::{self, Read, Write};
use std::time::Duration;

use serde::Serialize;

use crate::client::Transport;

/// ZMK's USB ids (OpenMoko-allocated).
pub const ZMK_VID: u16 = 0x1D50;
pub const ZMK_PID: u16 = 0x615E;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SerialCandidate {
    /// `/dev/ttyACM0`, `/dev/cu.usbmodem…`, `COM5`.
    pub path: String,
    pub product: Option<String>,
    pub serial_number: Option<String>,
}

/// Serial ports that belong to a ZMK keyboard.
pub fn find_keyboards() -> Vec<SerialCandidate> {
    let Ok(ports) = serialport::available_ports() else {
        return vec![];
    };
    let mut out: Vec<SerialCandidate> = ports
        .into_iter()
        .filter_map(|p| match p.port_type {
            serialport::SerialPortType::UsbPort(u) if u.vid == ZMK_VID && u.pid == ZMK_PID => {
                Some(SerialCandidate {
                    // macOS lists both /dev/tty.* and /dev/cu.*; cu.* is the one to open.
                    path: p.port_name,
                    product: u.product,
                    serial_number: u.serial_number,
                })
            }
            _ => None,
        })
        .filter(|c| !c.path.starts_with("/dev/tty.usb"))
        .collect();
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

pub struct SerialTransport {
    port: Box<dyn serialport::SerialPort>,
}

impl SerialTransport {
    pub fn open(path: &str) -> io::Result<SerialTransport> {
        let port = serialport::new(path, 115_200)
            .timeout(Duration::from_millis(50))
            .open()
            .map_err(|e| match e.kind {
                serialport::ErrorKind::Io(io::ErrorKind::PermissionDenied) => io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!(
                        "no permission to open {path}. On Linux, add yourself to the serial-port group \
                         (`sudo usermod -aG dialout $USER` on Debian/Ubuntu/Fedora, `uucp` on Arch), then log out and in."
                    ),
                ),
                serialport::ErrorKind::NoDevice => io::Error::new(io::ErrorKind::NotFound, format!("{path} is gone (keyboard unplugged?)")),
                _ => io::Error::other(e.to_string()),
            })?;
        // CDC-ACM ports often only send once the host raises DTR.
        let mut port = port;
        let _ = port.write_data_terminal_ready(true);
        Ok(SerialTransport { port })
    }
}

impl Transport for SerialTransport {
    fn send(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.port.write_all(bytes)?;
        self.port.flush()
    }

    fn recv(&mut self, timeout: Duration) -> io::Result<Vec<u8>> {
        let _ = self.port.set_timeout(timeout.max(Duration::from_millis(1)));
        let mut buf = [0u8; 512];
        match self.port.read(&mut buf) {
            Ok(n) => Ok(buf[..n].to_vec()),
            Err(e) if e.kind() == io::ErrorKind::TimedOut || e.kind() == io::ErrorKind::WouldBlock => {
                Ok(vec![])
            }
            Err(e) => Err(e),
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::client::Client;
    use crate::proto::keymap::BehaviorBinding;
    use crate::sim::{ParamKind, SimBehavior, SimKeyboard, SimLayer};
    use serialport::SerialPort;

    /// The real serial transport talking to the simulator over a pseudo-terminal.
    #[test]
    fn talks_to_a_keyboard_over_a_pty() {
        let (host, mut device) = serialport::TTYPort::pair().expect("pty pair");
        let mut kb = SimKeyboard::new(
            "PTY board",
            vec![SimBehavior {
                id: 7,
                name: "Key Press".into(),
                param1: ParamKind::HidUsage,
                param2: ParamKind::Nil,
            }],
            vec![SimLayer {
                id: 0,
                name: "Base".into(),
                bindings: vec![BehaviorBinding {
                    behavior_id: 7,
                    param1: 0x70004,
                    param2: 0,
                }],
            }],
        );
        kb.unlocked = true;
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop2 = stop.clone();
        let dev = std::thread::spawn(move || {
            device.set_timeout(Duration::from_millis(20)).unwrap();
            let mut buf = [0u8; 256];
            while !stop2.load(std::sync::atomic::Ordering::Relaxed) {
                if let Ok(n) = device.read(&mut buf) {
                    kb.send(&buf[..n]).unwrap();
                }
                loop {
                    let out = kb.recv(Duration::ZERO).unwrap();
                    if out.is_empty() {
                        break;
                    }
                    device.write_all(&out).unwrap();
                }
            }
            kb
        });
        let mut c = Client::new(SerialTransport { port: Box::new(host) });
        assert_eq!(c.device_info().unwrap().name, "PTY board");
        c.set_binding(
            0,
            0,
            BehaviorBinding {
                behavior_id: 7,
                param1: 0x70029,
                param2: 0,
            },
        )
        .unwrap();
        assert_eq!(c.keymap().unwrap().layers[0].bindings[0].param1, 0x70029);
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let kb = dev.join().unwrap();
        assert!(kb.unsaved());
    }
}
