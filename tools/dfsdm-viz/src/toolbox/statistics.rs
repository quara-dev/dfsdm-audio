use std::f64::consts::PI;

use eframe::egui;
use egui::Color32;
use rustfft::{num_complex::Complex, FftPlanner};

use super::{SplMode, ToolboxAlgo, ToolboxSignals, DBFS_TO_DBSPL};

pub struct Statistics;

impl Statistics {
    pub fn new() -> Self { Self }
}

impl ToolboxAlgo for Statistics {
    fn name(&self) -> &str { "Statistics" }

    fn draw(&mut self, ui: &mut egui::Ui, signals: &ToolboxSignals<'_>) {
        let samples = signals.source_samples();
        let n = samples.len();
        if n < 2 {
            ui.label("Waiting for data…");
            return;
        }

        let mean = samples.iter().sum::<f64>() / n as f64;
        let var  = samples.iter().map(|&x| (x - mean).powi(2)).sum::<f64>() / n as f64;
        let std  = var.sqrt();
        let rms  = (samples.iter().map(|&x| x * x).sum::<f64>() / n as f64).sqrt();
        let peak = samples.iter().map(|&x| x.abs()).fold(0.0_f64, f64::max);

        let crest    = if rms > 0.0 { peak / rms }   else { 0.0 };
        let crest_db = if rms > 0.0 { 20.0 * crest.log10() } else { 0.0 };

        let rms_dbfs  = 20.0 * (rms  / 32_767.0).max(1e-10_f64).log10();
        let peak_dbfs = 20.0 * (peak / 32_767.0).max(1e-10_f64).log10();

        let skewness = if std > 0.0 {
            samples.iter().map(|&x| ((x - mean) / std).powi(3)).sum::<f64>() / n as f64
        } else { 0.0 };
        let kurtosis = if std > 0.0 {
            samples.iter().map(|&x| ((x - mean) / std).powi(4)).sum::<f64>() / n as f64 - 3.0
        } else { 0.0 };

        let dom_hz = dominant_frequency(&samples, signals.sample_rate);

        // ── Format level values depending on SPL mode ───────────────────────
        let (rms_str, peak_str) = match signals.spl_mode {
            SplMode::Acoustic => {
                let rms_spl  = rms_dbfs  + DBFS_TO_DBSPL;
                let peak_spl = peak_dbfs + DBFS_TO_DBSPL;
                (
                    format!("{:.1} dBSPL  ({:.1} dBFS)", rms_spl,  rms_dbfs),
                    format!("{:.1} dBSPL  ({:.1} dBFS)", peak_spl, peak_dbfs),
                )
            }
            SplMode::Digital => (
                format!("{:.1}  ({:.1} dBFS)", rms,  rms_dbfs),
                format!("{:.0}  ({:.1} dBFS)", peak, peak_dbfs),
            ),
        };

        // ── Grid ────────────────────────────────────────────────────────────
        egui::Grid::new("stats_grid")
            .num_columns(2)
            .spacing([12.0, 3.0])
            .striped(true)
            .show(ui, |ui| {
                macro_rules! row {
                    ($label:expr, $val:expr) => {{
                        ui.label(egui::RichText::new($label).strong());
                        ui.monospace($val);
                        ui.end_row();
                    }};
                }
                row!("Signal",          signals.source.label().to_string());
                row!("N (samples)",     format!("{}", n));

                if signals.is_acoustic() {
                    row!("Mode", "IM69D130 Acoustic (dBSPL)".to_string());
                }

                row!("Mean (counts)",   format!("{:.1}", mean));
                row!("Std dev (counts)",format!("{:.1}", std));
                row!("RMS",             rms_str);
                row!("Peak",            peak_str);
                row!("Crest factor",    format!("{:.2}  ({:.1} dB)",  crest, crest_db));
                row!("Skewness",        format!("{:.4}", skewness));
                row!("Excess kurtosis", format!("{:.4}", kurtosis));
                row!("Dominant freq",   format!("{:.1} Hz", dom_hz));

                if signals.is_acoustic() {
                    ui.end_row();
                    ui.label(egui::RichText::new("─── IM69D130 Reference ───").weak());
                    ui.label("");
                    ui.end_row();
                    row!("Sensitivity", "−26 dBFS @ 94 dBSPL (1 kHz)".to_string());
                    row!("AOP",         "120 dBSPL (= 0 dBFS)".to_string());
                    row!("Noise floor", "~25 dBSPL(A)  (−95 dBFS(A))".to_string());
                    row!("Offset",      "dBSPL = dBFS + 120".to_string());
                }
            });

        // Amplitude histogram
        ui.separator();
        ui.label(egui::RichText::new("Amplitude distribution").strong());

        const N_BINS: usize = 48;
        let bin_size = 65536.0 / N_BINS as f64;
        let mut hist = [0usize; N_BINS];
        for &x in &samples {
            let idx = ((x + 32768.0) / bin_size) as usize;
            if idx < N_BINS { hist[idx] += 1; }
        }
        let hist_max = *hist.iter().max().unwrap_or(&1) as f32;

        let h = (ui.available_height() - 4.0).max(40.0);
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), h),
            egui::Sense::hover(),
        );
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 2.0, Color32::from_rgb(20, 20, 30));

        // X-axis tick labels: either in ADC counts or dBSPL
        let bar_w = rect.width() / N_BINS as f32;
        for (i, &count) in hist.iter().enumerate() {
            let bar_h = (count as f32 / hist_max) * rect.height();
            let x     = rect.left() + i as f32 * bar_w;
            let y     = rect.bottom() - bar_h;
            painter.rect_filled(
                egui::Rect::from_min_size(
                    egui::pos2(x, y),
                    egui::vec2((bar_w - 0.5).max(1.0), bar_h),
                ),
                0.0,
                Color32::from_rgb(0x00, 0xB4, 0xD8),
            );
        }

        // Axis tick labels
        let tick_positions: &[(usize, &str)] = if signals.is_acoustic() {
            // Map rough dBSPL values back to bin indices:
            // bin_centre_count = (bin_idx + 0.5) * bin_size - 32768
            // dBFS = 20·log10(|count| / 32767);  dBSPL = dBFS + 120
            // Mark a few round dBSPL values
            &[
                (0,  "25"),
                (6,  "50"),
                (12, "70"),
                (18, "85"),
                (24, "94"),  // calibration ref
                (30, "100"),
                (36, "110"),
                (42, "115"),
                (47, "120"),
            ]
        } else {
            &[
                (0,  "-∞"),
                (12, "-40k"),
                (24, "0"),
                (36, "+20k"),
                (47, "+32k"),
            ]
        };
        for &(bin, label) in tick_positions {
            let x = rect.left() + bin as f32 * bar_w + bar_w / 2.0;
            painter.text(
                egui::pos2(x, rect.bottom() - 2.0),
                egui::Align2::CENTER_BOTTOM,
                label,
                egui::FontId::proportional(8.0),
                Color32::from_rgb(180, 180, 180),
            );
        }

        if signals.is_acoustic() {
            // Vertical marker at calibration reference (94 dBSPL)
            let ref_bin = 24usize;
            let ref_x   = rect.left() + ref_bin as f32 * bar_w + bar_w / 2.0;
            painter.line_segment(
                [egui::pos2(ref_x, rect.top()), egui::pos2(ref_x, rect.bottom())],
                egui::Stroke::new(1.0, Color32::from_rgb(255, 180, 0)),
            );
            painter.text(
                egui::pos2(ref_x + 2.0, rect.top() + 2.0),
                egui::Align2::LEFT_TOP,
                "94 dBSPL",
                egui::FontId::proportional(8.0),
                Color32::from_rgb(255, 180, 0),
            );
        }
    }
}

fn dominant_frequency(samples: &[f64], sample_rate: f64) -> f64 {
    let n = samples.len().min(4096).next_power_of_two();
    if n < 4 { return 0.0; }

    let hann: Vec<f64> = (0..n)
        .map(|i| 0.5 * (1.0 - (2.0 * PI * i as f64 / (n - 1) as f64).cos()))
        .collect();

    let mut planner = FftPlanner::new();
    let fft         = planner.plan_fft_forward(n);
    let mut buf: Vec<Complex<f64>> = samples[..n]
        .iter()
        .zip(hann.iter())
        .map(|(&s, &w)| Complex::new(s * w, 0.0))
        .collect();
    fft.process(&mut buf);

    // Skip DC (bin 0)
    let (max_bin, _) = buf[1..n / 2]
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| {
            a.norm_sqr()
                .partial_cmp(&b.norm_sqr())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or((0, &Complex::new(0.0, 0.0)));

    (max_bin + 1) as f64 * sample_rate / n as f64
}
