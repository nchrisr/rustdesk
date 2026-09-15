# Building RustDesk-Velour on macOS (Apple Silicon)

Verified 2026-09-14 on an M1 Pro, macOS 15 (Darwin 24.6), Xcode 15.0.1.
Every command below was run as written; nothing needs `sudo`.

## Machine-specific gotchas found on this Mac

Read these first; they explain choices below.

1. **Homebrew and the default Rust toolchain are the Intel (x86_64) builds**,
   running under Rosetta (`/usr/local/bin/brew`, `stable-x86_64-apple-darwin`).
   Everything RustDesk needs must be arm64: the Dart SDK is arm64, vcpkg
   builds arm64, and the Xcode project embeds a single-arch dylib. So:
   * use the `stable-aarch64-apple-darwin` Rust toolchain (set as a directory
     override, see step 3), and
   * never point tools at Homebrew's `libclang` (x86_64); use Xcode's universal
     one instead (`LIBCLANG_PATH` / `--llvm-path` below).
2. **Flutter must be 3.24.5**, the version the project's CI pins for macOS.
   The system Flutter (3.44.x) does not compile this codebase without source
   patches that the project only applies for Windows arm64. A dedicated
   checkout lives in `~/flutter-3.24.5` and is put first on `PATH` when
   building; the system Flutter is untouched.
3. The Xcode project links `target/release/liblibrustdesk.dylib`, so the app
   needs a **release** Rust build. `cargo test` uses the debug profile, so
   both profiles get built over time (~10–20 min each the first time).
4. If rust-analyzer (VS Code / Cursor Rust extension) is active on this
   folder it runs its own `cargo check` and fights the build for the target
   directory lock. Disable it for this workspace, or expect slower builds.

## One-time setup

```sh
# 1. vcpkg at the commit pinned in .github/workflows/flutter-build.yml
git clone https://github.com/microsoft/vcpkg.git ~/vcpkg
git -C ~/vcpkg checkout 9e593bb18ea69cc5095e012465dcd675a822ed0d
~/vcpkg/bootstrap-vcpkg.sh -disableMetrics

# 2. Native dependencies (aom, libvpx, libyuv, opus, ffmpeg, libjpeg-turbo ...).
#    Reads vcpkg.json in the repo root. ~30-45 min on an M1 Pro.
cd <repo>
export VCPKG_ROOT=$HOME/vcpkg VCPKG_DEFAULT_HOST_TRIPLET=arm64-osx
$VCPKG_ROOT/vcpkg install --triplet arm64-osx --x-install-root="$VCPKG_ROOT/installed"

# 3. arm64 Rust toolchain, as an override for this directory only
rustup toolchain install stable-aarch64-apple-darwin --force-non-host   # if not present
rustup override set stable-aarch64-apple-darwin                         # run inside <repo>
rustc -vV | grep host   # must print aarch64-apple-darwin

# 4. Flutter 3.24.5 in its own folder, with the two SDK patches CI applies
git clone --depth 1 -b 3.24.5 https://github.com/flutter/flutter.git ~/flutter-3.24.5
( cd ~/flutter-3.24.5 && git apply <repo>/.github/patches/flutter_3.24.4_dropdown_menu_enableFilter.diff )
sed -i '' -e 's/_setFramesEnabledState(false);/\/\/_setFramesEnabledState(false);/g' \
  ~/flutter-3.24.5/packages/flutter/lib/src/scheduler/binding.dart

# 5. Bridge code generator (version pinned in .github/workflows/bridge.yml)
cargo install flutter_rust_bridge_codegen --version 1.80.1 --features "uuid" --locked
```

## Environment for every build shell

```sh
export VCPKG_ROOT=$HOME/vcpkg
export LIBCLANG_PATH=/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/lib
export PATH=$HOME/flutter-3.24.5/bin:$PATH
```

## Generate the Flutter <-> Rust bridge

Needed after any change to `src/flutter_ffi.rs` (WP0 and later touch it).
Outputs are git-ignored.

```sh
cd <repo>/flutter && flutter pub get && cd ..
~/.cargo/bin/flutter_rust_bridge_codegen \
  --rust-input ./src/flutter_ffi.rs \
  --dart-output ./flutter/lib/generated_bridge.dart \
  --c-output ./flutter/macos/Runner/bridge_generated.h \
  --llvm-path /Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr
cp flutter/macos/Runner/bridge_generated.h flutter/ios/Runner/bridge_generated.h
```

## Build and run

```sh
# Rust library the app embeds (release, because Xcode links target/release/).
# First build ~18 min; incremental rebuilds are much faster.
cargo build --features flutter --lib --release

# Desktop app. FLUTTER_XCODE_ARCHS keeps Xcode single-arch to match the dylib.
cd flutter
FLUTTER_XCODE_ARCHS=arm64 FLUTTER_XCODE_ONLY_ACTIVE_ARCH=YES flutter build macos --debug
open build/macos/Build/Products/Debug/RustDesk.app

# Alternative for iterating on Dart code with hot reload:
FLUTTER_XCODE_ARCHS=arm64 FLUTTER_XCODE_ONLY_ACTIVE_ARCH=YES flutter run -d macos
```

Timings measured on the M1 Pro (first build): vcpkg ~20 min, cargo debug
6 min 54 s, cargo release 18 min 14 s, `flutter build macos --debug` ~3 min.

Note: `flutter pub get` rewrites `flutter/pubspec.lock`; CI regenerates it on
every build, so leave that change uncommitted.

## Tests

```sh
cargo test --features flutter            # Rust unit tests (debug profile)
cd flutter && flutter test               # Flutter widget tests
```
