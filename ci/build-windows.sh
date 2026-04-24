#!/usr/bin/env bash
# Build the Windows release payload + wizard from a Linux/macOS host.
#
# Produces, under `target/dist/`:
#   gadarah.exe                  — headless CLI / daemon
#   gadarah-gui.exe              — desktop GUI
#   gadarah-wizard.exe           — installation wizard (bootstrapper)
#   config/firms/*.toml          — shipped firm presets
#   README.md
#   LICENSE-MIT, LICENSE-APACHE
#   payload.zip                  — zip used by the wizard at runtime
#
# Requirements (one of):
#   (a) mingw-w64 toolchain:  pacman -S mingw-w64-gcc  (Arch AUR)
#                             or apt install mingw-w64   (Debian/Ubuntu)
#   (b) zig + cargo-zigbuild: pacman -S zig
#                             cargo install cargo-zigbuild
# And in all cases:
#   rustup target add x86_64-pc-windows-gnu

set -euo pipefail

TARGET="x86_64-pc-windows-gnu"
DIST="target/dist"
mkdir -p "$DIST" "$DIST/config" "$DIST/config/firms"

if command -v x86_64-w64-mingw32-gcc >/dev/null 2>&1; then
    BUILDER=(cargo build --release --target "$TARGET")
elif command -v cargo-zigbuild >/dev/null 2>&1 && command -v zig >/dev/null 2>&1; then
    BUILDER=(cargo zigbuild --release --target "$TARGET")
else
    cat >&2 <<EOF
No Windows cross-linker available. Install one of:
  - mingw-w64-gcc (provides x86_64-w64-mingw32-gcc)
  - zig + cargo-zigbuild

See .cargo/config.toml for the linker names this project expects.
EOF
    exit 1
fi

echo "[1/4] Building gadarah (CLI) + gadarah-gui"
"${BUILDER[@]}" -p gadarah-cli -p gadarah-gui

cp "target/$TARGET/release/gadarah.exe" "$DIST/"
cp "target/$TARGET/release/gadarah-gui.exe" "$DIST/"
cp -r config/firms/*.toml "$DIST/config/firms/"
cp config/gadarah.toml "$DIST/config/"
cp wix/install_ollama.ps1 "$DIST/"
[ -f README.md ] && cp README.md "$DIST/"

echo "[2/4] Packing payload.zip"
PAYLOAD="$PWD/$DIST/payload.zip"
rm -f "$PAYLOAD"
if command -v zip >/dev/null 2>&1; then
    (cd "$DIST" && zip -qr "$PAYLOAD" gadarah.exe gadarah-gui.exe install_ollama.ps1 config/)
else
    python3 - "$DIST" "$PAYLOAD" <<'PY'
import os, sys, zipfile
dist, out = sys.argv[1], sys.argv[2]
include = ["gadarah.exe", "gadarah-gui.exe", "install_ollama.ps1", "config"]
with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
    for entry in include:
        path = os.path.join(dist, entry)
        if not os.path.exists(path):
            continue
        if os.path.isfile(path):
            z.write(path, arcname=entry)
            continue
        for root, _dirs, files in os.walk(path):
            for name in files:
                full = os.path.join(root, name)
                rel = os.path.relpath(full, dist)
                z.write(full, arcname=rel)
PY
fi

echo "[3/4] Building gadarah-wizard with embedded payload"
GADARAH_WIZARD_PAYLOAD="$PAYLOAD" "${BUILDER[@]}" -p gadarah-wizard

cp "target/$TARGET/release/gadarah-wizard.exe" "$DIST/"

echo "[4/4] Done. Artifacts in $DIST/"
ls -lh "$DIST"/*.exe "$DIST/payload.zip"
