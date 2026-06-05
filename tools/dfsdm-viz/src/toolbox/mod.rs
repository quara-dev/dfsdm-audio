pub mod autocorr;
pub mod envelope_zcr;
pub mod overview;
pub mod spectrogram;
pub mod statistics;

use eframe::egui;

/// Visible time window shown in the main plot and fed to toolbox algorithms.
pub const WINDOW_S: f64 = 3.0;

/// Which audio channel is passed to the active toolbox algorithm.
#[derive(PartialEq, Clone, Copy)]
pub enum ToolboxSource {
    Raw,
    Hpf,
}

impl ToolboxSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Raw => "Raw",
            Self::Hpf => "HPF",
        }
    }
}

/// Data bundle passed to every toolbox algorithm on each draw call.
///
/// Both `raw` and `hpf` slices cover the currently-visible time window.
/// Each entry is `[time_s, sample_value]`.
pub struct ToolboxSignals<'a> {
    pub raw:         &'a [[f64; 2]],
    pub hpf:         &'a [[f64; 2]],
    pub sample_rate: f64,
    pub x_min:       f64,
    pub x_max:       f64,
    pub source:      ToolboxSource,
}

impl<'a> ToolboxSignals<'a> {
    /// Returns just the sample values (no timestamps) of the selected channel.
    pub fn source_samples(&self) -> Vec<f64> {
        let buf = match self.source {
            ToolboxSource::Raw => self.raw,
            ToolboxSource::Hpf => self.hpf,
        };
        buf.iter().map(|p| p[1]).collect()
    }
}

/// Trait implemented by every analysis algorithm shown in the toolbox panel.
pub trait ToolboxAlgo {
    fn name(&self) -> &str;
    fn draw(&mut self, ui: &mut egui::Ui, signals: &ToolboxSignals<'_>);
}

/// Instantiate all built-in toolbox algorithms in display order.
pub fn create_algos() -> Vec<Box<dyn ToolboxAlgo>> {
    vec![
        Box::new(overview::Overview::new()),
        Box::new(spectrogram::Spectrogram::new()),
        Box::new(statistics::Statistics::new()),
        Box::new(autocorr::Autocorr::new()),
        Box::new(envelope_zcr::EnvelopeZcr::new()),
    ]
}
