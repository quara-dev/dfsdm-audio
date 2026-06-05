#!/bin/bash
# build_cross.sh — Cross-compile dfsdm-viz for Linux (x86_64) and Windows (x86_64)
#
# Usage:
#   cd tools/dfsdm-viz
#   ./build_cross.sh
#
# Outputs:
#   target/x86_64-unknown-linux-gnu/release/dfsdm-viz
#   target/x86_64-pc-windows-gnu/release/dfsdm-viz.exe
#
# Requirements:
#   - Docker with access to the Docker daemon
#
# The Linux build uses OpenGL + X11/Wayland headers from the Dockerfile.
# The Windows build uses the same image with the mingw-w64 toolchain; the
# X11/Wayland headers are not compiled when targeting windows (Cargo gates
# those features at the crate level).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
IMAGE_NAME="dfsdm_viz_builder"

echo "==> Building Docker image: ${IMAGE_NAME}"
docker build -t "${IMAGE_NAME}" "${SCRIPT_DIR}"

run_build() {
    local TARGET="$1"
    echo "==> Building dfsdm-viz for ${TARGET}..."

    local ENV_VARS=""
    if [ "${TARGET}" = "x86_64-pc-windows-gnu" ]; then
        ENV_VARS="-e CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc"
    fi

    docker run --rm \
        -v "${SCRIPT_DIR}":/app \
        -w /app \
        ${ENV_VARS} \
        "${IMAGE_NAME}" \
        cargo build --release --target "${TARGET}"
}

run_build x86_64-unknown-linux-gnu
run_build x86_64-pc-windows-gnu

# Fix permissions — Docker runs as root; restore to calling user
echo "==> Fixing file permissions..."
docker run --rm \
    -v "${SCRIPT_DIR}":/app \
    -w /app \
    "${IMAGE_NAME}" \
    chown -R "$(id -u):$(id -g)" target/ || true

echo ""
echo "==> Build complete."
echo "    Linux  : target/x86_64-unknown-linux-gnu/release/dfsdm-viz"
echo "    Windows: target/x86_64-pc-windows-gnu/release/dfsdm-viz.exe"
