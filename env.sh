# Source this: keeps every toolchain and cache inside the project directory.
R="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export TFP_ROOT="$R"
export RUSTUP_HOME="$R/tools/rustup" CARGO_HOME="$R/tools/cargo"
export DOTNET_ROOT="$R/tools/dotnet" DOTNET_CLI_HOME="$R/tools/dotnet-home"
export NUGET_PACKAGES="$R/tools/nuget" DOTNET_CLI_TELEMETRY_OPTOUT=1 DOTNET_NOLOGO=1 DOTNET_ROLL_FORWARD=Major
export npm_config_cache="$R/tools/npm-cache" PIP_CACHE_DIR="$R/tools/pip-cache"
export XDG_CACHE_HOME="$R/tools/xdg-cache" WINEPREFIX="$R/tools/wineprefix"
export PATH="$R/tools/cargo/bin:$R/tools/dotnet:$R/tools/dotnet-home/.dotnet/tools:$R/tools/venv/bin:$PATH"
# The original game (read-only; never written to): Steam copy (with Dark World)
# and the official 8-player build (reference for >4 player layouts).
# Read-only /data/towerfall{,8} (a sandboxed build worker) win over the defaults.
if [ -z "${TF_GAME_DIR:-}" ] && [ -d /data/towerfall ]; then TF_GAME_DIR=/data/towerfall; fi
if [ -z "${TF8_GAME_DIR:-}" ] && [ -d /data/towerfall8 ]; then TF8_GAME_DIR=/data/towerfall8; fi
export TF_GAME_DIR="${TF_GAME_DIR:-$HOME/.steam/steam/steamapps/common/TowerFall}"
export TF8_GAME_DIR="${TF8_GAME_DIR:-$HOME/Gaming/TowerFall}"
# Dev: the port's user data dir (import cache, settings, Warpzone identity) stays in the project.
export TFP_DATA_DIR="$R/cache/userdata"
export WARPZONE_HOME="$R/cache/warpzone-home"
# Temporary files stay in the project too (and /tmp may be noexec, which
# breaks rustdoc's doctest binaries).
mkdir -p "$R/scratch/tmp" && export TMPDIR="$R/scratch/tmp"
