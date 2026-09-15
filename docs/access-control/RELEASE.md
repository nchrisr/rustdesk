# Producing RustDesk_Velour deliverables

Two files, both unsigned (Windows shows "Run anyway" once; macOS needs
right-click → Open once):

* `RustDesk_Velour-windows-x64.zip` — folder `RustDesk_Velour` with
  `rustdesk.exe` and its DLLs (portable; Install button inside the app).
* `RustDesk_Velour-macos-arm64.zip` — `RustDesk_Velour.app` for Apple Silicon.

## Windows (GitHub builds it)

1. Push `master`.
2. github.com → fork → Actions → **Velour Windows build** → Run workflow →
   branch `master`.
3. Download the artifact **RustDesk_Velour-windows-x64** when green.

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
cd dist && ditto -c -k --keepParent RustDesk_Velour.app RustDesk_Velour-macos-arm64.zip
```

Check before shipping: `nm -gU dist/RustDesk_Velour.app/Contents/Frameworks/liblibrustdesk.dylib | grep -c velour`
prints a non-zero number (the embedded library is the Velour one). `dist/` is
git-ignored.

An Intel-Mac build needs an x86_64 vcpkg install and toolchain; not set up.
