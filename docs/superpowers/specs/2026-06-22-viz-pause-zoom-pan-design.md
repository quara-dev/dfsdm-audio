# dfsdm-viz: Pause / Zoom / Pan — Design

**Date:** 2026-06-22
**Component:** `tools/dfsdm-viz` (egui/eframe + egui_plot real-time audio visualiser)
**Status:** Approved, pending implementation plan

## Problem

The visualiser draws the last `WINDOW_S` of audio and force-sets plot bounds
every frame (`app.rs` `set_plot_bounds`), which locks the view: the user cannot
pause, zoom, or pan to inspect a transient. We want oscilloscope-style
navigation while live data keeps flowing.

## Goals

- Pause the on-screen view without losing incoming data.
- Zoom and pan the time (X) axis to inspect detail anywhere in recent history.
- Keep amplitude (Y) framed automatically at all times.
- Re-engage live follow on demand.

## Non-Goals (YAGNI)

- Manual Y-axis zoom / Y lock.
- History deeper than the existing 30 s ring buffer.
- Scrollbar widget, minimap, or time-cursor measurement tools.
- Snapshotting data to a separate frozen buffer.

## Key Decisions

| Decision | Choice |
| --- | --- |
| Data while paused | Background thread keeps filling the 30 s ring buffer; only the view freezes. |
| Navigation scope | Works during live run too: any pan/scroll auto-pauses follow. |
| Axes | X free (zoom + pan); Y stays on existing fast-attack auto-scale. |
| Re-engage live | Toolbar "Go Live" button + Spacebar toggle; resets view width to default `WINDOW_S`. |

## Architecture

Decouple *buffer contents* (unchanged: bg thread fills 30 s ring buffer) from
*plot view bounds*. A `follow` flag decides who owns the X view.

### New state on `DfsdmApp`

```rust
follow:       bool,        // true = live auto-scroll (default)
view_width_s: f64,         // current time-window width, default WINDOW_S
last_x_range: (f64, f64),  // previous frame's visible X range, for buffer query
```

No new buffers. Navigation operates over the existing `raw_buf` / `hpf_buf`
(`HISTORY_S = 30 s`).

### Interaction model

- **Live (`follow = true`)** — default. X view = `[t_s - view_width_s, t_s]`,
  set every frame. Y = current auto-scale. Behaviour identical to today.
- **Auto-pause** — inside the plot closure, if `response.dragged()` or a
  scroll-zoom occurs while hovered, set `follow = false`.
- **Paused (`follow = false`)** — X bounds owned by the user; egui handles pan
  and scroll-zoom on the X axis. We never overwrite X. Pan is clamped to
  `[t_s - HISTORY_S, t_s]`.
- **Re-engage** — "⏸ Pause / ▶ Go Live" toolbar button and Spacebar toggle.
  Go Live sets `view_width_s = WINDOW_S` and `follow = true`.

### X-free / Y-auto with egui_plot 0.27

- Plot builder: `allow_drag([true, false])`, `allow_zoom([true, false])`,
  `allow_scroll(true)` — restrict user gestures to the time axis.
- Each frame, inside the closure: read `plot_ui.plot_bounds()`, keep the user's
  X range, overwrite the Y range with the auto `y_scale`, then
  `set_plot_bounds(...)`. Y always auto-frames the signal, even while paused;
  X stays where the user left it.

### Data flow change

`visible_window` currently filters points `>= now_s - WINDOW_S`. Change it to
query the **actual visible X range** (derived from plot bounds, one-frame lag via
`last_x_range`), clamped to the ring buffer. The existing min/max envelope
decimation to `TARGET_PLOT_PTS` then runs on that range — so zooming in
re-decimates and exposes real waveform detail. The toolbox consumes the same
visible slice, so analysis follows what is on screen.

## Components Touched

- `app.rs`
  - Add state fields above.
  - `update()`: branch X-bounds logic on `follow`; read/clamp visible range;
    detect interaction to auto-pause; apply Y-auto each frame.
  - `draw_toolbar()`: add Pause / Go Live button and a "PAUSED" + visible
    time-range indicator.
  - Spacebar handling via `ctx.input`.
  - Extract pure helper `clamp_view_to_history(x_min, x_max, t_s, history_s)`
    for unit testing.
- No firmware, serial, or buffer-ingestion changes.

## Error / Edge Cases

- Empty buffer (not yet connected): keep current behaviour; follow stays on.
- Pan past oldest sample: clamp to `t_s - HISTORY_S`.
- Disconnect while paused: view stays frozen on last data; Go Live re-arms for
  next connect.
- Buffer shorter than `view_width_s` early on: existing `now_s.max(WINDOW_S)`
  guard preserved.

## Testing

- **Unit:** `clamp_view_to_history` (clamping, ordering); decimation already
  unit-testable on the visible range.
- **Manual:**
  1. Connect — live scroll unchanged.
  2. Drag — view auto-pauses; data keeps accumulating underneath.
  3. Scroll-zoom in — waveform detail appears (re-decimation).
  4. Spacebar — toggles pause/live.
  5. Go Live — snaps back to live, default width.
  6. Pan past 30 s — clamps at buffer edge.
