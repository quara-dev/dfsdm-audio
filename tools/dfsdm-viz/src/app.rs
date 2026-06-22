use std::collections::VecDeque;

use chrono::Local;
use crossbeam_channel::{Receiver, Sender};
use eframe::egui;
use egui::Color32;
use egui_plot::{Line, Plot, PlotBounds, PlotPoints};

use crate::connection_manager::port_info_from;
use crate::events::{AppEvent, ConnCmd, PortInfo};
use crate::hpf::AudioHpFilter;
use crate::toolbox::{self, ToolboxAlgo, ToolboxSignals, ToolboxSource, WINDOW_S};
use crate::wav::WavRecorder;

// ── Plot decimation ────────────────────────────────────────────────────────
const TARGET_PLOT_PTS: usize = 3_000;

// ── Ring buffer depth ──────────────────────────────────────────────────────
const HISTORY_S: f64 = 30.0; // seconds of audio kept in RAM

/// Clamp a requested view range to the data retained in the ring buffer.
///
/// - Never pans past `t_s` (no future).
/// - Never pans before the oldest retained sample (`t_s - history_s`, floored at 0).
/// - Caps the window width at the available span; preserves width otherwise.
fn clamp_view_to_history(x_min: f64, x_max: f64, t_s: f64, history_s: f64) -> (f64, f64) {
    let newest = t_s;
    let oldest = (t_s - history_s).max(0.0);
    let span   = (newest - oldest).max(1e-6);

    let width = (x_max - x_min).max(1e-6).min(span);
    let mut lo = x_min;
    let mut hi = x_max;

    if hi > newest {
        hi = newest;
        lo = hi - width;
    }
    if lo < oldest {
        lo = oldest;
        hi = lo + width;
    }
    if hi > newest {
        hi = newest;
    }
    (lo, hi)
}

// ── Signal colours ─────────────────────────────────────────────────────────
const COL_RAW: Color32 = Color32::from_rgb(0x00, 0xB4, 0xD8); // cyan-blue
const COL_HPF: Color32 = Color32::from_rgb(0xFF, 0x6D, 0x00); // amber-orange

/// Which waveform lines to draw in the audio plot.
#[derive(PartialEq, Clone, Copy)]
enum AudioDisplay {
    Raw,
    Hpf,
    Both,
}

pub struct DfsdmApp {
    // ── Channels ────────────────────────────────────────────────────────────
    rx:       Receiver<AppEvent>,
    cmd_tx:   Sender<ConnCmd>,
    event_tx: Sender<AppEvent>, // used to inject port-scan results

    // ── Connection ──────────────────────────────────────────────────────────
    connected:         bool,
    port_scanning:     bool,
    available_ports:   Vec<PortInfo>,
    selected_port_idx: Option<usize>,
    port_connected:    String,
    baud:              u32,
    error_msg:         Option<String>,

    // ── Audio state ─────────────────────────────────────────────────────────
    sample_rate: u32,
    hpf:         AudioHpFilter,
    raw_buf:     VecDeque<[f64; 2]>,
    hpf_buf:     VecDeque<[f64; 2]>,
    buf_cap:     usize,
    t_s:         f64, // monotonic sample clock

    // ── Statistics ──────────────────────────────────────────────────────────
    burst_count:    u64,
    total_samples:  u64,
    rms_smooth:     f32, // fast-attack / slow-decay

    // ── Display ─────────────────────────────────────────────────────────────
    audio_display: AudioDisplay,
    y_scale:       f64,

    // ── WAV recording ───────────────────────────────────────────────────────
    wav:        WavRecorder,
    wav_folder: String,
    wav_status: String, // one-line status shown in toolbar

    // ── Toolbox ─────────────────────────────────────────────────────────────
    show_toolbox:      bool,
    toolbox_algos:     Vec<Box<dyn ToolboxAlgo>>,
    selected_algo_idx: usize,
    toolbox_source:    ToolboxSource,
}

