#!/bin/bash
# Runs the cross-compiled towerfall-port.exe under Proton's Wine, sandboxed
# so that nothing outside the project directory can be written:
#   - bubblewrap mounts / read-only (the game install included), the project
#     read-write, gives Wine a private /tmp (tools/wine-tmp; wineserver puts
#     its socket there) and a private in-memory /dev/shm;
#   - HOME/XDG dirs and the Wine prefix (tools/wineprefix) live in tools/.
#
# Display: Wine's Vulkan (winevulkan) needs a user driver, and with Wine's
# "null" graphics driver Vulkan instance creation hangs, so Wine uses the
# X11 driver on the project's private Xvfb (scripts/dev/xvfb.sh, :64),
# never the user's desktop. Rendering then goes through wgpu's Vulkan
# backend -> winevulkan -> the host's Mesa (llvmpipe on Xvfb).
#
# Usage: scripts/run-windows.sh [towerfall-port args...]
#   e.g. scripts/run-windows.sh --headless --frames 10 --screenshot scratch/win.png
# Env: PROTON_DIR (default: Steam's "Proton - Experimental"), EXE (default:
# target/dist/windows/towerfall-port.exe), WINEDEBUG (default -all),
# TFP_WINE_DISPLAY (default :64).
set -euo pipefail
source "$(dirname "$0")/../env.sh"
cd "$TFP_ROOT"

PROTON_DIR=${PROTON_DIR:-"$HOME/.steam/steam/steamapps/common/Proton - Experimental"}
WINE="$PROTON_DIR/files/bin/wine"
EXE=${EXE:-target/dist/windows/towerfall-port.exe}
[ -x "$WINE" ] || { echo "Proton wine not found at $WINE" >&2; exit 1; }
[ -f "$EXE" ] || { echo "$EXE missing: run scripts/build-windows.sh" >&2; exit 1; }

DISPLAY_NUM=$(scripts/dev/xvfb.sh "${TFP_WINE_DISPLAY:-:64}")

T="$TFP_ROOT/tools"
mkdir -p "$T/wine-home" "$T/wine-tmp" "$T/wine-xdg/config" "$T/wine-xdg/data" "$WINEPREFIX"

# Windows paths for the game data (Wine maps / to Z:).
winpath() { printf 'Z:%s' "${1//\//\\}"; }
game_env=()
[ -d "$TF_GAME_DIR" ] && game_env+=(--setenv TF_GAME_DIR "$(winpath "$TF_GAME_DIR")")
[ -d "$TF8_GAME_DIR" ] && game_env+=(--setenv TF8_GAME_DIR "$(winpath "$TF8_GAME_DIR")")

sandbox() {
    bwrap \
        --ro-bind / / \
        --dev-bind /dev /dev \
        --tmpfs /dev/shm \
        --proc /proc \
        --bind "$TFP_ROOT" "$TFP_ROOT" \
        --bind "$T/wine-tmp" /tmp \
        --ro-bind "/tmp/.X11-unix/X${DISPLAY_NUM#:}" "/tmp/.X11-unix/X${DISPLAY_NUM#:}" \
        --setenv DISPLAY "$DISPLAY_NUM" \
        --unsetenv WAYLAND_DISPLAY \
        --setenv HOME "$T/wine-home" \
        --setenv XDG_CONFIG_HOME "$T/wine-xdg/config" \
        --setenv XDG_DATA_HOME "$T/wine-xdg/data" \
        --setenv XDG_CACHE_HOME "$XDG_CACHE_HOME" \
        --setenv WINEPREFIX "$WINEPREFIX" \
        --setenv WINEDLLPATH "$PROTON_DIR/files/lib/vkd3d:$PROTON_DIR/files/lib/wine" \
        --setenv LD_LIBRARY_PATH "$PROTON_DIR/files/lib/x86_64-linux-gnu:$PROTON_DIR/files/lib/i386-linux-gnu" \
        --setenv WINEDLLOVERRIDES "winemenubuilder.exe=d;mscoree=d;mshtml=d;dxgi=n" \
        --setenv WINEDEBUG "${WINEDEBUG:--all}" \
        --setenv SDL_AUDIODRIVER "${SDL_AUDIODRIVER:-dummy}" \
        "${game_env[@]}" \
        --chdir "$TFP_ROOT" \
        "$WINE" "$@"
}

# First run creates the prefix.
if [ ! -f "$WINEPREFIX/system.reg" ]; then
    sandbox wineboot -u
fi

# wgpu's exe imports dxgi.dll. Wine's own dxgi needs wined3d + vkd3d, which
# Proton only wires up through its launcher; like Proton does for games, use
# DXVK's dxgi.dll (Vulkan-based) inside the prefix.
SYS32="$WINEPREFIX/drive_c/windows/system32"
DXVK_DXGI="$PROTON_DIR/files/lib/wine/dxvk/x86_64-windows/dxgi.dll"
cmp -s "$DXVK_DXGI" "$SYS32/dxgi.dll" || cp -f "$DXVK_DXGI" "$SYS32/dxgi.dll"

sandbox "$EXE" "$@"
