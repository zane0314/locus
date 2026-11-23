# Locus

Precision multi-core Android proxy client powered by a unified **Rust** business engine (`locus_core`), three proxy kernels (**sing-box**, **Xray-core**, **mihomo**), and a **Linear Obsidian** Jetpack Compose UI.

## Architecture

- **Rust Core (`rust/locus_core`)**: All business, parsing, routing, chain derivation, configuration compilation, backup selection, multi-core planning, and runtime diagnostics logic compiled to 16 KiB-aligned `arm64-v8a` `liblocus_core.so` (Android) and host `liblocus_core.dylib` (JVM unit tests).
- **Three Proxy Engines**:
  - `sing-box` (`libcore.aar` / `libgojni.so`): default native TUN, routing, DNS, FakeIP, and outbound engine.
  - `Xray` & `mihomo` (`libextra_cores.so`, 16 KiB-aligned): isolated `:extraMain` / `:extraTest` AIDL services orchestrated by Rust `multicore_planner`.
- **Android Shell (`app/`)**: Thin Kotlin/Compose UI (`applicationId = com.zane.locus`), VPN/Proxy foreground services, SQLite store, and Quick Settings tile.

## Build & Verification

```bash
# 1. Run Rust unit tests
cd rust/locus_core && cargo test

# 2. Build host dylib + Android arm64-v8a 16 KiB-aligned .so
bash rust/build-rust.sh all

# 3. Run all JUnit tests against the real Rust JNI library
./gradlew :app:testDebugUnitTest --no-daemon

# 4. Assemble debug & signed release APKs
./gradlew :app:assembleDebug :app:assembleRelease --no-daemon
```
