#!/usr/bin/env bash
#
# Install repo-local git hooks. Run once per clone.
#
# Uses `core.hooksPath` rather than copying into .git/hooks, so the hooks
# stay in version control and updating them just means pulling.
#
# Usage:
#     scripts/install-hooks.sh
#
# What you get:
#     pre-commit -- cargo fmt --check + cargo clippy -D warnings on staged
#                   Rust changes (skipped automatically for docs-only diffs).
#
# Uninstall:
#     git config --unset core.hooksPath
#
# See scripts/git-hooks/pre-commit for the bypass env var.

set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel)"
HOOKS_DIR="$REPO_ROOT/scripts/git-hooks"

if [[ ! -d "$HOOKS_DIR" ]]; then
    printf '[install-hooks] expected hooks dir at %s\n' "$HOOKS_DIR" >&2
    exit 1
fi

chmod +x "$HOOKS_DIR"/* 2>/dev/null || true

git -C "$REPO_ROOT" config core.hooksPath "scripts/git-hooks"

printf '[install-hooks] core.hooksPath -> scripts/git-hooks\n'
printf '[install-hooks] active hooks:\n'
ls -1 "$HOOKS_DIR" | sed 's/^/  /'
