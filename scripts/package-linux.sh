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
  --library /usr/lib/x86_64-linux-gnu/libXrandr.so.2
# Preserve notices for bundled native libraries. Official builds also provide
# their exact distribution source packages alongside the executable downloads.
packages=(wl-clipboard)
for library in "$appdir"/usr/lib/*.so*; do
  base=$(basename "$library")
  owner=$(dpkg-query -S "/usr/lib/x86_64-linux-gnu/$base" "/lib/x86_64-linux-gnu/$base" 2>/dev/null | head -n 1 | cut -d: -f1 || true)
  [[ -n "$owner" ]] || { echo "Cannot identify bundled library: $base" >&2; exit 1; }
  packages+=("$owner")
done
mapfile -t packages < <(printf '%s\n' "${packages[@]}" | sort -u)
for package in "${packages[@]}"; do
  install -Dm644 "/usr/share/doc/$package/copyright" "$appdir/usr/share/licenses/qriff/$package-copyright"
done
if [[ ${1:-} == --sources ]]; then
  sources="$staging/native-sources"
  mkdir -p "$sources"
  for package in "${packages[@]}"; do
    source_version=$(dpkg-query -W -f='${source:Package}=${source:Version}' "$package")
    (cd "$sources" && apt-get source --download-only "$source_version")
  done
fi
# Keep Rust dependency notices with the app, and include original crate archives
# in the optional source download.
python3 - "$appdir" "$staging/native-sources" "${1:-}" <<'PYTHON'
import json, pathlib, shutil, subprocess, sys
appdir, sources = map(pathlib.Path, sys.argv[1:3])
metadata = json.loads(subprocess.check_output([
    "cargo", "metadata", "--locked", "--offline", "--format-version", "1",
    "--filter-platform", "x86_64-unknown-linux-gnu",
]))
for package in metadata["packages"]:
    if not (package.get("source") or "").startswith("registry+"):
        continue
    root = pathlib.Path(package["manifest_path"]).parent
    identity = f'{package["name"]}-{package["version"]}'
    dest = appdir / "usr/share/licenses/qriff/rust" / identity
    dest.mkdir(parents=True, exist_ok=True)
    (dest / "metadata.json").write_text(json.dumps({key: package.get(key) for key in
        ("name", "version", "license", "repository")}, indent=2) + "\n")
    for pattern in ("LICENSE*", "LICENCE*", "COPYING*", "NOTICE*"):
        for notice in root.glob(pattern):
            if notice.is_file():
                shutil.copy2(notice, dest / notice.name)
    if sys.argv[3] == "--sources":
        archive = root.parents[2] / "cache" / root.parent.name / f"{identity}.crate"
        crate_dest = sources / "rust-crates"
        crate_dest.mkdir(parents=True, exist_ok=True)
        shutil.copy2(archive, crate_dest / archive.name)
PYTHON
if [[ ${1:-} == --sources ]]; then
  tar -C "$staging" -czf "dist/$name-dependency-sources.tar.gz" native-sources
fi
"$linuxdeploy" --appdir "$appdir" --output appimage
APPIMAGELAUNCHER_DISABLE=1 "$OUTPUT" --appimage-extract-and-run --help >/dev/null
"$appdir/AppRun" --help >/dev/null
"$appdir/usr/bin/wl-copy" --version
# The archive has the same bundled libraries and launcher, without requiring FUSE.
tar -C "$staging" -czf "dist/$name.tar.gz" "$name"
(cd dist && sha256sum "$name.AppImage" "$name"*.tar.gz > SHA256SUMS)
