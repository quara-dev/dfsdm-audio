pub mod autocorr;
pub mod envelope_zcr;
pub mod overview;
pub mod spectrogram;
pub mod statistics;

use eframe::egui;

/// Visible time window shown in the main plot and fed to toolbox algorithms.
pub const WINDOW_S: f64 = 3.0;

// ── IM69D130 calibration constants ────────────────────────────────────────
//
// Source: Infineon IM69D130 datasheet, Table 1 "Acoustic specifications"
//   Sensitivity (typ):   -26 dBFS  @ 1 kHz, 94 dBSPL
//   Acoustic Overload:   120 dBSPL (= 0 dBFS, consistent: 94 + 26 = 120)
//   Noise Floor:         -95 dBFS(A) @ f_clk = 3.072 MHz  → 25 dBSPL(A)
//   Dynamic range:       95 dB
//
// Conversion:  dBSPL = dBFS + DBFS_TO_DBSPL
//              dBSPL = dBFS + 120
//
// For power quantities (10·log10 instead of 20·log10):
//              dBSPL_power = dBFS_power + DBFS_TO_DBSPL
// (same offset because it cancels through the 10× vs 20× factor)

/// IM69D130 typical sensitivity: -26 dBFS at 94 dBSPL / 1 kHz.
pub const MIC_SENSITIVITY_DBFS: f64 = -26.0;

/// IEC 60268-4 standard reference level for microphone sensitivity measurement.
pub const MIC_REFERENCE_DBSPL: f64 = 94.0;

/// dBFS → dBSPL offset: add this to any dBFS value to obtain dBSPL.
/// = MIC_REFERENCE_DBSPL − MIC_SENSITIVITY_DBFS = 94 − (−26) = 120 dB.
pub const DBFS_TO_DBSPL: f64 = MIC_REFERENCE_DBSPL - MIC_SENSITIVITY_DBFS; // 120.0

/// Acoustic overload point (AOP): the SPL at which the microphone reaches 0 dBFS (THD = 10 %).
pub const MIC_AOP_DBSPL: f64 = 120.0;

/// Equivalent noise level (A-weighted) derived from the datasheet noise floor of −95 dBFS(A).
pub const MIC_NOISE_FLOOR_DBSPL: f64 = 25.0; // = -95 + 120

/// Which audio channel is passed to the active toolbox algorithm.
#[derive(PartialEq, Clone, Copy)]
pub enum ToolboxSource {
    Raw,
    Hpf,
}

/// Whether to display values in engineering units (dBSPL / Pa) using the
/// IM69D130 datasheet calibration, or in raw digital units (dBFS / counts).
#[derive(PartialEq, Clone, Copy)]
pub enum SplMode {
    /// Raw digital values: dBFS, ADC counts.
    Digital,
    /// Calibrated acoustic values: dBSPL using IM69D130 datasheet sensitivity.
    Acoustic,
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
    /// When `Acoustic`, all level displays use dBSPL / SPL (IM69D130 calibration).
    /// When `Digital`, displays use dBFS / ADC counts.
    pub spl_mode:    SplMode,
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

    /// Convert a dBFS value to dBSPL when acoustic mode is active, else return unchanged.
    #[inline]
    pub fn dbfs_to_display(&self, dbfs: f64) -> f64 {
        match self.spl_mode {
            SplMode::Acoustic => dbfs + DBFS_TO_DBSPL,
            SplMode::Digital  => dbfs,
        }
    }

    /// Y-axis label for level (dB) plots.
    pub fn level_label(&self) -> &'static str {
        match self.spl_mode {
            SplMode::Acoustic => "dBSPL",
            SplMode::Digital  => "dBFS",
        }
    }

    /// Short unit suffix for inline text.
    pub fn level_unit(&self) -> &'static str {
        match self.spl_mode {
            SplMode::Acoustic => "dBSPL",
            SplMode::Digital  => "dBFS",
        }
    }

    /// Returns true when acoustic calibration is active.
    pub fn is_acoustic(&self) -> bool {
        self.spl_mode == SplMode::Acoustic
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