impl DfsdmApp {
    pub fn new(
        rx:       Receiver<AppEvent>,
        cmd_tx:   Sender<ConnCmd>,
        event_tx: Sender<AppEvent>,
        port:     Option<String>,
        baud:     u32,
        sample_rate: u32,
    ) -> Self {
        let rate = if sample_rate == 0 { 32_000 } else { sample_rate };
        let cap  = (HISTORY_S * rate as f64) as usize;

        // Kick off an initial port scan.
        {
            let tx = event_tx.clone();
            std::thread::spawn(move || {
                let raw = serialport::available_ports().unwrap_or_default();
                let ports: Vec<PortInfo> = raw.into_iter().map(port_info_from).collect();
                let _ = tx.send(AppEvent::PortsAvailable(ports));
            });
        }

        let mut app = Self {
            rx,
            cmd_tx,
            event_tx,
            connected:         false,
            port_scanning:     true,
            available_ports:   Vec::new(),
            selected_port_idx: None,
            port_connected:    String::new(),
            baud,
            error_msg:         None,
            sample_rate:       rate,
            hpf:               AudioHpFilter::new(),
            raw_buf:           VecDeque::with_capacity(cap + 1024),
            hpf_buf:           VecDeque::with_capacity(cap + 1024),
            buf_cap:           cap,
            t_s:               0.0,
            burst_count:       0,
            total_samples:     0,
            rms_smooth:        0.0,
            audio_display:     AudioDisplay::Both,
            y_scale:           0.0,
            wav:               WavRecorder::new(),
            wav_folder:        String::new(),
            wav_status:        String::new(),
            show_toolbox:      false,
            toolbox_algos:     toolbox::create_algos(),
            selected_algo_idx: 0,
            toolbox_source:    ToolboxSource::Hpf,
        };

        // Auto-connect if a port was given on the CLI.
        if let Some(p) = port {
            app.do_connect_to(&p);
        }

        app
    }

    // ── Internal helpers ───────────────────────────────────────────────────

    fn do_connect_to(&mut self, port: &str) {
        self.port_connected = port.to_string();
        let _ = self.cmd_tx.send(ConnCmd::Connect(port.to_string(), self.baud));
    }

    fn do_disconnect(&mut self) {
        let _ = self.cmd_tx.send(ConnCmd::Disconnect);
    }

    fn start_port_scan(&mut self) {
        self.port_scanning = true;
        let tx = self.event_tx.clone();
        std::thread::spawn(move || {
            let raw = serialport::available_ports().unwrap_or_default();
            let ports: Vec<PortInfo> = raw.into_iter().map(port_info_from).collect();
            let _ = tx.send(AppEvent::PortsAvailable(ports));
        });
    }

