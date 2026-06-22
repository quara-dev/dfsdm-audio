/// Wire-protocol constants (must match firmware `main.c`).
const SYNC: [u8; 4] = [0xAA, 0x55, 0xAA, 0x55];
const BURST_SAMPLES: usize = 256;            // = AUDIO_HALF_SIZE in firmware (AUDIO_BUFFER_SIZE/2)
const BURST_BYTES: usize = BURST_SAMPLES * 2; // i16 LE

/// Stateful sync-frame parser for the DFSDM audio UART protocol.
///
/// Feed raw bytes via `push_bytes()` and drain complete 1024-sample bursts
/// via `drain_bursts()`.  Designed to be embedded inside the
/// `connection_manager` thread — no channels or I/O.
pub struct SyncFrameParser {
    /// Byte accumulator between `push_bytes` calls.
    acc: Vec<u8>,
    /// True after a sync word has been consumed; expecting payload bytes.
    synced: bool,
    /// Completed bursts ready for `drain_bursts()`.
    ready: Vec<Vec<i16>>,
}

impl SyncFrameParser {
    pub fn new() -> Self {
        Self {
            acc:    Vec::with_capacity(BURST_BYTES * 2 + 8),
            synced: false,
            ready:  Vec::new(),
        }
    }

    /// Append raw bytes from the serial port and parse any complete frames.
    pub fn push_bytes(&mut self, data: &[u8]) {
        self.acc.extend_from_slice(data);
        loop {
            if !self.synced {
                match find_sync(&self.acc) {
                    Some(pos) => {
                        // Discard everything before (and including) the sync word.
                        self.acc.drain(..pos + 4);
                        self.synced = true;
                    }
                    None => {
                        // Keep the last 3 bytes — sync word may straddle read boundary.
                        let keep = self.acc.len().saturating_sub(3);
                        self.acc.drain(..keep);
                        break;
                    }
                }
            }

            if self.acc.len() < BURST_BYTES {
                break; // payload incomplete; wait for more bytes
            }

            // Decode 1024 × little-endian i16 samples.
            let samples: Vec<i16> = self.acc[..BURST_BYTES]
                .chunks_exact(2)
                .map(|c| i16::from_le_bytes([c[0], c[1]]))
                .collect();
            self.acc.drain(..BURST_BYTES);
            self.synced = false; // hunt for next sync word

            self.ready.push(samples);
        }
    }

    /// Drain all completed bursts accumulated since the last call.
    pub fn drain_bursts(&mut self) -> impl Iterator<Item = Vec<i16>> + '_ {
        self.ready.drain(..)
    }
}

/// Returns the byte offset of the first `SYNC` pattern in `buf`.
fn find_sync(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == SYNC)
}
