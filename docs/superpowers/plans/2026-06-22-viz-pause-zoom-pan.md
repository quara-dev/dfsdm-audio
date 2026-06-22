# dfsdm-viz Pause / Zoom / Pan Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add oscilloscope-style pause, X-axis zoom, and pan to the dfsdm-viz real-time audio plot while live data keeps flowing.

**Architecture:** Decouple the 30 s ring buffer (bg thread keeps filling) from the plot view bounds. A `follow` flag decides whether the app drives the X view (live auto-scroll) or the user does (paused pan/zoom). Y stays on the existing fast-attack auto-scale every frame. Pure logic (history clamping, range windowing) is extracted into testable functions; the egui wiring is integration-tested manually.

**Tech Stack:** Rust, eframe/egui 0.27, egui_plot 0.27.

## Global Constraints

- Target crate: `tools/dfsdm-viz`. No firmware, serial, or buffer-ingestion changes.
- egui/egui_plot version: 0.27 (do not bump).
- Constants (already defined in `src/app.rs` / `src/toolbox/mod.rs`): `WINDOW_S = 3.0` (default view width), `HISTORY_S = 30.0` (ring buffer depth), `TARGET_PLOT_PTS = 3_000`.
- egui_plot 0.27 API facts used below (verified): `Plot::allow_drag/allow_zoom/allow_scroll` accept `impl Into<egui::Vec2b>` (so `[true, false]` works); `allow_boxed_zoom(bool)`; `PlotUi::plot_bounds() -> PlotBounds`; `PlotUi::set_plot_bounds(PlotBounds)`; `PlotUi::response() -> &Response`; `PlotUi::ctx() -> &Context`; `PlotBounds::from_min_max([f64;2],[f64;2])`, `PlotBounds::min()/max() -> [f64;2]`.
- Build/lint commands run from `tools/dfsdm-viz/`.

---

### Task 1: Pure history-clamp helper

Clamp a requested `(x_min, x_max)` view range to the data actually held in the ring buffer: never pan into the future (`> t_s`), never past the oldest retained sample (`t_s - HISTORY_S`, floored at 0), and never request a window wider than the available span. Width is preserved while shifting.

**Files:**
- Modify: `tools/dfsdm-viz/src/app.rs` (add free fn near top-level helpers, after the `use` block, before `impl DfsdmApp`)
- Test: `tools/dfsdm-viz/src/app.rs` (new `#[cfg(test)] mod tests` at end of file)

**Interfaces:**
- Consumes: nothing.
- Produces: `fn clamp_view_to_history(x_min: f64, x_max: f64, t_s: f64, history_s: f64) -> (f64, f64)` — module-private free function.

- [ ] **Step 1: Write the failing test**

Add at the very end of `src/app.rs`:

```rust
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
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path tools/dfsdm-viz/Cargo.toml clamp_ 2>&1 | tail -20`
Expected: FAIL — `cannot find function 'clamp_view_to_history' in this scope`.

- [ ] **Step 3: Write minimal implementation**

Add after the `use` block (before `impl DfsdmApp`) in `src/app.rs`:

```rust
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
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --manifest-path tools/dfsdm-viz/Cargo.toml clamp_ 2>&1 | tail -20`
Expected: PASS — 5 tests pass.

- [ ] **Step 5: Commit**

```bash
git add tools/dfsdm-viz/src/app.rs
git commit -m "feat(viz): add clamp_view_to_history helper with tests"
```

---

### Task 2: Range-based visible-window query

Today `visible_window(buf, win_start)` returns every point with `time >= win_start` — implicitly "last WINDOW_S". To pan/zoom we need to fetch an explicit `[x_min, x_max]` slice. Add a range variant and route the existing call sites through it. This keeps decimation (`decimate_for_plot`) and the toolbox operating on exactly the visible range, so zooming in re-decimates and reveals detail.

