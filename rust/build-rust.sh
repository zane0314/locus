#!/usr/bin/env bash
set -Eeuo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
CRATE_DIR="$SCRIPT_DIR/locus_core"

export RUSTUP_HOME="${RUSTUP_HOME:-/Users/zane/.rustup}"
export CARGO_HOME="${CARGO_HOME:-/Users/zane/.cargo}"
export PATH="$CARGO_HOME/bin:/opt/homebrew/bin:$PATH"

ANDROID_HOME="${ANDROID_HOME:-/opt/homebrew/share/android-commandlinetools}"
ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-$ANDROID_HOME/ndk/25.0.8775105}"
NDK_HOST="$(find "$ANDROID_NDK_HOME/toolchains/llvm/prebuilt" -mindepth 1 -maxdepth 1 -type d -name 'darwin-*' | LC_ALL=C sort | sed -n '1p')"
LINKER="$NDK_HOST/bin/aarch64-linux-android23-clang"
READELF="$NDK_HOST/bin/llvm-readelf"

MODE="${1:-all}"

build_host() {
  echo "[Locus Rust] building host library for JVM tests..."
  (cd "$CRATE_DIR" && cargo build --release)
  mkdir -p "$PROJECT_ROOT/.build/rust-host"
  cp -f "$CRATE_DIR/target/release/liblocus_core.dylib" "$PROJECT_ROOT/.build/rust-host/liblocus_core.dylib"
  echo "[Locus Rust] host library ready: $PROJECT_ROOT/.build/rust-host/liblocus_core.dylib"
}

build_android() {
  echo "[Locus Rust] building Android arm64-v8a library (16 KiB aligned)..."
  (
    cd "$CRATE_DIR"
    CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$LINKER" \
    CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS="-C link-arg=-Wl,-z,max-page-size=16384" \
      cargo build --release --target aarch64-linux-android
  )
  local so="$CRATE_DIR/target/aarch64-linux-android/release/liblocus_core.so"
  local dst="$PROJECT_ROOT/app/src/main/jniLibs/arm64-v8a/liblocus_core.so"
  mkdir -p "$(dirname "$dst")"
  cp -f "$so" "$dst"

  local hdr phdr
  hdr="$("$READELF" -h "$dst")"
  printf '%s\n' "$hdr" | grep -Eq 'Machine:[[:space:]]+AArch64' \
    || { echo "liblocus_core.so is not AArch64" >&2; exit 1; }
  phdr="$("$READELF" -l "$dst")"
  printf '%s\n' "$phdr" \
    | awk '/ LOAD / { if ($NF != "0x4000") bad=1; count++ } END { exit(count >= 1 && !bad ? 0 : 1) }' \
    || { echo "liblocus_core.so LOAD segment not 16 KiB aligned" >&2; exit 1; }
  echo "[Locus Rust] Android library verified (AArch64, 16 KiB aligned): $dst"
}

case "$MODE" in
  host) build_host ;;
  android) build_android ;;
  all) build_host; build_android ;;
  *) echo "usage: build-rust.sh [host|android|all]" >&2; exit 2 ;;
esac
