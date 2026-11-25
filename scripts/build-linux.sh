#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

echo "[nidong] Linux build starting in: $ROOT_DIR"

check_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "[nidong] ERROR: missing command '$1'." >&2
    return 1
  fi
}

check_cmd node
check_cmd npm
check_cmd cargo
check_cmd pkg-config
check_cmd xdg-open

MISSING_PKGS=()
check_pkg() {
  if ! pkg-config --exists "$1" 2>/dev/null; then
    MISSING_PKGS+=("$2")
  fi
}

check_pkg webkit2gtk-4.1 libwebkit2gtk-4.1-dev
check_pkg gtk+-3.0 libgtk-3-dev
check_pkg openssl libssl-dev
check_pkg dbus-1 libdbus-1-dev
check_pkg librsvg-2.0 librsvg2-dev

if ! pkg-config --exists ayatana-appindicator3-0.1 2>/dev/null && ! pkg-config --exists appindicator3-0.1 2>/dev/null; then
  MISSING_PKGS+=(libayatana-appindicator3-dev)
fi

if [ "${#MISSING_PKGS[@]}" -gt 0 ]; then
  echo "[nidong] Missing system development packages:" >&2
  echo "  sudo apt update && sudo apt install -y build-essential curl wget file patchelf xdg-utils ${MISSING_PKGS[*]} gstreamer1.0-plugins-base gstreamer1.0-plugins-good libnotify-bin" >&2
  exit 1
fi

if [ ! -d node_modules ]; then
  echo "[nidong] Installing npm dependencies..."
  npm install
fi

TARGETS="${NIDONG_LINUX_TARGETS:-deb,appimage}"
EXTRA_ARGS=(--bundles "$TARGETS")

KEY_PATH="${TAURI_SIGNING_PRIVATE_KEY:-$HOME/.tauri/nidong.key}"
if [ -f "$KEY_PATH" ]; then
  echo "[nidong] Signing key detected at $KEY_PATH; enabling updater artifacts."
  export TAURI_SIGNING_PRIVATE_KEY="$KEY_PATH"
  export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}"
  EXTRA_ARGS+=(--config '{"bundle":{"createUpdaterArtifacts":true}}')
else
  echo "[nidong] No signing key found; building unsigned local packages (deb / AppImage)."
fi

echo "[nidong] Running Rust unit tests..."
cargo test --manifest-path src-tauri/Cargo.toml --lib

echo "[nidong] Building Linux bundles ($TARGETS)..."
export APPIMAGE_EXTRACT_AND_RUN="${APPIMAGE_EXTRACT_AND_RUN:-1}"
export NO_STRIP="${NO_STRIP:-true}"
npx tauri build "${EXTRA_ARGS[@]}"

echo "[nidong] Build finished. Output artifacts:"
find src-tauri/target/release/bundle -maxdepth 3 -type f \( -name "*.deb" -o -name "*.AppImage" -o -name "*.sig" \) -print 2>/dev/null || true