**Files:**
- Modify: `tools/dfsdm-viz/src/app.rs` (add `visible_window_range`; keep or replace `visible_window`)
- Test: `tools/dfsdm-viz/src/app.rs` (`mod tests`)

**Interfaces:**
- Consumes: nothing new.
- Produces: `fn DfsdmApp::visible_window_range(buf: &VecDeque<[f64; 2]>, x_min: f64, x_max: f64) -> Vec<[f64; 2]>` — associated fn, module-private.

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `src/app.rs`:

```rust
use std::collections::VecDeque;

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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path tools/dfsdm-viz/Cargo.toml range_window 2>&1 | tail -20`
Expected: FAIL — `no function or associated item named 'visible_window_range'`.

- [ ] **Step 3: Write minimal implementation**

In `impl DfsdmApp`, replace the existing `visible_window` method with the range variant (and update its doc comment):

```rust
    /// Extract points whose time falls within the inclusive range [x_min, x_max].
    fn visible_window_range(buf: &VecDeque<[f64; 2]>, x_min: f64, x_max: f64) -> Vec<[f64; 2]> {
        buf.iter()
            .filter(|p| p[0] >= x_min && p[0] <= x_max)
            .copied()
            .collect()
    }
```

Then update the two call sites in `update()` (currently `Self::visible_window(&self.raw_buf, win_start)` and the `hpf_buf` line). For now keep behaviour identical by passing the existing live window — these lines are rewritten fully in Task 3, so the minimal change here is:

```rust
        let now_s     = self.t_s.max(WINDOW_S);
        let win_start = now_s - WINDOW_S;

        let raw_vis = Self::visible_window_range(&self.raw_buf, win_start, now_s);
        let hpf_vis = Self::visible_window_range(&self.hpf_buf, win_start, now_s);
```

- [ ] **Step 4: Run tests + build to verify pass**

Run: `cargo test --manifest-path tools/dfsdm-viz/Cargo.toml range_window 2>&1 | tail -20`
Expected: PASS — 2 new tests pass.
Run: `cargo build --manifest-path tools/dfsdm-viz/Cargo.toml 2>&1 | tail -5`
Expected: builds (no reference to the removed `visible_window` remains).

- [ ] **Step 5: Commit**

```bash
git add tools/dfsdm-viz/src/app.rs
git commit -m "feat(viz): range-based visible_window_range with tests"
```

---

### Task 3: Wire follow/pause state, X-free zoom/pan, Y-auto, controls

Add the `follow` / `view_width_s` / `last_x_range` state, the live-vs-paused X-bounds branch, per-frame Y-auto via `set_plot_bounds`, interaction-driven auto-pause, the Pause/Go-Live toolbar button, and the Spacebar toggle. This is integration wiring over egui; verified by build, clippy, and a manual checklist (no unit test — it renders).

**Files:**
- Modify: `tools/dfsdm-viz/src/app.rs` — struct fields, `new()` initialisers, a `toggle_follow` helper, `draw_toolbar()`, `draw_toolbox()` call site, and `update()`.

**Interfaces:**
- Consumes: `clamp_view_to_history` (Task 1), `DfsdmApp::visible_window_range` (Task 2).
- Produces: no new public interface; internal `fn DfsdmApp::toggle_follow(&mut self)`.

- [ ] **Step 1: Add state fields**

In `struct DfsdmApp`, under the `// ── Display ──` group (next to `y_scale`):

```rust
    // ── View navigation ─────────────────────────────────────────────────────
    follow:       bool,        // true = live auto-scroll; false = paused/navigating
    view_width_s: f64,         // current time-window width
    last_x_range: (f64, f64),  // previous frame's visible X, drives this frame's query
```

In `DfsdmApp::new(...)`, in the struct literal (next to `y_scale: 0.0,`):

```rust
            follow:        true,
            view_width_s:  WINDOW_S,
            last_x_range:  (0.0, WINDOW_S),
```

