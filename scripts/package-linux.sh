#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(uname -m) == x86_64 ]] || { echo 'Only x86_64 packaging is supported.' >&2; exit 1; }
version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)
name="QRiff-${version}-linux-x86_64"
staging=$(mktemp -d)
trap 'rm -rf "$staging"' EXIT
appdir="$staging/$name"
mkdir -p "$appdir/usr/bin" "$appdir/usr/share/licenses/qriff" dist
install -m755 target/release/qriff "$appdir/usr/bin/qriff"
install -m755 "$(command -v wl-copy)" "$appdir/usr/bin/wl-copy"
install -m644 LICENSE "$appdir/usr/share/licenses/qriff/LICENSE"
# Preserve the bundled clipboard helper's copyright and license notices.
install -m644 /usr/share/doc/wl-clipboard/copyright "$appdir/usr/share/licenses/qriff/wl-clipboard-copyright"
cat > "$appdir/AppRun" <<'RUN'
#!/usr/bin/env bash
set -euo pipefail
APPDIR=$(cd -- "$(dirname -- "$(readlink -f -- "$0")")" && pwd)
export PATH="$APPDIR/usr/bin:$PATH"
export LD_LIBRARY_PATH="$APPDIR/usr/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export XDG_DATA_DIRS="$APPDIR/usr/share:${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"
exec "$APPDIR/usr/bin/qriff" "$@"
RUN
chmod +x "$appdir/AppRun"
# Fail closed if the upstream continuous download changes. Update the reviewed
# digest deliberately when upgrading linuxdeploy.
linuxdeploy="$staging/linuxdeploy.AppImage"
curl --fail --location --retry 3 -o "$linuxdeploy" https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage
echo "8aea8da0f7f7039d2a2cecb14657d752a222a5e1d3825caeef186c82f751cdd1  $linuxdeploy" | sha256sum --check
chmod +x "$linuxdeploy"
export APPIMAGE_EXTRACT_AND_RUN=1
export ARCH=x86_64
export OUTPUT="$PWD/dist/$name.AppImage"
# Iced loads these libraries at runtime, so ldd alone cannot discover them.
"$linuxdeploy" --appdir "$appdir" \
  --executable "$appdir/usr/bin/qriff" --executable "$appdir/usr/bin/wl-copy" \
  --desktop-file assets/qriff.desktop --icon-file assets/qriff.png \
  --library /usr/lib/x86_64-linux-gnu/libwayland-client.so.0 \
  --library /usr/lib/x86_64-linux-gnu/libwayland-cursor.so.0 \
  --library /usr/lib/x86_64-linux-gnu/libxkbcommon.so.0 \
  --library /usr/lib/x86_64-linux-gnu/libxkbcommon-x11.so.0 \
  --library /usr/lib/x86_64-linux-gnu/libX11.so.6 \
  --library /usr/lib/x86_64-linux-gnu/libXcursor.so.1 \
  --library /usr/lib/x86_64-linux-gnu/libXi.so.6 \
  --library /usr/lib/x86_64-linux-gnu/libXrandr.so.2 --output appimage
"$OUTPUT" --appimage-extract-and-run --help >/dev/null
"$appdir/AppRun" --help >/dev/null
"$appdir/usr/bin/wl-copy" --version
# The archive has the same bundled libraries and launcher, without requiring FUSE.
tar -C "$staging" -czf "dist/$name.tar.gz" "$name"
(cd dist && sha256sum "$name.AppImage" "$name.tar.gz" > SHA256SUMS)
