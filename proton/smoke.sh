#!/usr/bin/env bash
# End-to-end check of the Proton path (docs/proton.md). Needs `cargo win`
# and a Proton install; the checks are what a bug report against the Proton
# path should be able to say it saw. Exits non-zero on the first failure.
set -uo pipefail

repo="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo"

config=${VICTORY_CONFIG:-victory.proton.toml}
[[ -f $config ]] || {
    echo "no $config — copy victory.example.toml to it and fix the paths (docs/proton.md)" >&2
    exit 1
}

ok()   { echo "ok       $*"; }
fail() { echo "FAILED   $*"; exit 1; }

# 1. config resolves, and names Wine as the host.
out=$(proton/run.sh --config "$config" config 2>/dev/null) || fail "victory config exited non-zero"
grep -q "^config file" <<<"$out" || fail "config output has no 'config file' line: $out"
grep -q "^host .*Wine" <<<"$out" || fail "no 'host Wine' banner under Wine: $out"
ok "config banner and paths"

# 2. game_dir is reachable through the prefix.
grep "^game_dir" <<<"$out" | grep -q "does not exist" &&
    fail "game_dir not visible from the prefix: $(grep '^game_dir' <<<"$out")"
ok "game_dir reachable"

# 3. a real file-side command against the title: census.
out=$(proton/run.sh --config "$config" census 2>/dev/null) || fail "victory census exited non-zero"
[[ -n $out ]] || fail "census printed nothing"
ok "census over game_dir"

echo "proton smoke: all checks passed"