- [ ] **Step 2: Add the toggle helper**

In `impl DfsdmApp`, near the other `// ── Internal helpers ──` fns:

```rust
    /// Toggle between live-follow and paused navigation.
    /// Pausing freezes at the current view (held in `last_x_range`).
    /// Going live resets the window width to the default and re-arms auto-scroll.
    fn toggle_follow(&mut self) {
        if self.follow {
            self.follow = false;
        } else {
            self.follow = true;
            self.view_width_s = WINDOW_S;
        }
    }
```

- [ ] **Step 3: Add the toolbar Pause / Go-Live control**

In `draw_toolbar()`, immediately after the Display-mode block (after the `Both` selectable_value and its following `ui.separator();`), insert:

```rust
            // --- Pause / Go Live ----------------------------------------
            if self.follow {
                if ui.button("⏸ Pause").on_hover_text("Freeze view (Space)").clicked() {
                    self.toggle_follow();
                }
            } else {
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new("▶ Go Live").color(Color32::from_rgb(0, 200, 100)),
                    ))
                    .on_hover_text("Resume live scroll (Space)")
                    .clicked()
                {
                    self.toggle_follow();
                }
                ui.label(
                    egui::RichText::new("PAUSED").color(Color32::from_rgb(255, 180, 0)),
                );
            }

            ui.separator();
```

- [ ] **Step 4: Rewrite the view/bounds logic in `update()`**

Replace the block from `// ── Compute visible window ──` down to (but not including) the `// ── Toolbar ──` panel call with:

```rust
        // ── Spacebar toggles pause/live ────────────────────────────────────
        if ctx.input(|i| i.key_pressed(egui::Key::Space)) {
            self.toggle_follow();
        }

        // ── Resolve the visible X range ────────────────────────────────────
        let now_s = self.t_s.max(WINDOW_S);
        let (x_min, x_max) = if self.follow {
            (now_s - self.view_width_s, now_s)
        } else {
            clamp_view_to_history(self.last_x_range.0, self.last_x_range.1, self.t_s, HISTORY_S)
        };

        let raw_vis = Self::visible_window_range(&self.raw_buf, x_min, x_max);
        let hpf_vis = Self::visible_window_range(&self.hpf_buf, x_min, x_max);

        // Compute means once; reused by update_y_scale AND decimate_for_plot.
        let raw_mean = Self::mean(&raw_vis);
        let hpf_mean = Self::mean(&hpf_vis);

        self.update_y_scale(&raw_vis, &hpf_vis, raw_mean, hpf_mean);
```

Note: `win_start` no longer exists — `x_min`/`x_max` replace it everywhere below.

- [ ] **Step 5: Update the toolbox call site**

In the toolbox `SidePanel` block, change the `draw_toolbox` call's last two args from `win_start, now_s` to `x_min, x_max`:

```rust
                        self.draw_toolbox(
                            ui,
                            &raw_vis,
                            &hpf_vis,
                            x_min,
                            x_max,
                        );
```

- [ ] **Step 6: Rewrite the plot block for X-free / Y-auto / auto-pause**

Replace the `Plot::new("audio_wave") ...` builder-and-closure in the `CentralPanel` with:

