#!/usr/bin/env bash
set -Eeuo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
NATIVE_ROOT="$SCRIPT_DIR"
LIBCORE_ROOT="$NATIVE_ROOT/OwnBoxForAndroid/libcore"
EXTRA_CORES_ROOT="$NATIVE_ROOT/extra-cores"
BUILD_DIR="$LIBCORE_ROOT/.build"
AAR="$BUILD_DIR/libcore.aar"
INPUT_HASH_FILE="$BUILD_DIR/native-input.sha256"
EXTRA_CORES_BUILD_DIR="$EXTRA_CORES_ROOT/.build"
EXTRA_CORES_SO="$PROJECT_ROOT/app/src/main/jniLibs/arm64-v8a/libextra_cores.so"
EXTRA_CORES_HASH_FILE="$EXTRA_CORES_BUILD_DIR/extra-cores-input.sha256"
VERIFY_DIR="$PROJECT_ROOT/.build/native-verify"
ABI_CHECK="$PROJECT_ROOT/tests/check_native_abi.py"

ANDROID_HOME="${ANDROID_HOME:-/opt/homebrew/share/android-commandlinetools}"
ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-$ANDROID_HOME/ndk/25.0.8775105}"
JAVA_HOME="${JAVA_HOME:-/opt/homebrew/opt/openjdk@17/libexec/openjdk.jdk/Contents/Home}"
GO_VERSION="1.26.8"
GO_BIN="${GO_BIN:-$PROJECT_ROOT/.toolchains/go-$GO_VERSION/go/bin/go}"
EXTRA_GO_VERSION="1.27.3"
EXTRA_GO_BIN="${EXTRA_GO_BIN:-}"
if [ -z "$EXTRA_GO_BIN" ]; then
  if [ -x "$PROJECT_ROOT/.toolchains/go-$EXTRA_GO_VERSION/go/bin/go" ]; then
    EXTRA_GO_BIN="$PROJECT_ROOT/.toolchains/go-$EXTRA_GO_VERSION/go/bin/go"
  elif [ -x "$HOME/go/pkg/mod/golang.org/toolchain@v0.0.1-go$EXTRA_GO_VERSION.darwin-arm64/bin/go" ]; then
    EXTRA_GO_BIN="$HOME/go/pkg/mod/golang.org/toolchain@v0.0.1-go$EXTRA_GO_VERSION.darwin-arm64/bin/go"
  elif [ -x "$HOME/go/pkg/mod/golang.org/toolchain@v0.0.1-go$EXTRA_GO_VERSION.darwin-amd64/bin/go" ]; then
    EXTRA_GO_BIN="$HOME/go/pkg/mod/golang.org/toolchain@v0.0.1-go$EXTRA_GO_VERSION.darwin-amd64/bin/go"
  fi
fi
GOMOBILE_BIN="${GOMOBILE_BIN:-/Users/zane/go/bin/gomobile-matsuri}"
GOBIND_BIN="${GOBIND_BIN:-/Users/zane/go/bin/gobind-matsuri}"
PYTHON_BIN="${PYTHON_BIN:-$(command -v python3 || true)}"

export ANDROID_HOME ANDROID_NDK_HOME JAVA_HOME
export GOTOOLCHAIN=local
export GOFLAGS=-mod=readonly
export GOBIND=gobind-matsuri
export PATH="$(dirname "$GO_BIN"):$JAVA_HOME/bin:$(dirname "$GOMOBILE_BIN"):/opt/homebrew/bin:$PATH"

GOMOBILE_VERSION="v0.0.0-20250830135019-17d6af34f6bd"
NDK_VERSION="25.0.8775105"
ANDROID_API=23
NATIVE_ABI="arm64-v8a"
NATIVE_ENTRY="jni/$NATIVE_ABI/libgojni.so"
NATIVE_TAGS='with_conntrack,with_gvisor,with_quic,with_wireguard,with_utls,with_clash_api'
NATIVE_LDFLAGS='-s -w -X github.com/sagernet/sing-box/constant.Version=1.14.4 -extldflags=-Wl,-z,max-page-size=16384'
EXTRA_CORES_MIHOMO_VERSION='v1.19.34'
EXTRA_CORES_XRAY_REV='676eaf66f8f0'
EXTRA_CORES_LIBXRAY_VERSION='v1.261109.0'
EXTRA_CORES_LDFLAGS="-s -w -checklinkname=0 -X github.com/metacubex/mihomo/constant.Version=$EXTRA_CORES_MIHOMO_VERSION -extldflags=-Wl,-z,max-page-size=16384"

die() { printf 'native build failed: %s\n' "$*" >&2; exit 1; }
log() { printf '[AnyBox native] %s\n' "$*"; }
need_exec() { [ -x "$1" ] || die "missing executable: $1"; }
need_file() { [ -f "$1" ] || die "missing file: $1"; }

