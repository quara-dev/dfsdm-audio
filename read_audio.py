#!/usr/bin/env python3
"""
read_audio.py  –  Real-time audio monitor for dfsdm-audio firmware.

Wire format (sent by firmware per DMA half/full callback):
    [0xAA 0x55 0xAA 0x55]  – 4-byte sync word
    [int16_LE × CHUNK]     – 1024 little-endian signed-16 samples  (2048 bytes)

Usage:
    python read_audio.py <port>
    python read_audio.py /dev/ttyUSB0 --window 2.0
    python read_audio.py COM3 --rate 32000

Dependencies:
    pip install pyserial numpy matplotlib
"""

import argparse
import queue
import sys
import threading

import numpy as np
import matplotlib.pyplot as plt
import matplotlib.animation as animation
import serial

# ── protocol constants (must match firmware) ───────────────────────────────────
SYNC  = b'\xAA\x55\xAA\x55'
CHUNK = 1024                    # samples per burst  (AUDIO_BUFFER_SIZE / 2)

# ── defaults ───────────────────────────────────────────────────────────────────
BAUD         = 921600
SAMPLE_RATE  = 32_000
WINDOW_SEC   = 2.0      # 2 s history — good balance of context vs. detail
DISPLAY_PTS  = 2000     # points actually rendered; keeps matplotlib fast at any window size
UPDATE_MS    = 40       # 25 fps — adequate for a 2 s overview
# ───────────────────────────────────────────────────────────────────────────────


def parse_args():
    p = argparse.ArgumentParser(
        description="Real-time DFSDM audio monitor — framed int16 PCM over UART"
    )
    p.add_argument("port",
                   help="Serial port, e.g. /dev/ttyUSB0 or COM3")
    p.add_argument("--baud",   type=int,   default=BAUD,
                   help=f"Baud rate (default {BAUD})")
    p.add_argument("--rate",   type=int,   default=SAMPLE_RATE,
                   help=f"Sample rate Hz (default {SAMPLE_RATE})")
    p.add_argument("--window", type=float, default=WINDOW_SEC,
                   help=f"History window in seconds (default {WINDOW_SEC})")
    return p.parse_args()


class SerialReader(threading.Thread):
    """
    Background daemon — frame-aware UART reader.
    Puts float32 numpy arrays (one burst = CHUNK samples) into a Queue.
    No terminal output; drops bursts silently when the queue is full.
    """

    def __init__(self, port: str, baud: int, chunk: int, q: queue.Queue):
        super().__init__(daemon=True)
        self.port  = port
        self.baud  = baud
        self.chunk = chunk
        self.q     = q
        self._stop = threading.Event()

    def _read_exactly(self, ser: serial.Serial, n: int):
        data = b""
        while len(data) < n and not self._stop.is_set():
            part = ser.read(n - len(data))
            if part:
                data += part
        return data if len(data) == n else None

    def _find_sync(self, ser: serial.Serial) -> bool:
        window = bytearray(len(SYNC))
        while not self._stop.is_set():
            b = ser.read(1)
            if not b:
                continue
            window = window[1:] + bytearray(b)
            if bytes(window) == SYNC:
                return True
        return False

    def run(self):
        try:
            ser = serial.Serial(self.port, self.baud, timeout=0.5)
        except serial.SerialException as exc:
            sys.exit(f"Cannot open {self.port}: {exc}")

        bpb = self.chunk * 2   # bytes per burst

        while not self._stop.is_set():
            if not self._find_sync(ser):
                break
            raw = self._read_exactly(ser, bpb)
            if raw is None:
                break
            samples = np.frombuffer(raw, dtype="<i2").astype(np.float32)
            try:
                self.q.put_nowait(samples)
            except queue.Full:
                pass   # drop — never stall the reader

        ser.close()

    def stop(self):
        self._stop.set()