```rust
            Plot::new("audio_wave")
                .show_axes(true)
                .show_grid(true)
                .allow_drag([true, false])
                .allow_zoom([true, false])
                .allow_scroll(true)
                .allow_boxed_zoom(false)
                .label_formatter(|_, v| format!("t = {:.3} s\namp = {:.0}", v.x, v.y))
                .show(ui, |plot_ui| {
                    // Any user interaction drops out of live-follow.
                    let resp = plot_ui.response();
                    let interacted = resp.dragged()
                        || (resp.hovered()
                            && plot_ui.ctx().input(|i| {
                                i.smooth_scroll_delta.y.abs() > 0.0
                                    || (i.zoom_delta() - 1.0).abs() > f32::EPSILON
                            }));
                    if interacted {
                        self.follow = false;
                    }

                    // X: live window when following, else the user's current X
                    // (egui already applied this frame's drag/zoom). Y: always auto.
                    let cur = plot_ui.plot_bounds();
                    let (bx_min, bx_max) = if self.follow {
                        (x_min, x_max)
                    } else {
                        clamp_view_to_history(cur.min()[0], cur.max()[0], self.t_s, HISTORY_S)
                    };
                    plot_ui.set_plot_bounds(PlotBounds::from_min_max(
                        [bx_min, -self.y_scale],
                        [bx_max,  self.y_scale],
                    ));
                    // Drives next frame's data query.
                    self.last_x_range = (bx_min, bx_max);

                    let raw_pts = Self::decimate_for_plot(&raw_vis, raw_mean);
                    let hpf_pts = Self::decimate_for_plot(&hpf_vis, hpf_mean);

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
```

Note: the `decimate_for_plot` calls move *inside* the closure (they previously sat just above `Plot::new`). Remove the now-duplicated outer `let raw_pts = ...; let hpf_pts = ...;` lines above `Plot::new`.

- [ ] **Step 7: Build and lint**

Run: `cargo build --manifest-path tools/dfsdm-viz/Cargo.toml 2>&1 | tail -15`
Expected: builds clean (no `win_start` references, no unused-var errors).
Run: `cargo clippy --manifest-path tools/dfsdm-viz/Cargo.toml 2>&1 | tail -15`
Expected: no errors (warnings acceptable).

- [ ] **Step 8: Manual verification**

Connect to the device (or a serial source) and confirm:
1. Live scroll behaves as before (auto-follow, default 3 s window).
2. Drag the plot left/right → top bar shows **PAUSED**, view freezes, data still accumulates underneath.
3. Scroll-wheel zoom in on a quiet region → waveform detail emerges (re-decimation), Y stays framed.
4. Press **Space** → toggles PAUSED ⇄ live.
5. Click **▶ Go Live** → snaps to live, window back to 3 s.
6. While paused, pan past the 30 s buffer edge → view clamps, does not scroll into emptiness or the future.

Record the result (pass/fail per item) in the commit body or PR description.

- [ ] **Step 9: Commit**

```bash
git add tools/dfsdm-viz/src/app.rs
git commit -m "feat(viz): pause, X-axis zoom/pan with live auto-pause and Y-auto"
```

---

## Self-Review

**Spec coverage:**
- Pause keeps ingesting, view freezes → Task 3 (follow flag; bg thread untouched, no ingestion change). ✓
- Zoom/pan during live, interaction auto-pauses → Task 3 Step 6 (`interacted` → `follow = false`). ✓
- "Go Live" re-engages, resets width → Task 3 Steps 2–3 (`toggle_follow`). ✓
- X free, Y auto → Task 3 Step 6 (`allow_*([true,false])`, Y from `y_scale`). ✓
- Spacebar toggle → Task 3 Step 4. ✓
- Pan clamped to 30 s buffer → Task 1 `clamp_view_to_history`, applied in Task 3 Steps 4 & 6. ✓
- Zoom re-decimates / toolbox follows visible range → Task 2 `visible_window_range` + Task 3 Step 5. ✓
- Edge cases (empty buffer, early `< WINDOW_S`, disconnect-while-paused) → preserved by `now_s = t_s.max(WINDOW_S)` and unchanged ingestion. ✓

**Placeholder scan:** No TBD/TODO; every code step shows full code. ✓

**Type consistency:** `clamp_view_to_history(f64,f64,f64,f64) -> (f64,f64)` and `visible_window_range(&VecDeque<[f64;2]>, f64, f64) -> Vec<[f64;2]>` used identically across Tasks 1–3. `toggle_follow(&mut self)` consistent. egui_plot calls match verified 0.27 signatures. ✓
