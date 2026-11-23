#!/usr/bin/env bash
# worktree-with-data.sh <path> <branch> [<base>]
#
# Create a git worktree at <path> on a new branch <branch> (off <base>,
# default HEAD) and link the main checkout's gitignored data into it:
# `extracted`, `saves` (the whole directory, not only saves/library),
# `captures` and `ghidra/scripts/funcs`. Without these, disc-gated tests
# skip-pass on a broken tree and the preflight / non-vacuity asserts hard-fail
# on a green one. The links are read-only by convention: never commit them.
#
# Re-running on an existing worktree only (re)creates missing links.
set -euo pipefail

if [[ $# -lt 2 ]]; then
    sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'
    exit 2
fi
path=$1
branch=$2
base=${3:-HEAD}
main=$(git rev-parse --show-toplevel)

if [[ ! -e "$path/.git" ]]; then
    git -C "$main" worktree add -b "$branch" "$path" "$base"
fi

for rel in extracted saves captures ghidra/scripts/funcs; do
    src="$main/$rel"
    dst="$path/$rel"
    if [[ ! -e "$src" ]]; then
        echo "skip $rel: not present in $main" >&2
        continue
    fi
    if [[ -L "$dst" ]]; then
        continue
    fi
    if [[ -e "$dst" ]]; then
        echo "skip $rel: $dst exists and is not a link" >&2
        continue
    fi
    mkdir -p "$(dirname "$dst")"
    ln -s "$src" "$dst"
    echo "linked $rel"
done
