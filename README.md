# dfsdm-audio

Real-time MEMS microphone audio capture on the STM32H725, streamed over UART
and visualised on a host PC.

## Hardware

| Item | Value |
|------|-------|
| MCU | STM32H725 (Cortex-M7, 550 MHz) |
| Microphone | IM69D120 (PDM, 1.8 V I/O) |
| Interface | DFSDM (digital filter), DMA → UART4 |
| Baud rate | 921 600 |
| Sample rate | 32 kHz (DFSDM clock 3.2 MHz / OSR 50 / IntOSR 2) |
| Sample format | int16 little-endian, mono |

Wire format per DMA half/full callback:

```
[0xAA 0x55 0xAA 0x55]  4-byte sync word
[1024 × int16 LE]      2048 bytes of audio samples
```

---

## 1 — Firmware (STM32H725)

### Prerequisites

| Tool | Version | Notes |
|------|---------|-------|
| ARM GCC | 10.3-2021.10 | bundled in `toolchain/` (Linux) and `toolchain_win/` (Windows) |
| CMake | ≥ 3.22 | |
| Ninja | any | |

The bundled toolchains are excluded from git (`.gitignore`). Download from
[developer.arm.com](https://developer.arm.com/downloads/-/gnu-rm) and unpack
into `toolchain/` (Linux) or `toolchain_win/` (Windows) at the repo root.

Expected layout:

```
toolchain/
  bin/
    arm-none-eabi-gcc
    arm-none-eabi-objcopy
    ...
```

### Build

```bash
# Add the toolchain to PATH for this shell session
export PATH="$(pwd)/toolchain/bin:$PATH"

# Configure (generates build/Debug/)
cmake --preset Debug

# Compile (36 translation units, ~4 seconds)
cmake --build --preset Debug
```

Outputs in `build/Debug/`:

| File | Description |
|------|-------------|
| `dfsdm-audio.elf` | ELF with full debug info (for GDB / STM32CubeIDE) |
| `dfsdm-audio.hex` | Intel HEX — flash this to the board |

Release build (size-optimised, `-Os`):

```bash
cmake --preset Release && cmake --build --preset Release
```

### Flash

Use **STM32CubeProgrammer** (GUI or CLI):

```bash
STM32_Programmer_CLI -c port=SWD -w build/Debug/dfsdm-audio.hex -v -rst
```

Or drag-and-drop the `.hex` onto the ST-LINK virtual mass-storage drive if
your board supports it.

---

## 2 — Host visualiser (`tools/dfsdm-viz`)

A Rust/egui desktop app that connects to the board over UART and shows:

- Live scrolling waveform — Raw (cyan) and/or HPF-filtered (amber)
- WAV recording with timestamped filenames
- Signal toolbox: Welch PSD, STFT spectrogram, statistics, autocorrelation,
  RMS envelope + ZCR

### Prerequisites

| Tool | Notes |
|------|-------|
| Docker | used for cross-compilation; no local Rust install required |

### Build (cross-compile for Linux + Windows)

```bash
cd tools/dfsdm-viz
./build_cross.sh
```

The script builds a Docker image on first run (~30 s), then compiles both
targets in release mode.

Outputs:

| File | Platform |
|------|----------|
| `target/x86_64-unknown-linux-gnu/release/dfsdm-viz` | Linux x86-64 |
| `target/x86_64-pc-windows-gnu/release/dfsdm-viz.exe` | Windows x86-64 |

### Run

**Linux:**

```bash
./target/x86_64-unknown-linux-gnu/release/dfsdm-viz
```

**Windows** — double-click `dfsdm-viz.exe` or run from a terminal:

```cmd
dfsdm-viz.exe
```

The port can be selected from the toolbar ComboBox after launch, or passed on
the command line:

```bash
dfsdm-viz --port /dev/ttyUSB0          # Linux
dfsdm-viz.exe --port COM3              # Windows
```

All CLI options:

```
Options:
  -p, --port <PORT>              Serial port (optional; pick in GUI if omitted)
  -b, --baud <BAUD>              Baud rate [default: 921600]
  -s, --sample-rate <HZ>         Audio sample rate [default: 32000]
      --width <PX>               Window width  [default: 1400]
      --height <PX>              Window height [default: 650]
```

### Local dev build (requires `libudev-dev` on Linux)

```bash
cd tools/dfsdm-viz

# Ubuntu / Debian
sudo apt install libudev-dev libdbus-1-dev

cargo build          # debug
cargo build --release
```
