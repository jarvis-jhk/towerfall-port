#!/bin/bash
# Builds the release archives in dist/ (nothing is published):
#   dist/towerfall-port-<ver>-linux-x86_64.tar.gz   binary (rpath $ORIGIN/lib), lib/libSDL2-2.0.so.0
#   dist/towerfall-port-<ver>-windows-x86_64.zip    towerfall-port.exe, SDL2.dll
#   dist/SHA256SUMS
# Both contain README.md (packaging/README-player.md), the licences and
# THIRD-PARTY/ (SDL2's licence, the Rust crates with their licences).
# Then checks that nothing from the game is inside (an allowlist of file
# names plus a deny list of game file types) and prints the file lists.
# Usage: scripts/package.sh [--linux-only | --windows-only]
set -euo pipefail
source "$(dirname "$0")/../env.sh"
cd "$TFP_ROOT"
WANT_LINUX=1; WANT_WIN=1
case "${1:-}" in
  --linux-only) WANT_WIN=0 ;;
  --windows-only) WANT_LINUX=0 ;;
  "") ;;
  *) echo "usage: $0 [--linux-only | --windows-only]" >&2; exit 2 ;;
esac
VER=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
DIST=dist
STAGE=target/package/stage
mkdir -p "$DIST"
rm -rf "$STAGE"
# Release builds for distribution: stripped, no debug info, in their own
# target dir so the development builds stay cached.
export CARGO_TARGET_DIR="$TFP_ROOT/target/package"
export CARGO_PROFILE_RELEASE_DEBUG=false CARGO_PROFILE_RELEASE_STRIP=true

common_files() { # $1 = staging dir
  local d=$1; shift
  cp packaging/README-player.md "$d/README.md"
  cp LICENSE-MIT LICENSE-APACHE "$d/"
  mkdir -p "$d/THIRD-PARTY"
  cp tools/windows/sdl2/LICENSE.txt "$d/THIRD-PARTY/SDL2-LICENSE.txt"
  {
    echo "Rust crates compiled into towerfall-port (name version licence)."
    echo "The MIT and Apache-2.0 texts are LICENSE-MIT / LICENSE-APACHE."
    echo
    cargo tree -p towerfall-port -e normal --prefix none --format '{p} {l}' "$@" 2>/dev/null \
      | sed 's/ (.*)//' | sort -u | grep -v '^towerfall-port \|^tfp-'
  } > "$d/THIRD-PARTY/crates.txt"
}

if [ "$WANT_LINUX" = 1 ]; then
  NAME="towerfall-port-$VER-linux-x86_64"
  D="$STAGE/$NAME"
  RUSTFLAGS='-C link-arg=-Wl,-rpath,$ORIGIN/lib' cargo build --release -p towerfall-port
  BIN="$CARGO_TARGET_DIR/release/towerfall-port"
  mkdir -p "$D/lib"
  cp "$BIN" "$D/"
  SDL=$(ldd "$BIN" | awk '/libSDL2-2.0.so.0/ {print $3}')
  [ -n "$SDL" ] || { echo "libSDL2 not found by ldd" >&2; exit 1; }
  cp -L "$SDL" "$D/lib/libSDL2-2.0.so.0"
  readelf -d "$D/towerfall-port" | grep -q 'RUNPATH.*\$ORIGIN/lib' \
    || { echo "missing RUNPATH \$ORIGIN/lib" >&2; exit 1; }
  common_files "$D"
  tools/venv/bin/python -I - "$STAGE" "$NAME" "$DIST/$NAME.tar.gz" <<'EOF'
import sys, tarfile, os
stage, name, out = sys.argv[1:]
def norm(t):
    t.uid = t.gid = 0; t.uname = t.gname = ""; t.mtime = 0
    if t.isdir() or t.name.endswith(("/towerfall-port", ".so.0")): t.mode = 0o755
    else: t.mode = 0o644
    return t
with tarfile.open(out, "w:gz") as tf:
    tf.add(os.path.join(stage, name), arcname=name, filter=norm)
EOF
fi

if [ "$WANT_WIN" = 1 ]; then
  [ -d tools/windows/llvm-mingw ] || scripts/fetch-windows-toolchain.sh
  NAME="towerfall-port-$VER-windows-x86_64"
  D="$STAGE/$NAME"
  ( source scripts/windows-env.sh
    cargo build --release --target x86_64-pc-windows-gnu -p towerfall-port )
  mkdir -p "$D"
  cp "$CARGO_TARGET_DIR/x86_64-pc-windows-gnu/release/towerfall-port.exe" "$D/"
  cp tools/windows/sdl2/x86_64-w64-mingw32/bin/SDL2.dll "$D/"
  common_files "$D" --target x86_64-pc-windows-gnu
  tools/venv/bin/python -I - "$STAGE" "$NAME" "$DIST/$NAME.zip" <<'EOF'
import sys, zipfile, os
stage, name, out = sys.argv[1:]
root = os.path.join(stage, name)
with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
    for dp, dn, fn in os.walk(root):
        dn.sort()
        for f in sorted(fn):
            p = os.path.join(dp, f)
            info = zipfile.ZipInfo(os.path.relpath(p, stage), (1980, 1, 1, 0, 0, 0))
            info.external_attr = 0o644 << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            with open(p, "rb") as fh:
                z.writestr(info, fh.read())
EOF
fi

# Nothing from the game: only the files we put there.
tools/venv/bin/python -I - "$DIST" "$VER" <<'EOF'
import sys, tarfile, zipfile, os, re
dist, ver = sys.argv[1:]
allowed = {"README.md", "LICENSE-MIT", "LICENSE-APACHE", "THIRD-PARTY/SDL2-LICENSE.txt",
           "THIRD-PARTY/crates.txt", "towerfall-port", "lib/libSDL2-2.0.so.0",
           "towerfall-port.exe", "SDL2.dll"}
deny = re.compile(r"\.(xnb|xwb|xsb|xgs|wav|ogg|mp3|png|xml|oel|tower|json|dll\.config)$|tf_save", re.I)
bad = 0
for f in sorted(os.listdir(dist)):
    p = os.path.join(dist, f)
    if f.endswith(".tar.gz"):
        names = [m.name for m in tarfile.open(p).getmembers() if m.isfile()]
    elif f.endswith(".zip"):
        names = zipfile.ZipFile(p).namelist()
    else:
        continue
    if ver not in f:
        continue
    print(f"{f}: {os.path.getsize(p) / 1e6:.1f} MB")
    for n in names:
        rel = n.split("/", 1)[1] if "/" in n else n
        ok = rel in allowed and not deny.search(rel)
        print(f"  {'ok ' if ok else 'BAD'} {n}")
        bad += not ok
if bad:
    sys.exit(f"{bad} unexpected file(s) in the archives")
print("check: only our own files (no game data)")
EOF
( cd "$DIST" && sha256sum towerfall-port-"$VER"-* > SHA256SUMS && cat SHA256SUMS )
