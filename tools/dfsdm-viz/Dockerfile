FROM rust:1.88-slim-bookworm

# System libraries required by dfsdm-viz dependencies:
#
#   eframe/egui (glow backend, x11 + wayland features):
#     libGL          — OpenGL (glow renderer)
#     libx11-dev     — X11 window system (x11 feature)
#     libxcb-*       — XCB (used by winit under x11)
#     libwayland-dev — Wayland (wayland feature)
#     libxkbcommon   — keyboard handling for both x11 and wayland
#
#   mingw-w64 — cross-linker for x86_64-pc-windows-gnu target
#
# Note: serialport is built with default-features = false so libudev-dev
# is NOT required.  The Windows build does not need X11/Wayland/GL headers
# — Cargo gates those features at the crate level when the target is windows.

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    # serialport – udev-based port enumeration on Linux
    libudev-dev \
    # rfd (native file dialog) – xdg-portal backend uses D-Bus
    libdbus-1-dev \
    # OpenGL (glow backend for egui/eframe)
    libgl1-mesa-dev \
    # X11 / XCB (winit x11 backend)
    libx11-dev \
    libxcb1-dev \
    libxcb-render0-dev \
    libxcb-shape0-dev \
    libxcb-xfixes0-dev \
    # Wayland
    libwayland-dev \
    libxkbcommon-dev \
    # Windows cross-compilation toolchain
    mingw-w64 \
    # Static libwinpthread.a (needed for -Bstatic,-lwinpthread on gnu target)
    mingw-w64-x86-64-dev \
    && rm -rf /var/lib/apt/lists/*

# Register both cross-compilation targets
RUN rustup target add x86_64-unknown-linux-gnu x86_64-pc-windows-gnu

WORKDIR /app

CMD ["cargo", "build", "--release"]
