#!/bin/sh
# Sets up Julia for `math-run run -s julia` on Linux (x86_64, aarch64) or
# macOS:
#   1. juliaup and the pinned Julia, made the default;
#   2. the pinned packages (julia/Project.toml and julia/Manifest.toml of this
#      repository, with julia/MathRunWarmup, whose precompilation runs a small
#      problem of each op so that Julia keeps their compiled code) as Julia's
#      shared environment `math-run`, which the runner uses (`julia
#      --project=@math-run`), instantiated and precompiled;
#   3. a smoke test: the version, and one problem, with the math-run given
#      as the first argument (or `math-run` on the PATH, or
#      target/release/math-run).
#
#   sh scripts/julia-setup.sh [path/to/math-run]
#
# It needs the network for juliaup and the packages (pkg.julialang.org), and
# about 3 GB of disk in ~/.julia. Times on the Mac (Apple M-series, at low
# priority) are in the README.
set -eu

JULIA_VERSION=1.12.6
here=$(cd "$(dirname "$0")/.." && pwd)

say() { printf '\n== %s\n' "$*"; }

say "juliaup and Julia $JULIA_VERSION"
export PATH="$HOME/.juliaup/bin:$PATH"
# a Julia of the pinned version on the PATH already is used as it is
if command -v julia >/dev/null 2>&1 && julia --version 2>/dev/null | grep -q " $JULIA_VERSION\$"; then
    echo "julia $JULIA_VERSION is on the PATH already"
else
    if ! command -v juliaup >/dev/null 2>&1; then
        curl -fsSL https://install.julialang.org | sh -s -- --yes
    fi
    juliaup add "$JULIA_VERSION"
    juliaup default "$JULIA_VERSION"
fi
julia --version

say "the packages, as the shared environment math-run"
env_dir="${JULIA_DEPOT_PATH:-$HOME/.julia}"
env_dir="${env_dir%%:*}/environments/math-run"
mkdir -p "$env_dir"
cp "$here/julia/Project.toml" "$here/julia/Manifest.toml" "$env_dir/"
rm -rf "$env_dir/MathRunWarmup"
cp -R "$here/julia/MathRunWarmup" "$env_dir/"
start=$(date +%s)
julia --startup-file=no --project=@math-run -e '
    using Pkg
    Pkg.instantiate()
    Pkg.precompile()
    Pkg.status()'
echo "instantiated and precompiled in $(( $(date +%s) - start )) s"

say "smoke test"
run=${1:-}
if [ -z "$run" ]; then
    if command -v math-run >/dev/null 2>&1; then
        run=math-run
    elif [ -x "$here/target/release/math-run" ]; then
        run="$here/target/release/math-run"
    fi
fi
if [ -z "$run" ]; then
    echo "no math-run found: give its path as the first argument to run the smoke test"
    exit 0
fi
"$run" version -s julia
out=$(mktemp -d)/julia-smoke.jsonl
"$run" run -s julia --suite "$here/tests/suite/suite.jsonl" \
    --only indefinite_integral-002 --timeout 60 -o "$out"
tail -n 1 "$out"