    /// Drain all pending events from the background thread.
    fn process_events(&mut self) {
        while let Ok(evt) = self.rx.try_recv() {
            match evt {
                AppEvent::Samples { raw, .. } => {
                    self.burst_count   += 1;
                    self.total_samples += raw.len() as u64;

                    // Compute RMS of this burst and update smoother.
                    let sq: f64 = raw.iter().map(|&s| (s as f64) * (s as f64)).sum();
                    let rms = (sq / raw.len() as f64).sqrt() as f32;
                    self.rms_smooth = if rms > self.rms_smooth {
                        rms
                    } else {
                        self.rms_smooth * 0.95 + rms * 0.05
                    };

                    // Apply HPF into a separate buffer; raw stays untouched.
                    let mut hpf_out = vec![0i16; raw.len()];
                    self.hpf.process_into(&raw, &mut hpf_out);

                    // Optionally record raw samples to WAV.
                    if self.wav.is_recording() {
                        self.wav.write_samples(&raw);
                    }

                    // Append to both ring buffers.
                    let dt = 1.0 / self.sample_rate as f64;
                    for (&rs, &hs) in raw.iter().zip(hpf_out.iter()) {
                        self.raw_buf.push_back([self.t_s, rs as f64]);
                        self.hpf_buf.push_back([self.t_s, hs as f64]);
                        self.t_s += dt;
                    }
                    while self.raw_buf.len() > self.buf_cap { self.raw_buf.pop_front(); }
                    while self.hpf_buf.len() > self.buf_cap { self.hpf_buf.pop_front(); }
                }

                AppEvent::Connected => {
                    self.connected = true;
                    self.error_msg = None;
                    self.y_scale   = 0.0; // reset so fast-attack snaps to signal on first burst
                }

                AppEvent::Disconnected => {
                    self.connected = false;
                    if self.wav.is_recording() {
                        if let Some(name) = self.wav.stop() {
                            self.wav_status = format!("Saved: {}", name);
                        }
                    }
                }

                AppEvent::Error(msg) => {
                    self.error_msg = Some(msg);
                    self.connected = false;
                }

                AppEvent::PortsAvailable(ports) => {
                    // Try to keep the same port selected by name.
                    let prev_name = self.selected_port_idx
                        .and_then(|i| self.available_ports.get(i))
                        .map(|p| p.name.clone());
                    self.available_ports = ports;
                    self.port_scanning   = false;
                    self.selected_port_idx = prev_name
                        .as_deref()
                        .and_then(|name| {
                            self.available_ports.iter().position(|p| p.name == name)
                        })
                        .or_else(|| {
                            if self.available_ports.is_empty() { None } else { Some(0) }
                        });
                }

                AppEvent::WavSaved(name) => {
                    self.wav_status = format!("Saved: {}", name);
                }
            }
        }
    }

    /// Extract points whose time falls within the inclusive range [x_min, x_max].
    fn visible_window_range(buf: &VecDeque<[f64; 2]>, x_min: f64, x_max: f64) -> Vec<[f64; 2]> {
        buf.iter()
            .filter(|p| p[0] >= x_min && p[0] <= x_max)
            .copied()
            .collect()
    }

    /// Min/max envelope decimation to TARGET_PLOT_PTS, mean-centred.
    fn decimate_for_plot(pts: &[[f64; 2]], mean: f64) -> Vec<[f64; 2]> {
        if pts.is_empty() { return Vec::new(); }
        let stride = (pts.len() / TARGET_PLOT_PTS).max(1);
        if stride <= 1 {
            return pts.iter().map(|p| [p[0], p[1] - mean]).collect();
        }
        let mut out = Vec::with_capacity((pts.len() / stride + 1) * 2);
        for chunk in pts.chunks(stride) {
            let mut mn = f64::INFINITY;
            let mut mx = f64::NEG_INFINITY;
            let mut t_mn = chunk[0][0];
            let mut t_mx = chunk[0][0];
            for p in chunk {
                let v = p[1] - mean;
                if v < mn { mn = v; t_mn = p[0]; }
                if v > mx { mx = v; t_mx = p[0]; }
            }
            if t_mn <= t_mx {
                out.push([t_mn, mn]);
                out.push([t_mx, mx]);
            } else {
                out.push([t_mx, mx]);
                out.push([t_mn, mn]);
            }
        }
        out
    }

    /// Compute the mean of a visible-window slice.
    fn mean(vis: &[[f64; 2]]) -> f64 {
        if vis.is_empty() { return 0.0; }
        vis.iter().map(|p| p[1]).sum::<f64>() / vis.len() as f64
    }

    /// Update `y_scale` from mean-centred peaks (fast-attack / slow-decay).
    ///
    /// Must use the same mean-centred values that `decimate_for_plot` renders,
    /// otherwise a DC offset in the raw buffer inflates the scale and compresses
    /// both waveforms to thin lines.
    fn update_y_scale(&mut self, raw_vis: &[[f64; 2]], hpf_vis: &[[f64; 2]],
                      raw_mean: f64, hpf_mean: f64) {
        let peak_raw = raw_vis.iter().map(|p| (p[1] - raw_mean).abs()).fold(0.0_f64, f64::max);
        let peak_hpf = hpf_vis.iter().map(|p| (p[1] - hpf_mean).abs()).fold(0.0_f64, f64::max);
        let peak = match self.audio_display {
            AudioDisplay::Raw  => peak_raw,
            AudioDisplay::Hpf  => peak_hpf,
            AudioDisplay::Both => peak_raw.max(peak_hpf),
        };
        // Floor of 1.0 just prevents a zero/negative axis when fully silent.
        // Do NOT use a large floor like 500 — that would prevent zooming in on
        // quiet signals (e.g. a ±27 count signal would never zoom below ±500).
        let target = (peak * 1.2).max(1.0);
        self.y_scale = if target > self.y_scale {
            target
        } else {
            self.y_scale * 0.97 + target * 0.03
        };
    }