find_ndk_host() {
  local host
  host="$(find "$ANDROID_NDK_HOME/toolchains/llvm/prebuilt" -mindepth 1 -maxdepth 1 \
    -type d -name 'darwin-*' -print | LC_ALL=C sort | sed -n '1p')"
  [ -n "$host" ] || die "NDK LLVM host directory not found under $ANDROID_NDK_HOME"
  printf '%s\n' "$host"
}

check_toolchain() {
  local revision gomobile_info gobind_info ndk_host
  [ -n "$GO_BIN" ] || die "go is not on PATH"
  need_exec "$GO_BIN"
  need_exec "$GOMOBILE_BIN"
  need_exec "$GOBIND_BIN"
  need_file "$ANDROID_NDK_HOME/source.properties"
  ndk_host="$(find_ndk_host)"
  need_exec "$ndk_host/bin/aarch64-linux-android${ANDROID_API}-clang"
  need_exec "$ndk_host/bin/llvm-readelf"

  [ "$("$GO_BIN" env GOVERSION)" = "go$GO_VERSION" ] \
    || die "Go $GO_VERSION required; got $("$GO_BIN" env GOVERSION)"
  revision="$(sed -n 's/^Pkg.Revision = //p' "$ANDROID_NDK_HOME/source.properties")"
  [ "$revision" = "$NDK_VERSION" ] \
    || die "Android NDK $NDK_VERSION required; got ${revision:-unknown}"
  gomobile_info="$("$GO_BIN" version -m "$GOMOBILE_BIN")"
  case "$gomobile_info" in
    *"golang.org/x/mobile"*"$GOMOBILE_VERSION"*) ;;
    *) die "gomobile-matsuri is not fixed x/mobile $GOMOBILE_VERSION" ;;
  esac
  gobind_info="$("$GO_BIN" version -m "$GOBIND_BIN")"
  case "$gobind_info" in
    *"golang.org/x/mobile"*"$GOMOBILE_VERSION"*) ;;
    *) die "gobind-matsuri is not fixed x/mobile $GOMOBILE_VERSION" ;;
  esac
  [ -d "$LIBCORE_ROOT" ] || die "missing libcore source tree: $LIBCORE_ROOT"
  [ -f "$LIBCORE_ROOT/go.mod" ] || die "missing libcore go.mod"
  if [ -d "$EXTRA_CORES_ROOT" ]; then
    [ -n "$EXTRA_GO_BIN" ] || die "Go $EXTRA_GO_VERSION required for extra-cores (set EXTRA_GO_BIN)"
    need_exec "$EXTRA_GO_BIN"
    [ "$("$EXTRA_GO_BIN" env GOVERSION)" = "go$EXTRA_GO_VERSION" ] \
      || die "Go $EXTRA_GO_VERSION required for extra-cores; got $("$EXTRA_GO_BIN" env GOVERSION)"
    [ -f "$EXTRA_CORES_ROOT/go.mod" ] || die "missing extra-cores go.mod"
  fi
  log "fixed Go $GO_VERSION (+ extra Go $EXTRA_GO_VERSION), NDK $NDK_VERSION, gomobile/gobind $GOMOBILE_VERSION checked"
}

native_input_hash() {
  {
    printf '%s\n' \
      'native-input-schema=2-portable' \
      "android-api=$ANDROID_API" \
      "abi=$NATIVE_ABI" \
      "ndk=$NDK_VERSION" \
      "gomobile=$GOMOBILE_VERSION" \
      "tags=$NATIVE_TAGS" \
      "ldflags=$NATIVE_LDFLAGS"
    find "$NATIVE_ROOT" -type f \
      ! -path '*/.build/*' ! -path '*/.git/*' ! -path '*/extra-cores/*' ! -name 'BASELINE.json' ! -name 'build-native.sh' ! -name '._*' ! -name '.DS_Store' -print \
      | LC_ALL=C sort \
      | while IFS= read -r file; do
          printf 'path=%s\n' "${file#"$PROJECT_ROOT/"}"
          shasum -a 256 "$file" | awk '{print $1}'
        done
  } | shasum -a 256 | awk '{print $1}'
}

extra_cores_input_hash() {
  {
    printf '%s\n' \
      'extra-cores-input-schema=1' \
      "android-api=$ANDROID_API" \
      "abi=$NATIVE_ABI" \
      "ndk=$NDK_VERSION" \
      "go=$EXTRA_GO_VERSION" \
      "ldflags=$EXTRA_CORES_LDFLAGS"
    find "$EXTRA_CORES_ROOT" -type f \
      ! -path '*/.build/*' ! -path '*/.git/*' ! -name '._*' ! -name '.DS_Store' -print \
      | LC_ALL=C sort \
      | while IFS= read -r file; do
          printf 'path=%s\n' "${file#"$PROJECT_ROOT/"}"
          shasum -a 256 "$file" | awk '{print $1}'
        done
  } | shasum -a 256 | awk '{print $1}'
}

