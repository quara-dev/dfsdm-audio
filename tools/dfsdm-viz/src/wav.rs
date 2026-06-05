use std::path::{Path, PathBuf};

/// Writes incoming int16 mono PCM samples to a WAV file using `hound`.
///
/// Call `start()` to open a new file, `write_samples()` on every burst, and
/// `stop()` to finalise and close.  The struct is zero-cost when idle
/// (`is_recording()` returns false).
pub struct WavRecorder {
    writer:      Option<hound::WavWriter<std::io::BufWriter<std::fs::File>>>,
    path:        PathBuf,
    sample_rate: u32,
}

impl WavRecorder {
    pub fn new() -> Self {
        Self {
            writer:      None,
            path:        PathBuf::new(),
            sample_rate: 32_000,
        }
    }

    /// Open `path` and begin recording at `sample_rate` Hz (mono, 16-bit PCM).
    /// Returns the filename (not the full path) on success.
    pub fn start(&mut self, path: &Path, sample_rate: u32) -> std::io::Result<String> {
        let spec = hound::WavSpec {
            channels:        1,
            sample_rate,
            bits_per_sample: 16,
            sample_format:   hound::SampleFormat::Int,
        };
        self.writer = Some(
            hound::WavWriter::create(path, spec)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?,
        );
        self.path        = path.to_path_buf();
        self.sample_rate = sample_rate;
        Ok(path.file_name().unwrap_or_default().to_string_lossy().into_owned())
    }

    /// Append a block of samples to the open WAV file.  No-op if not recording.
    pub fn write_samples(&mut self, samples: &[i16]) {
        if let Some(ref mut w) = self.writer {
            for &s in samples {
                // hound returns an error only on I/O failure; ignore silently.
                let _ = w.write_sample(s);
            }
        }
    }

    /// Finalise (flush + write WAV header size) and close.
    /// Returns the filename on success, `None` if not recording.
    pub fn stop(&mut self) -> Option<String> {
        if let Some(w) = self.writer.take() {
            let _ = w.finalize();
            Some(
                self.path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
            )
        } else {
            None
        }
    }

    pub fn is_recording(&self) -> bool { self.writer.is_some() }

    pub fn full_path(&self) -> &str { self.path.to_str().unwrap_or("") }
}
