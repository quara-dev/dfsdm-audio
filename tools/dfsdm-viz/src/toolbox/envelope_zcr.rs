use eframe::egui;
use egui::Color32;
use egui_plot::{Line, Plot, PlotPoints};

use super::{SplMode, ToolboxAlgo, ToolboxSignals, DBFS_TO_DBSPL};

pub struct EnvelopeZcr {
    frame_ms:   f32,
    show_zcr:   bool,
}

impl EnvelopeZcr {
    pub fn new() -> Self { Self { frame_ms: 20.0, show_zcr: true } }
}

impl ToolboxAlgo for EnvelopeZcr {
    fn name(&self) -> &str { "Envelope / ZCR" }

    fn draw(&mut self, ui: &mut egui::Ui, signals: &ToolboxSignals<'_>) {
        ui.horizontal(|ui| {
            ui.label("Frame:");
            ui.add(
                egui::DragValue::new(&mut self.frame_ms)
                    .speed(1.0)
                    .clamp_range(5.0f32..=100.0f32)
                    .suffix(" ms"),
            );
            ui.checkbox(&mut self.show_zcr, "Show ZCR");
            ui.separator();
            ui.label(format!("Signal: {}", signals.source.label()));
        });

        let samples = signals.source_samples();
        if samples.len() < 64 {
            ui.label("Waiting for data…");
            return;
        }

        let frame_len  = ((self.frame_ms as f64 / 1000.0) * signals.sample_rate) as usize;
        let frame_len  = frame_len.max(16);
        let ms_per_sample = 1000.0 / signals.sample_rate;

        let (env_pts, zcr_pts) = compute_envelope_zcr(
            &samples,
            frame_len,
            ms_per_sample,
            signals.spl_mode,
        );

        let avail_h = ui.available_height();
        let (env_h, zcr_h) = if self.show_zcr {
            (avail_h * 0.6, avail_h * 0.4)
        } else {
            (avail_h, 0.0)
        };

        let env_label = match signals.spl_mode {
            SplMode::Acoustic => "dBSPL",
            SplMode::Digital  => "RMS (norm)",
        };

        // --- RMS Envelope ---
        Plot::new("env_plot")
            .height(env_h)
            .y_axis_label(env_label)
            .x_axis_label("Time (ms)")
            .show(ui, |plot_ui| {
                plot_ui.line(
                    Line::new(PlotPoints::new(env_pts))
                        .color(Color32::from_rgb(0x00, 0xB4, 0xD8))
                        .name("RMS Envelope"),
                );
            });

        // --- Zero-Crossing Rate ---
        if self.show_zcr {
            Plot::new("zcr_plot")
                .height(zcr_h)
                .y_axis_label("ZCR (per ms)")
                .x_axis_label("Time (ms)")
                .show(ui, |plot_ui| {
                    plot_ui.line(
                        Line::new(PlotPoints::new(zcr_pts))
                            .color(Color32::from_rgb(0xFF, 0xA0, 0x00))
                            .name("ZCR"),
                    );
                });
        }
    }
}

fn compute_envelope_zcr(
    samples: &[f64],
    frame_len: usize,
    ms_per_sample: f64,
    spl_mode: SplMode,
) -> (Vec<[f64; 2]>, Vec<[f64; 2]>) {
    let n = samples.len();
    let n_frames = n / frame_len;
    let mut env_pts = Vec::with_capacity(n_frames);
    let mut zcr_pts = Vec::with_capacity(n_frames);

    for f in 0..n_frames {
        let start = f * frame_len;
        let frame = &samples[start..start + frame_len];

        // RMS of this frame (in ADC counts)
        let rms_counts =
            (frame.iter().map(|&x| x * x).sum::<f64>() / frame_len as f64).sqrt();

        // Y value depends on mode:
        //   Digital:  normalised 0..1  (rms_counts / 32767)
        //   Acoustic: dBSPL           (20·log10(rms/32767) + 120)
        let env_y = match spl_mode {
            SplMode::Digital  => rms_counts / 32_767.0,
            SplMode::Acoustic => {
                let dbfs = 20.0 * (rms_counts / 32_767.0).max(1e-10).log10();
                dbfs + DBFS_TO_DBSPL
            }
        };

        // Zero-crossing rate: crossings per ms
        let mut zc = 0usize;
        for i in 1..frame.len() {
            if (frame[i - 1] >= 0.0) != (frame[i] >= 0.0) {
                zc += 1;
            }
        }
        let zcr_per_ms = zc as f64 / (frame_len as f64 * ms_per_sample);

        let t_ms = (start as f64 + frame_len as f64 / 2.0) * ms_per_sample;
        env_pts.push([t_ms, env_y]);
        zcr_pts.push([t_ms, zcr_per_ms]);
    }

    (env_pts, zcr_pts)
}
