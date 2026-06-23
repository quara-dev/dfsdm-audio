use std::f64::consts::PI;

use eframe::egui;
use egui::Color32;
use egui_plot::{Line, Plot, PlotPoints};
use rustfft::{num_complex::Complex, FftPlanner};

use super::{SplMode, ToolboxAlgo, ToolboxSignals, DBFS_TO_DBSPL};

pub struct Overview {
    log_freq: bool,
}

impl Overview {
    pub fn new() -> Self {
        Self { log_freq: false }
    }
}

impl ToolboxAlgo for Overview {
    fn name(&self) -> &str { "PSD (Welch)" }

    fn draw(&mut self, ui: &mut egui::Ui, signals: &ToolboxSignals<'_>) {
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.log_freq, "Log freq axis");
            ui.separator();
            ui.label(format!("Signal: {}", signals.source.label()));
        });

        let samples = signals.source_samples();
        if samples.len() < 64 {
            ui.label("Waiting for data…");
            return;
        }

        let (freqs, psd_dbfs) = welch_psd(&samples, signals.sample_rate);

        // Shift PSD to dBSPL if acoustic mode is active.
        // For a power spectral density expressed in 10·log10 scale, the same
        // additive offset (+120 dB) applies as for 20·log10 level values.
        let psd_display: Vec<f64> = match signals.spl_mode {
            SplMode::Acoustic => psd_dbfs.iter().map(|&v| v + DBFS_TO_DBSPL).collect(),
            SplMode::Digital  => psd_dbfs.clone(),
        };

        let pts: PlotPoints = freqs
            .iter()
            .zip(psd_display.iter())
            .skip(1) // skip DC bin
            .map(|(&f, &p)| {
                let x = if self.log_freq { f.max(1.0).log10() } else { f };
                [x, p]
            })
            .collect();

        let rms: f64 = (samples.iter().map(|&x| x * x).sum::<f64>() / samples.len() as f64).sqrt();
        let rms_dbfs = 20.0 * (rms / 32_767.0).max(1e-10_f64).log10();
        let rms_display = match signals.spl_mode {
            SplMode::Acoustic => rms_dbfs + DBFS_TO_DBSPL,
            SplMode::Digital  => rms_dbfs,
        };
        let unit = signals.level_label();

        Plot::new("overview_psd")
            .height(ui.available_height() - 24.0)
            .y_axis_label(unit)
            .x_axis_label(if self.log_freq { "log₁₀(Hz)" } else { "Hz" })
            .show(ui, |plot_ui| {
                plot_ui.line(
                    Line::new(pts)
                        .color(Color32::from_rgb(0x00, 0xB4, 0xD8))
                        .name("PSD"),
                );
            });

        ui.label(
            egui::RichText::new(format!("Window RMS: {:.1} {}", rms_display, unit))
                .small()
                .color(Color32::GRAY),
        );
    }
}

/// Welch's method: average power spectra over overlapping Hann-windowed segments.
/// Returns (frequency_hz[], power_dBFS[]).
fn welch_psd(samples: &[f64], sample_rate: f64) -> (Vec<f64>, Vec<f64>) {
    let n        = samples.len();
    let win_size = n.min(2048).next_power_of_two().min(2048);
    let hop      = win_size / 2;
    let n_bins   = win_size / 2 + 1;

    let hann: Vec<f64> = (0..win_size)
        .map(|i| 0.5 * (1.0 - (2.0 * PI * i as f64 / (win_size - 1) as f64).cos()))
        .collect();
    // Normalisation: sum of squared window coefficients
    let win_norm: f64 = hann.iter().map(|&w| w * w).sum();

    let mut planner = FftPlanner::new();
    let fft         = planner.plan_fft_forward(win_size);

    let mut power_acc = vec![0.0f64; n_bins];
    let mut n_frames  = 0usize;
    let mut pos       = 0;

    while pos + win_size <= n {
        let mut buf: Vec<Complex<f64>> = samples[pos..pos + win_size]
            .iter()
            .zip(hann.iter())
            .map(|(&s, &w)| Complex::new(s * w, 0.0))
            .collect();
        fft.process(&mut buf);
        for (acc, c) in power_acc.iter_mut().zip(buf[..n_bins].iter()) {
            *acc += c.norm_sqr();
        }
        n_frames += 1;
        pos      += hop;
    }

    if n_frames == 0 {
        return (Vec::new(), Vec::new());
    }

    let scale  = n_frames as f64 * win_norm;
    let fs_sq  = 32_767.0_f64.powi(2);
    let freqs  = (0..n_bins).map(|i| i as f64 * sample_rate / win_size as f64).collect();
    let psd_db = power_acc
        .iter()
        .map(|&p| 10.0 * (p / scale / fs_sq).max(1e-20_f64).log10())
        .collect();

    (freqs, psd_db)
}