verify_elf_arm64_16k() {
  local lib="$1" ndk_host elf_header
  ndk_host="$(find_ndk_host)"
  elf_header="$("$ndk_host/bin/llvm-readelf" -h "$lib")"
  printf '%s\n' "$elf_header" | grep -Eq 'Machine:[[:space:]]+AArch64' \
    || die "native library is not AArch64: $lib"
  "$ndk_host/bin/llvm-readelf" -l "$lib" \
    | awk '/ LOAD / { if ($NF != "0x4000") bad=1; count++ } END { exit(count >= 1 && !bad ? 0 : 1) }' \
    || die "native library has a LOAD segment not aligned to 16 KiB: $lib"
}

extract_artifact() {
  mkdir -p "$VERIFY_DIR"
  [ -s "$AAR" ] || die "native AAR is empty: $AAR"
  unzip -t "$AAR" >/dev/null || die "native AAR is not a valid zip: $AAR"
  unzip -Z1 "$AAR" | grep -Fx "$NATIVE_ENTRY" >/dev/null \
    || die "native AAR has no $NATIVE_ENTRY"
  unzip -p "$AAR" classes.jar > "$VERIFY_DIR/classes.jar"
  unzip -p "$AAR" "$NATIVE_ENTRY" > "$VERIFY_DIR/libgojni.so"
  [ -s "$VERIFY_DIR/classes.jar" ] || die "native AAR classes.jar is empty"
  [ -s "$VERIFY_DIR/libgojni.so" ] || die "native AAR libgojni.so is empty"
  verify_elf_arm64_16k "$VERIFY_DIR/libgojni.so"
}

check_legacy_abi() {
  [ -n "$PYTHON_BIN" ] || die "python3 is required for native ABI verification"
  need_file "$ABI_CHECK"
  "$PYTHON_BIN" "$ABI_CHECK"
}

verify_extra_cores() {
  local ndk_host dyn_syms mod_info sym
  [ -s "$EXTRA_CORES_SO" ] || die "extra-cores library is empty: $EXTRA_CORES_SO"
  verify_elf_arm64_16k "$EXTRA_CORES_SO"
  ndk_host="$(find_ndk_host)"
  dyn_syms="$("$ndk_host/bin/llvm-readelf" --dyn-syms "$EXTRA_CORES_SO")"
  for sym in \
    JNI_OnLoad \
    Java_com_zane_zanebox_core_ExtraCoreNative_nativeVersion \
    Java_com_zane_zanebox_core_ExtraCoreNative_nativeValidate \
    Java_com_zane_zanebox_core_ExtraCoreNative_nativeStart \
    Java_com_zane_zanebox_core_ExtraCoreNative_nativeStop; do
    printf '%s\n' "$dyn_syms" | grep -Fq "$sym" \
      || die "missing JNI export $sym in $EXTRA_CORES_SO"
  done
  mod_info="$("$EXTRA_GO_BIN" version -m "$EXTRA_CORES_SO")"
  printf '%s\n' "$mod_info" | grep -Fq "github.com/metacubex/mihomo	$EXTRA_CORES_MIHOMO_VERSION" \
    || die "libextra_cores.so mihomo version mismatch (expected $EXTRA_CORES_MIHOMO_VERSION)"
  printf '%s\n' "$mod_info" | grep -Fq "github.com/xtls/libxray	$EXTRA_CORES_LIBXRAY_VERSION" \
    || die "libextra_cores.so libxray version mismatch (expected $EXTRA_CORES_LIBXRAY_VERSION)"
  printf '%s\n' "$mod_info" | grep -Fq "github.com/xtls/xray-core" \
    || die "libextra_cores.so missing xray-core module info"
  printf '%s\n' "$mod_info" | grep -Fq "$EXTRA_CORES_XRAY_REV" \
    || die "libextra_cores.so xray-core revision mismatch (expected $EXTRA_CORES_XRAY_REV)"
}