def main():
    args = parse_args()

    # Ring buffer holds the full history; DISPLAY_PTS controls rendered resolution
    buf_n   = int(args.rate * args.window)        # e.g. 64 000 for 2 s @ 32 kHz
    stride  = max(1, buf_n // DISPLAY_PTS)        # decimation factor
    plot_n  = buf_n // stride                     # actual number of plotted points

    ring = np.zeros(buf_n, dtype=np.float32)

    # Y-axis state — tracked to apply hysteresis so the scale doesn't jump
    # on every tiny transient
    ylim = [1000.0]   # mutable cell shared with update()

    q = queue.Queue(maxsize=8)
    reader = SerialReader(args.port, args.baud, CHUNK, q)
    reader.start()

    # ── figure ────────────────────────────────────────────────────────────────
    fig, (ax_wave, ax_rms) = plt.subplots(
        2, 1, figsize=(12, 5),
        gridspec_kw={"height_ratios": [4, 1]},
    )
    fig.patch.set_facecolor("#1a1a2e")
    for ax in (ax_wave, ax_rms):
        ax.set_facecolor("#16213e")
        ax.tick_params(colors="#aaaaaa")
        for spine in ax.spines.values():
            spine.set_edgecolor("#444466")

    t_axis = np.linspace(-args.window, 0, plot_n)   # x in seconds

    (line,) = ax_wave.plot(t_axis, np.zeros(plot_n),
                           color="#00d4ff", lw=0.8, antialiased=True)
    ax_wave.set_xlim(t_axis[0], t_axis[-1])
    ax_wave.set_ylim(-ylim[0], ylim[0])
    ax_wave.set_ylabel("amplitude  (int16, DC removed)", color="#cccccc", fontsize=9)
    ax_wave.set_xlabel("time  (s)", color="#aaaaaa", fontsize=8)
    ax_wave.set_title(
        f"{args.port}   {args.baud} baud   {args.rate} Hz   "
        f"window {args.window:.1f} s   ({plot_n} display pts, stride {stride}×)",
        color="#e0e0ff", fontsize=9,
    )
    ax_wave.axhline(0, color="#334455", lw=0.5)

    info_text = ax_wave.text(
        0.01, 0.95, "", transform=ax_wave.transAxes,
        color="#ffcc44", fontsize=9, va="top", family="monospace",
    )

    rms_bar  = ax_rms.barh([0], [0], height=0.6, color="#00aa55", edgecolor="none")[0]
    ax_rms.set_xlim(0, 32768)
    ax_rms.set_ylim(-0.5, 0.5)
    ax_rms.set_xlabel("RMS amplitude", color="#cccccc", fontsize=9)
    ax_rms.set_yticks([])
    ax_rms.axvline(32768 * 0.9, color="#ff4444", lw=0.8, ls="--")
    rms_label = ax_rms.text(
        200, 0, "0", color="#aaffaa", fontsize=8, va="center", family="monospace"
    )

    plt.tight_layout(pad=1.2)

    # ── animation ─────────────────────────────────────────────────────────────
    def update(_frame):
        nonlocal ring

        # Drain all queued bursts into the ring buffer
        while True:
            try:
                burst = q.get_nowait()
            except queue.Empty:
                break
            n = len(burst)
            if n >= buf_n:
                ring[:] = burst[-buf_n:]
            else:
                ring[:-n] = ring[n:]    # shift old data left
                ring[-n:]  = burst      # append new data at the right

        # DC removal: subtract window mean so the wave rides the zero line
        data = ring - ring.mean()

        # Downsample: take every stride-th sample for rendering
        display = data[::stride][:plot_n]
        line.set_ydata(display)

        # Dynamic Y axis with hysteresis:
        #   - Expand immediately when signal exceeds current scale
        #   - Contract slowly (only when peak drops below 60 % of current scale)
        peak = float(np.max(np.abs(data)))
        new_ylim = max(1000.0, peak * 1.15)
        if new_ylim > ylim[0] or new_ylim < ylim[0] * 0.6:
            ylim[0] = new_ylim
            ax_wave.set_ylim(-ylim[0], ylim[0])

        # RMS level meter
        rms   = float(np.sqrt(np.mean(data ** 2)))
        level = rms / 32768
        rms_bar.set_width(rms)
        rms_label.set_x(rms + 200)
        rms_label.set_text(f"{rms:.0f}")
        rms_bar.set_color(
            "#00aa55" if level < 0.5 else
            "#ffaa00" if level < 0.85 else
            "#ff3333"
        )

        info_text.set_text(
            f"RMS {rms:6.0f}    peak {peak:5.0f}    "
            f"{'▮' * min(40, int(level * 40))}"
        )

    # blit=False required for dynamic ylim — with 2 000 display points at
    # 25 fps this is fast enough that blit offers no meaningful gain
    ani = animation.FuncAnimation(   # noqa: F841
        fig, update,
        interval=UPDATE_MS,
        blit=False,
        cache_frame_data=False,
    )

    plt.show()
    reader.stop()


if __name__ == "__main__":
    main()