    // ── Toolbar panel ─────────────────────────────────────────────────────

    fn draw_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            // --- Port selector -------------------------------------------
            let combo_label = self.selected_port_idx
                .and_then(|i| self.available_ports.get(i))
                .map(|p| {
                    if p.description.is_empty() {
                        p.name.clone()
                    } else {
                        format!("{}  ({})", p.name, p.description)
                    }
                })
                .unwrap_or_else(|| {
                    if self.port_scanning { "(scanning…)".into() }
                    else if self.available_ports.is_empty() { "(no ports)".into() }
                    else { "(select port)".into() }
                });

            egui::ComboBox::from_id_source("port_combo")
                .width(220.0)
                .selected_text(&combo_label)
                .show_ui(ui, |ui| {
                    for (i, p) in self.available_ports.iter().enumerate() {
                        let label = if p.description.is_empty() {
                            p.name.clone()
                        } else {
                            format!("{}  ({})", p.name, p.description)
                        };
                        ui.selectable_value(&mut self.selected_port_idx, Some(i), label);
                    }
                });

            if ui.button("⟳").on_hover_text("Refresh port list").clicked() {
                self.start_port_scan();
            }

            ui.separator();

            // --- Connect / Disconnect ------------------------------------
            if self.connected {
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new("Disconnect").color(Color32::from_rgb(255, 80, 80)),
                    ))
                    .clicked()
                {
                    self.do_disconnect();
                }
            } else {
                let can_connect = self.selected_port_idx.is_some();
                if ui
                    .add_enabled(can_connect, egui::Button::new("Connect"))
                    .clicked()
                {
                    if let Some(idx) = self.selected_port_idx {
                        if let Some(port) = self.available_ports.get(idx) {
                            self.do_connect_to(&port.name.clone());
                        }
                    }
                }
            }

            ui.separator();

            // --- Display mode -------------------------------------------
            ui.label("Show:");
            ui.selectable_value(&mut self.audio_display, AudioDisplay::Raw, "Raw")
                .on_hover_text("Raw DFSDM samples only");
            ui.selectable_value(&mut self.audio_display, AudioDisplay::Hpf, "HPF")
                .on_hover_text("High-pass filtered only");
            ui.selectable_value(&mut self.audio_display, AudioDisplay::Both, "Both")
                .on_hover_text("Both waveforms overlaid");

            ui.separator();

            // --- WAV recording ------------------------------------------
            {
                let rec = self.wav.is_recording();
                let btn_text = if rec {
                    egui::RichText::new("■ Stop").color(Color32::from_rgb(255, 80, 80))
                } else {
                    egui::RichText::new("● Rec")
                };
                if ui.button(btn_text).clicked() {
                    if rec {
                        if let Some(name) = self.wav.stop() {
                            self.wav_status = format!("Saved: {}", name);
                        }
                    } else {
                        let folder = if self.wav_folder.is_empty() {
                            std::env::current_dir()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned()
                        } else {
                            self.wav_folder.clone()
                        };
                        let ts   = Local::now().format("%Y%m%d_%H%M%S");
                        let name = format!("DFSDM_{}.wav", ts);
                        let path = std::path::Path::new(&folder).join(&name);
                        match self.wav.start(&path, self.sample_rate) {
                            Ok(_) => self.wav_status = format!("Recording: {}", name),
                            Err(e) => self.wav_status = format!("WAV error: {}", e),
                        }
                    }
                }
            }

            if ui.button("Browse…").on_hover_text("Choose WAV output folder").clicked() {
                // rfd::FileDialog is synchronous on Linux/Windows.
                if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                    self.wav_folder = folder.to_string_lossy().into_owned();
                }
            }

            if !self.wav_status.is_empty() {
                ui.label(
                    egui::RichText::new(&self.wav_status)
                        .small()
                        .color(Color32::GRAY),
                );
            }

            ui.separator();

            // --- Toolbox toggle -----------------------------------------
            ui.toggle_value(&mut self.show_toolbox, "Toolbox");

            ui.separator();

            // --- Stats display ------------------------------------------
            if self.connected {
                let level = self.rms_smooth / 32_768.0;
                let rms_color = if level < 0.5 {
                    Color32::from_rgb(0, 200, 100)
                } else if level < 0.85 {
                    Color32::from_rgb(255, 180, 0)
                } else {
                    Color32::from_rgb(255, 60, 60)
                };
                ui.label(
                    egui::RichText::new(format!(
                        "{}  |  bursts: {}  samples: {}  rate: {} Hz",
                        self.port_connected,
                        self.burst_count,
                        self.total_samples,
                        self.sample_rate,
                    ))
                    .small(),
                );
                ui.separator();
                ui.label(
                    egui::RichText::new(format!("RMS {:.0}", self.rms_smooth))
                        .color(rms_color),
                );
                ui.add(egui::ProgressBar::new(level.min(1.0)).desired_width(120.0));
            } else if let Some(ref err) = self.error_msg.clone() {
                ui.colored_label(Color32::from_rgb(255, 80, 80), err);
            }
        });
    }

    // ── Toolbox side panel ────────────────────────────────────────────────

    fn draw_toolbox(
        &mut self,
        ui:      &mut egui::Ui,
        raw_vis: &[[f64; 2]],
        hpf_vis: &[[f64; 2]],
        win_start: f64,
        now_s:     f64,
    ) {
        // Source selector
        ui.horizontal(|ui| {
            ui.label("Source:");
            ui.selectable_value(&mut self.toolbox_source, ToolboxSource::Raw, "Raw");
            ui.selectable_value(&mut self.toolbox_source, ToolboxSource::Hpf, "HPF");
        });
        ui.separator();

        // Algo tab strip
        let n = self.toolbox_algos.len();
        let mut new_sel = self.selected_algo_idx;
        ui.horizontal_wrapped(|ui| {
            for i in 0..n {
                let name = self.toolbox_algos[i].name().to_string();
                if ui
                    .selectable_label(self.selected_algo_idx == i, &name)
                    .clicked()
                {
                    new_sel = i;
                }
            }
        });
        self.selected_algo_idx = new_sel;
        ui.separator();

        let signals = ToolboxSignals {
            raw:         raw_vis,
            hpf:         hpf_vis,
            sample_rate: self.sample_rate as f64,
            x_min:       win_start,
            x_max:       now_s,
            source:      self.toolbox_source,
        };

        let idx = self.selected_algo_idx.min(self.toolbox_algos.len().saturating_sub(1));
        if let Some(algo) = self.toolbox_algos.get_mut(idx) {
            algo.draw(ui, &signals);
        }
    }
}

