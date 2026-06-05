use eframe::egui;
use egui::Color32;
use egui_plot::{Line, Plot, PlotPoints};

use super::{ToolboxAlgo, ToolboxSignals};

pub struct Autocorr {
    max_lag_ms: f32,
}

impl Autocorr {
    pub fn new() -> Self { Self { max_lag_ms: 50.0 } }
}

impl ToolboxAlgo for Autocorr {
    fn name(&self) -> &str { "Autocorrelation" }

    fn draw(&mut self, ui: &mut egui::Ui, signals: &ToolboxSignals<'_>) {
        ui.horizontal(|ui| {
            ui.label("Max lag:");
            ui.add(
                egui::DragValue::new(&mut self.max_lag_ms)
                    .speed(1.0)
                    .clamp_range(5.0f32..=500.0f32)
                    .suffix(" ms"),
            );
            ui.separator();
            ui.label(format!("Signal: {}", signals.source.label()));
        });

        let samples = signals.source_samples();
        if samples.len() < 64 {
            ui.label("Waiting for data…");
            return;
        }

        let max_lag = ((self.max_lag_ms as f64 / 1000.0) * signals.sample_rate) as usize;
        let max_lag = max_lag.min(samples.len() / 2).max(1);

        let ac = autocorrelation(&samples, max_lag);
        let ms_per_sample = 1000.0 / signals.sample_rate;

        let pts: PlotPoints = ac
            .iter()
            .enumerate()
            .map(|(i, &v)| [i as f64 * ms_per_sample, v])
            .collect();

        Plot::new("autocorr_plot")
            .height(ui.available_height())
            .y_axis_label("Correlation")
            .x_axis_label("Lag (ms)")
            .show(ui, |plot_ui| {
                plot_ui.line(
                    Line::new(pts)
                        .color(Color32::from_rgb(0xFF, 0x6D, 0x00))
                        .name("Autocorrelation"),
                );
            });
    }
}

/// Normalised autocorrelation up to `max_lag` samples (lag=0 → 1.0).
fn autocorrelation(samples: &[f64], max_lag: usize) -> Vec<f64> {
    let n    = samples.len();
    let mean = samples.iter().sum::<f64>() / n as f64;
    let det: Vec<f64> = samples.iter().map(|&x| x - mean).collect();
    let norm = det.iter().map(|&x| x * x).sum::<f64>();

    if norm == 0.0 {
        return vec![0.0; max_lag + 1];
    }

    (0..=max_lag)
        .map(|lag| {
            det[..n - lag]
                .iter()
                .zip(det[lag..].iter())
                .map(|(&a, &b)| a * b)
                .sum::<f64>()
                / norm
        })
        .collect()
}
