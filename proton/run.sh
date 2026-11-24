#!/usr/bin/env bash
# VIctory under Proton: run the Windows build of victory inside a Proton
# prefix, which supplies exactly what victory hosts on — a Win32 process
# environment with real D3D12 (via dxvk/vkd3d-proton). See docs/proton.md.
#
# The executable is built here with the gnu target (cargo win); nothing in
# this script touches the game itself.
set -euo pipefail

usage() {
    cat <<'USAGE'
usage: proton/run.sh [options] [--] [victory args...]

  --prefix DIR   Proton/Wine prefix (default: $XDG_DATA_HOME/victory/proton)
  --exe FILE     victory.exe to run (default: the cargo win output under target/)
  --proton DIR   Proton directory, or a plain wine binary
                 (default: newest GE-Proton under Steam's compatibilitytools.d)
  --z DIR        link DIR into the prefix's Z: drive as Z:\<basename>
  --config NAME  victory config to use, relative to the repo root
                 (default: victory.toml if present; see docs/proton.md)
  --texture-no-wait
                 set VICTORY_TEXTURE_NO_WAIT=1: the texture lane stops
                 waiting on the title's remap lock under Wine (#178);
                 costs up to two textures a frame, deferred one frame

Examples:
  proton/run.sh config
  proton/run.sh --z /mnt/games inspect "Z:\\game\\scripts\\x64\\final.js.c.c.gmpc"
USAGE
}

repo="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
prefix="${VICTORY_PROTON_PREFIX:-${XDG_DATA_HOME:-$HOME/.local/share}/victory/proton}"
exe="${VICTORY_EXE:-$repo/target/x86_64-pc-windows-gnu/release/victory.exe}"
proton="${PROTON:-}"
z_dir=""
config=""
texture_no_wait=""

while [[ $# -gt 0 && $1 != -- ]]; do
    case "$1" in
        --prefix) prefix=$2; shift 2 ;;
        --exe)    exe=$2; shift 2 ;;
        --proton) proton=$2; shift 2 ;;
        --z)      z_dir=$2; shift 2 ;;
        --config) config=$2; shift 2 ;;
        --texture-no-wait) texture_no_wait=1; shift ;;
        -h|--help) usage; exit 0 ;;
        *)      break ;;
    esac
done
[[ $# -gt 0 && $1 == -- ]] && shift
[[ $# -gt 0 ]] || set -- config

die() { echo "proton/run.sh: $*" >&2; exit 1; }

# Newest GE-Proton, then any Proton-*, from Steam's compatibility tools.
if [[ -z $proton ]]; then
    tools=${STEAM_COMPAT_TOOLS:-$HOME/.local/share/Steam/compatibilitytools.d}
    if [[ -d $tools ]]; then
        newest=$(find "$tools" -maxdepth 1 -mindepth 1 -type d \
            \( -name 'GE-Proton*' -o -name 'Proton-*' \) -printf '%f\n' \
            | sort -V | tail -n1)
        [[ -n $newest ]] && proton=$tools/$newest
    fi
fi
[[ -n $proton ]] || die "no Proton found; install one under compatibilitytools.d or pass --proton"

[[ -x "$proton/proton" || -x "$proton" ]] || die "$proton is neither a Proton directory nor an executable"
[[ -f $exe ]] || die "no $exe — build it with: cargo win"

# Where the wine prefix really lives: the proton script derives it from
# STEAM_COMPAT_DATA_PATH (a `pfx` dir inside it), silently over riding the
# WINEPREFIX we set. Probe the script's own answer, and fall back through
# the Steam convention to our plain prefix.
if [[ -x "$proton/proton" ]]; then
    real="$("$proton/proton" getnativepath 'C:\windows\system32' 2>/dev/null || true)"
else
    real=""
fi
[[ -n $real && -d $real ]] || real=${prefix}/pfx
[[ -d $real ]] || real=$prefix

# Z:'s root. New Wine prefixes map it to <prefix>/drive_z, and we curate
# that: game folders linked in under --z appear as Z:\<name>. GE-Proton 10+
# instead maps z: to `/` - the whole host filesystem is visible under Z: -
# which also works (game_dir becomes e.g. Z:\mnt\games\title), so in that
# mode --z is a no-op and the docs say so.
zlink=$real/dosdevices/z:
if [[ -e $zlink && ! -L $zlink ]]; then
    mv "$zlink" "$real/drive_z"
    ln -sfn -- drive_z "$zlink"
fi
if [[ ! -e $zlink ]]; then
    mkdir -p "$real/drive_z"
    ln -sfn -- drive_z "$zlink"
fi
zroot=$(readlink -f -- "$zlink")
if [[ -n $z_dir && $zroot != / ]]; then
    mkdir -p "$zroot"
    ln -sfn -- "$z_dir" "$zroot/$(basename -- "$z_dir")"
fi

# Steam uses these when launching Proton; the proton script wants them even
# when driven by hand.
export WINEPREFIX=$prefix
export STEAM_COMPAT_DATA_PATH=$prefix
export STEAM_COMPAT_CLIENT_INSTALL_PATH=${STEAM_COMPAT_CLIENT_INSTALL_PATH:-$HOME/.steam/Steam}
# Proton injects through steam.exe unless it sees a UMU launch; that inject
# forks (GUI convention) and our output dies with the orphan. UMU_ID marks
# this as a non-Steam launch, so the script runs wine directly, waits, and
# stdout/exit code come back to this terminal.
export UMU_ID=victory

# Wine's built-in d3d12/dxgi stubs answer E_INVALIDARG and draw nothing;
# the GPU path needs the dxvk/vkd3d-proton copies Proton syncs into the
# prefix as native. Recent Proton forces this itself; saying it too keeps
# hand-made prefixes behaving the same (docs/proton.md, #41).
export WINEDLLOVERRIDES=${WINEDLLOVERRIDES:-"d3d12=n;d3d12core=n;dxgi=n"}

# The texture lane's remap-lock wait is a Wine-side mapping lock that
# costs seconds while the world streams; the flag trades up to two
# textures a frame for it (#178).
if [[ -n $texture_no_wait ]]; then
    export VICTORY_TEXTURE_NO_WAIT=1
fi

# Wine resolves the process' Unix cwd through its Z: mapping, so a config
# given relative to the repo root works on both sides unchanged.
cd "$repo"
if [[ -n $config ]]; then
    [[ -f $config ]] || die "no $repo/$config"
    export VICTORY_CONFIG=$config
fi

# A bare wine binary (not a Proton dir) gets the exe as-is: no dxvk/vkd3d
# sync, so file-side commands only.
if [[ ! -x "$proton/proton" ]]; then
    exec "$proton" "$exe" "$@"
fi

# Ask Proton for the exe's own Windows-side (Z:) path: the script takes the
# direct-wine branch only for argv that is not a Unix absolute path.
wexe=$("$proton/proton" getcompatpath "$exe" 2>/dev/null)
[[ -n $wexe ]] || die "proton getcompatpath failed for $exe"
exec "$proton/proton" waitforexitandrun "$wexe" "$@"