impl eframe::App for DfsdmApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.process_events();

        // ── Compute visible window ─────────────────────────────────────────
        let now_s     = self.t_s.max(WINDOW_S);
        let win_start = now_s - WINDOW_S;

        let raw_vis = Self::visible_window_range(&self.raw_buf, win_start, now_s);
        let hpf_vis = Self::visible_window_range(&self.hpf_buf, win_start, now_s);

        // Compute means once; reused by update_y_scale AND decimate_for_plot.
        let raw_mean = Self::mean(&raw_vis);
        let hpf_mean = Self::mean(&hpf_vis);

        self.update_y_scale(&raw_vis, &hpf_vis, raw_mean, hpf_mean);

        // ── Toolbar ────────────────────────────────────────────────────────
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            self.draw_toolbar(ui);
        });

        // ── Toolbox side panel ─────────────────────────────────────────────
        if self.show_toolbox {
            egui::SidePanel::right("toolbox_panel")
                .resizable(true)
                .default_width(420.0)
                .min_width(300.0)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        self.draw_toolbox(
                            ui,
                            &raw_vis,
                            &hpf_vis,
                            win_start,
                            now_s,
                        );
                    });
                });
        }

        // ── Waveform plot ──────────────────────────────────────────────────
        egui::CentralPanel::default().show(ctx, |ui| {
            // Decimate for GPU — mean-centred (means computed above)
            let raw_pts = Self::decimate_for_plot(&raw_vis, raw_mean);
            let hpf_pts = Self::decimate_for_plot(&hpf_vis, hpf_mean);

            Plot::new("audio_wave")
                .show_axes(true)
                .show_grid(true)
                .label_formatter(|_, v| format!("t = {:.3} s\namp = {:.0}", v.x, v.y))
                .show(ui, |plot_ui| {
                    plot_ui.set_plot_bounds(PlotBounds::from_min_max(
                        [win_start, -self.y_scale],
                        [now_s,      self.y_scale],
                    ));

                    match self.audio_display {
                        AudioDisplay::Raw | AudioDisplay::Both => {
                            if !raw_pts.is_empty() {
                                plot_ui.line(
                                    Line::new(PlotPoints::new(raw_pts))
                                        .color(COL_RAW)
                                        .width(1.0)
                                        .name("Raw"),
                                );
                            }
                        }
                        _ => {}
                    }

                    match self.audio_display {
                        AudioDisplay::Hpf | AudioDisplay::Both => {
                            if !hpf_pts.is_empty() {
                                plot_ui.line(
                                    Line::new(PlotPoints::new(hpf_pts))
                                        .color(COL_HPF)
                                        .width(1.0)
                                        .name("HPF"),
                                );
                            }
                        }
                        _ => {}
                    }
                });
        });

        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_within_bounds_is_unchanged() {
        // oldest = 0, newest = 10
        assert_eq!(clamp_view_to_history(5.0, 8.0, 10.0, 30.0), (5.0, 8.0));
    }

    #[test]
    fn clamp_rejects_future_pan() {
        // width 3, newest 10 -> hi pinned to 10, width preserved
        assert_eq!(clamp_view_to_history(9.0, 12.0, 10.0, 30.0), (7.0, 10.0));
    }

    #[test]
    fn clamp_rejects_pan_before_history_floor() {
        // t_s 100, history 30 -> oldest 70; width 3 preserved
        assert_eq!(clamp_view_to_history(50.0, 53.0, 100.0, 30.0), (70.0, 73.0));
    }

    #[test]
    fn clamp_floors_at_zero_early() {
        // t_s 10 < history 30 -> oldest 0
        assert_eq!(clamp_view_to_history(-5.0, -2.0, 10.0, 30.0), (0.0, 3.0));
    }

    #[test]
    fn clamp_caps_width_to_available_span() {
        // span = 10, request width 100 -> full span [0,10]
        assert_eq!(clamp_view_to_history(0.0, 100.0, 10.0, 30.0), (0.0, 10.0));
    }

    fn buf_of(times: &[f64]) -> VecDeque<[f64; 2]> {
        times.iter().map(|&t| [t, t * 10.0]).collect()
    }

    #[test]
    fn range_window_includes_inclusive_endpoints() {
        let buf = buf_of(&[0.0, 1.0, 2.0, 3.0, 4.0]);
        let got = DfsdmApp::visible_window_range(&buf, 1.0, 3.0);
        assert_eq!(got, vec![[1.0, 10.0], [2.0, 20.0], [3.0, 30.0]]);
    }

    #[test]
    fn range_window_empty_when_outside() {
        let buf = buf_of(&[0.0, 1.0, 2.0]);
        assert!(DfsdmApp::visible_window_range(&buf, 5.0, 6.0).is_empty());
    }
}
