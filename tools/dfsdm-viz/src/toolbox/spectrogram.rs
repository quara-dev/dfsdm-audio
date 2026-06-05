use std::f64::consts::PI;

use eframe::egui;
use egui::Color32;
use rustfft::{num_complex::Complex, FftPlanner};

use super::{ToolboxAlgo, ToolboxSignals};

pub struct Spectrogram {
    db_min:  f32,
    db_max:  f32,
    texture: Option<egui::TextureHandle>,
}

impl Spectrogram {
    pub fn new() -> Self {
        Self { db_min: -60.0, db_max: 0.0, texture: None }
    }
}

impl ToolboxAlgo for Spectrogram {
    fn name(&self) -> &str { "Spectrogram" }

    fn draw(&mut self, ui: &mut egui::Ui, signals: &ToolboxSignals<'_>) {
        // Controls
        ui.horizontal(|ui| {
            ui.label("dB min:");
            ui.add(egui::DragValue::new(&mut self.db_min).speed(1.0).clamp_range(-120.0f32..=-10.0f32));
            ui.label("max:");
            ui.add(egui::DragValue::new(&mut self.db_max).speed(1.0).clamp_range(-40.0f32..=0.0f32));
            ui.separator();
            ui.label(format!("Signal: {}", signals.source.label()));
        });

        let samples = signals.source_samples();
        if samples.len() < 256 {
            ui.label("Waiting for data…");
            return;
        }

        // Target ~150 time columns; adjust hop accordingly.
        let fft_size   = 256usize;
        let target_cols = 150usize;
        let hop = (samples.len() / target_cols).max(fft_size / 4).max(1);

        let stft = compute_stft(&samples, fft_size, hop);
        if stft.is_empty() {
            return;
        }

        let n_cols = stft.len();
        let n_rows = fft_size / 2; // bins 1..=n_rows (skip DC at 0)

        // Build a ColourImage (n_cols × n_rows, origin = top-left, row 0 = high freq)
        let db_range = (self.db_max - self.db_min) as f64;
        let mut pixels: Vec<Color32> = Vec::with_capacity(n_cols * n_rows);

        for row in 0..n_rows {
            let bin = n_rows - row; // row 0 → high freq bin, row n_rows-1 → low freq
            for frame in &stft {
                let power = if bin < frame.len() { frame[bin] } else { 0.0 };
                let db    = 10.0 * (power.max(1e-20_f64) / 32_767.0_f64.powi(2)).log10();
                let norm  = ((db - self.db_min as f64) / db_range).clamp(0.0, 1.0) as f32;
                pixels.push(inferno(norm));
            }
        }

        let image   = egui::ColorImage { size: [n_cols, n_rows], pixels };
        self.texture = Some(ui.ctx().load_texture(
            "dfsdm_spectrogram",
            image,
            egui::TextureOptions::LINEAR,
        ));

        // Display frequency labels on the right side (top = high Hz, bottom = low Hz)
        let avail      = ui.available_size();
        let panel_rect = ui.max_rect();

        // Draw texture filling available area
        if let Some(ref tex) = self.texture {
            ui.add(
                egui::Image::new(egui::load::SizedTexture::new(tex.id(), avail))
                    .fit_to_exact_size(avail),
            );
        }

        // Overlay frequency axis ticks via painter
        let painter = ui.painter_at(panel_rect);
        let sr      = signals.sample_rate;
        let nyq     = sr / 2.0;
        for label_hz in [500.0, 1000.0, 2000.0, 4000.0, 8000.0, 12000.0, 16000.0_f64] {
            if label_hz >= nyq { continue; }
            // Fraction from top (high-freq) to bottom (low-freq).
            let frac = 1.0 - label_hz / nyq;
            let y = panel_rect.top() + frac as f32 * panel_rect.height();
            painter.line_segment(
                [egui::pos2(panel_rect.right() - 12.0, y), egui::pos2(panel_rect.right(), y)],
                egui::Stroke::new(1.0, Color32::WHITE),
            );
            painter.text(
                egui::pos2(panel_rect.right() - 14.0, y - 7.0),
                egui::Align2::RIGHT_TOP,
                format!("{:.0}Hz", label_hz),
                egui::FontId::proportional(9.0),
                Color32::WHITE,
            );
        }
    }
}

/// Short-time Fourier transform.  Returns power spectra (magnitude²) per frame.
fn compute_stft(samples: &[f64], fft_size: usize, hop: usize) -> Vec<Vec<f64>> {
    let n = samples.len();
    if n < fft_size {
        return Vec::new();
    }

    let hann: Vec<f64> = (0..fft_size)
        .map(|i| 0.5 * (1.0 - (2.0 * PI * i as f64 / (fft_size - 1) as f64).cos()))
        .collect();

    let mut planner = FftPlanner::new();
    let fft         = planner.plan_fft_forward(fft_size);
    let n_bins      = fft_size / 2 + 1;
    let mut result  = Vec::new();
    let mut pos     = 0;

    while pos + fft_size <= n {
        let mut buf: Vec<Complex<f64>> = samples[pos..pos + fft_size]
            .iter()
            .zip(hann.iter())
            .map(|(&s, &w)| Complex::new(s * w, 0.0))
            .collect();
        fft.process(&mut buf);
        result.push(buf[..n_bins].iter().map(|c| c.norm_sqr()).collect());
        pos += hop;
    }

    result
}

/// Simplified inferno colormap: black → purple → red → orange → white.
fn inferno(t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let (r, g, b) = if t < 0.25 {
        let s = t / 0.25;
        (lerp(0.0, 80.0, s), 0.0, lerp(10.0, 100.0, s))
    } else if t < 0.5 {
        let s = (t - 0.25) / 0.25;
        (lerp(80.0, 210.0, s), lerp(0.0, 10.0, s), lerp(100.0, 50.0, s))
    } else if t < 0.75 {
        let s = (t - 0.5) / 0.25;
        (lerp(210.0, 255.0, s), lerp(10.0, 150.0, s), lerp(50.0, 0.0, s))
    } else {
        let s = (t - 0.75) / 0.25;
        (255.0, lerp(150.0, 255.0, s), lerp(0.0, 255.0, s))
    };
    Color32::from_rgb(r as u8, g as u8, b as u8)
}

#[inline]
fn lerp(a: f32, b: f32, t: f32) -> f32 { a + (b - a) * t }
