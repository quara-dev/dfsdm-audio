use crossbeam_channel::{Receiver, Sender};
use serialport::SerialPort;
use std::io::Read;
use std::time::{Duration, Instant};

use crate::events::{AppEvent, ConnCmd, PortInfo};
use crate::serial_reader::SyncFrameParser;

/// Map a `serialport::SerialPortInfo` to our display-ready `PortInfo`.
/// `pub` so `app.rs` can reuse it when spawning the port-refresh thread.
pub fn port_info_from(info: serialport::SerialPortInfo) -> PortInfo {
    let description = match info.port_type {
        serialport::SerialPortType::UsbPort(usb) => {
            usb.product.or(usb.manufacturer).unwrap_or_default()
        }
        _ => String::new(),
    };
    PortInfo { name: info.port_name, description }
}

/// Long-lived thread that manages the serial port connection lifecycle.
///
/// State machine:
///   IDLE    — blocking `recv()` on `cmd_rx`, waiting for `Connect`.
///   READING — non-blocking `try_recv()` + `port.read()` in a tight loop.
///
/// Worst-case reaction time to `Disconnect` = 100 ms (the port read timeout).
pub fn connection_manager_thread(
    cmd_rx:   Receiver<ConnCmd>,
    event_tx: Sender<AppEvent>,
) {
    'outer: loop {
        // ── IDLE: block until a Connect command arrives ────────────────────
        let (port_name, baud) = loop {
            match cmd_rx.recv() {
                Ok(ConnCmd::Connect(name, baud)) => break (name, baud),
                Ok(ConnCmd::Disconnect) => {} // already disconnected, ignore
                Err(_) => return,             // channel closed (app exited)
            }
        };

        // Try to open the requested port
        log::info!("Opening {} @ {} baud", port_name, baud);
        let mut port: Box<dyn SerialPort> = match serialport::new(&port_name, baud)
            .timeout(Duration::from_millis(100))
            .open()
        {
            Ok(p) => p,
            Err(e) => {
                let msg = format!("Failed to open {}: {}", port_name, e);
                log::error!("{}", msg);
                let _ = event_tx.send(AppEvent::Error(msg));
                let _ = event_tx.send(AppEvent::Disconnected);
                continue; // back to IDLE
            }
        };

        let _ = event_tx.send(AppEvent::Connected);
        let mut parser = SyncFrameParser::new();
        let mut buf = [0u8; 4096];

        // ── READING loop ────────────────────────────────────────────────────
        loop {
            // Check for commands (non-blocking — port.read has 100 ms timeout)
            match cmd_rx.try_recv() {
                Ok(ConnCmd::Disconnect) => {
                    log::info!("Disconnect requested, closing port.");
                    let _ = event_tx.send(AppEvent::Disconnected);
                    continue 'outer;
                }
                Ok(ConnCmd::Connect(new_name, new_baud)) => {
                    // Port switch: close old, open new immediately.
                    let _ = event_tx.send(AppEvent::Disconnected);
                    log::info!("Switching to {} @ {} baud", new_name, new_baud);
                    match serialport::new(&new_name, new_baud)
                        .timeout(Duration::from_millis(100))
                        .open()
                    {
                        Ok(new_port) => {
                            port   = new_port;
                            parser = SyncFrameParser::new();
                            let _ = event_tx.send(AppEvent::Connected);
                        }
                        Err(e) => {
                            let msg = format!("Failed to open {}: {}", new_name, e);
                            log::error!("{}", msg);
                            let _ = event_tx.send(AppEvent::Error(msg));
                            let _ = event_tx.send(AppEvent::Disconnected);
                            continue 'outer;
                        }
                    }
                }
                Err(crossbeam_channel::TryRecvError::Empty) => {}
                Err(crossbeam_channel::TryRecvError::Disconnected) => return,
            }

            // Read data from the port
            match port.read(&mut buf) {
                Ok(0) => {}
                Ok(n) => {
                    parser.push_bytes(&buf[..n]);
                    for burst in parser.drain_bursts() {
                        if event_tx
                            .send(AppEvent::Samples {
                                raw:        burst,
                                arrived_at: Instant::now(),
                            })
                            .is_err()
                        {
                            log::info!("Event channel closed, connection manager exiting.");
                            return;
                        }
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {}
                Err(e) => {
                    let msg = format!("Serial read error on {}: {}", port_name, e);
                    log::error!("{}", msg);
                    let _ = event_tx.send(AppEvent::Error(msg));
                    let _ = event_tx.send(AppEvent::Disconnected);
                    continue 'outer;
                }
            }
        }
    }
}