build_extra_cores() {
  [ -d "$EXTRA_CORES_ROOT" ] || return 0
  local input_hash previous_hash so_tmp final_input_hash ndk_host cc_bin
  input_hash="$(extra_cores_input_hash)"
  previous_hash=""
  [ -f "$EXTRA_CORES_HASH_FILE" ] && previous_hash="$(sed -n '1p' "$EXTRA_CORES_HASH_FILE")"

  if [ "${FORCE_BUILD:-0}" != 1 ] && [ -s "$EXTRA_CORES_SO" ] && [ "$input_hash" = "$previous_hash" ]; then
    log "extra-cores sources unchanged; reusing $EXTRA_CORES_SO"
  else
    mkdir -p "$EXTRA_CORES_BUILD_DIR" "$(dirname "$EXTRA_CORES_SO")"
    ndk_host="$(find_ndk_host)"
    cc_bin="$ndk_host/bin/aarch64-linux-android${ANDROID_API}-clang"
    so_tmp="$EXTRA_CORES_BUILD_DIR/libextra_cores.new.so"
    rm -f "$so_tmp" "${so_tmp%.so}.h"
    log "building extra-cores (mihomo $EXTRA_CORES_MIHOMO_VERSION + xray-core v26.11.9) (hash $input_hash)"
    (
      cd "$EXTRA_CORES_ROOT"
      CGO_ENABLED=1 \
      GOOS=android \
      GOARCH=arm64 \
      CC="$cc_bin" \
      GOTOOLCHAIN=local \
      GOFLAGS=-mod=readonly \
      "$EXTRA_GO_BIN" build \
        -buildmode=c-shared \
        -trimpath \
        -buildvcs=false \
        -ldflags="$EXTRA_CORES_LDFLAGS" \
        -o "$so_tmp" \
        ./cmd/extra
    )
    rm -f "${so_tmp%.so}.h"
    [ -s "$so_tmp" ] || die "go build did not produce $so_tmp"
    mv -f "$so_tmp" "$EXTRA_CORES_SO"
  fi

  verify_extra_cores
  final_input_hash="$(extra_cores_input_hash)"
  [ "$final_input_hash" = "$input_hash" ] \
    || die "extra-cores sources changed during build/check; rerun against one stable snapshot"
  printf '%s\n' "$input_hash" > "$EXTRA_CORES_HASH_FILE"
  log "extra-cores artifact verified: $EXTRA_CORES_SO"
}

build_native() {
  local input_hash previous_hash aar_tmp final_input_hash
  input_hash="$(native_input_hash)"
  previous_hash=""
  [ -f "$INPUT_HASH_FILE" ] && previous_hash="$(sed -n '1p' "$INPUT_HASH_FILE")"

  if [ "${FORCE_BUILD:-0}" != 1 ] && [ -s "$AAR" ] && [ "$input_hash" = "$previous_hash" ]; then
    log "native sources unchanged; reusing $AAR"
  else
    mkdir -p "$BUILD_DIR/android"
    aar_tmp="$BUILD_DIR/libcore.new.aar"
    rm -f "$aar_tmp"
    log "building current native sources with gomobile (hash $input_hash)"
    (
      cd "$LIBCORE_ROOT"
      "$GOMOBILE_BIN" bind \
        -target=android/arm64 \
        -androidapi "$ANDROID_API" \
        -cache "$BUILD_DIR/android" \
        -trimpath \
        -ldflags="$NATIVE_LDFLAGS" \
        -tags="$NATIVE_TAGS" \
        -o "$aar_tmp" \
        .
    )
    [ -s "$aar_tmp" ] || die "gomobile did not produce $aar_tmp"
    mv -f "$aar_tmp" "$AAR"
  fi

  extract_artifact
  check_legacy_abi
  final_input_hash="$(native_input_hash)"
  [ "$final_input_hash" = "$input_hash" ] \
    || die "native sources changed during build/check; rerun against one stable snapshot"
  # Record the hash only after the AAR, ELF and 285-member ABI checks pass.
  printf '%s\n' "$input_hash" > "$INPUT_HASH_FILE"
  log "native artifact verified: $AAR"
}

ACTION=build
FORCE_BUILD=0
case "${1:-}" in
  '') ;;
  --check) ACTION=check ;;
  --force) FORCE_BUILD=1 ;;
  --extra-only) ACTION=extra ;;
  --help|-h)
    cat <<'EOF'
用法：native/build-native.sh [--check|--force|--extra-only]

--check 只检查固定 Go/NDK/gomobile 工具链，不编译。
默认按源码哈希增量编译 libcore (sing-box) 与 extra-cores (mihomo + xray-core)，并核验 AAR、ELF 16 KiB 对齐、libcore ABI 及 JNI 导出符号。
--force 忽略源码哈希，强制重新生成 libcore.aar 与 libextra_cores.so。
--extra-only 仅编译并核验 extra-cores (libextra_cores.so)。
EOF
    exit 0
    ;;
  *) die "unknown argument: $1" ;;
esac

check_toolchain
[ "$ACTION" = check ] && exit 0
if [ "$ACTION" = extra ]; then
  build_extra_cores
  exit 0
fi
build_native
build_extra_cores
