use std::time::Instant;

/// Serial port information ready for display in the UI.
#[derive(Debug, Clone)]
pub struct PortInfo {
    pub name:        String,
    pub description: String, // empty if unavailable
}

/// Commands sent from the GUI to the connection manager thread.
#[derive(Debug)]
pub enum ConnCmd {
    Connect(String, u32), // port name, baud rate
    Disconnect,
}

/// Events sent from the connection manager / workers to the GUI.
#[derive(Debug)]
pub enum AppEvent {
    /// One burst of raw audio samples, timestamped at arrival.
    Samples { raw: Vec<i16>, arrived_at: Instant },
    /// Error message to display in the toolbar.
    Error(String),
    /// Connection manager successfully opened the port.
    Connected,
    /// Connection manager closed the port (commanded or on error).
    Disconnected,
    /// Result of an asynchronous port scan.
    PortsAvailable(Vec<PortInfo>),
    /// WAV file was finalised successfully (filename only, not full path).
    WavSaved(String),
}
