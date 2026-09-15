# Producing RustDesk_Velour deliverables

## Versioning

Two numbers, shown together on the About page as
"Edition: RustDesk-Velour 0.1" and "Version: 1.5.0":

* **RustDesk version** (`Cargo.toml`, `src/version.rs`, `flutter/pubspec.yaml`)
  — tracks upstream; peers exchange it and gate features on it. Do not
  invent values; change it only when rebasing on a newer RustDesk.
* **Velour version** — `VELOUR_VERSION` in `src/common.rs`, one constant,
  bumped by hand for each Velour release (0.1, 0.2 … 1.0). It goes into the
  deliverable names.

To cut a release: edit `VELOUR_VERSION`, commit, push `master`, then build
both files below.

Two files, both unsigned (Windows shows "Run anyway" once; macOS needs
right-click → Open once):

* `RustDesk_Velour-<version>-windows-x64.zip` — folder `RustDesk_Velour` with
  `rustdesk.exe` and its DLLs (portable; Install button inside the app).
* `RustDesk_Velour-<version>-macos-arm64.zip` — `RustDesk_Velour.app` for
  Apple Silicon.

## Windows (GitHub builds it)

1. Push `master`.
2. github.com → fork → Actions → **Velour Windows build** → Run workflow →
   branch `master`.
3. Download the artifact **RustDesk_Velour-<version>-windows-x64** when green.

## macOS (built on this Mac)

```sh
cd ~/My-Projects/velour/RustDesk/rustdesk
export VCPKG_ROOT=$HOME/vcpkg
export LIBCLANG_PATH=/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/lib
export PATH=$HOME/flutter-3.24.5/bin:$PATH
cargo build --features flutter --lib --release
cd flutter && FLUTTER_XCODE_ARCHS=arm64 FLUTTER_XCODE_ONLY_ACTIVE_ARCH=YES flutter build macos --release && cd ..
mkdir -p dist && rm -rf dist/RustDesk_Velour.app
cp -R flutter/build/macos/Build/Products/Release/RustDesk.app dist/RustDesk_Velour.app
V=$(grep -oE 'VELOUR_VERSION: &str = "[^"]+"' src/common.rs | grep -oE '[0-9][^"]*')
cd dist && ditto -c -k --keepParent RustDesk_Velour.app RustDesk_Velour-$V-macos-arm64.zip
```

Check before shipping: `nm -gU dist/RustDesk_Velour.app/Contents/Frameworks/liblibrustdesk.dylib | grep -c velour`
prints a non-zero number (the embedded library is the Velour one). `dist/` is
git-ignored.

An Intel-Mac build needs an x86_64 vcpkg install and toolchain; not set up.
