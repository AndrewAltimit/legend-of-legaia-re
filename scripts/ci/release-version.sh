#!/usr/bin/env bash
# Print the release version a build bakes in as LEGAIA_RELEASE_VERSION
# (read by crates/release-version), from the one source every build shares:
# the release's `v*` tag.
#
#   release-version.sh <tag>          the version of tag <tag>: the leading
#                                     `v` stripped, a `/` made `-` (a
#                                     dry-run ref names one path segment)
#   release-version.sh --head         the version of the `v*` tag on HEAD,
#                                     or nothing when HEAD carries none
#   release-version.sh --head --remote
#                                     the same, also asking `origin` (a
#                                     shallow CI checkout fetches no tags)
#
# release.yml derives the desktop archives' version with the first form;
# build-wasm.sh derives the site bundles' with the second, so a site built
# from a release's commit and that release's archives name one build and
# share netplay rooms. A commit with no tag prints nothing: the build is a
# development build (crates/engine-netplay's `engine_build_for(None)`).
#
# Kept to bash 3.2 (release.yml runs it on macOS too).

set -euo pipefail

strip() {
    local v="${1#v}"
    printf '%s\n' "${v//\//-}"
}

if [[ $# -ge 1 && "$1" != "--head" ]]; then
    strip "$1"
    exit 0
fi
[[ "${1:-}" == "--head" ]] || {
    echo "usage: release-version.sh <tag> | --head [--remote]" >&2
    exit 2
}

head="$(git rev-parse HEAD)"
tags="$(git tag --points-at HEAD --list 'v*' 2>/dev/null || true)"
if [[ -z "$tags" && "${2:-}" == "--remote" ]]; then
    # A lightweight tag lists the commit; an annotated one lists the tag
    # object, then `<tag>^{}` with the commit it peels to.
    tags="$(git ls-remote --tags origin 'refs/tags/v*' 2>/dev/null |
        awk -v h="$head" '$1 == h { t = $2; sub("^refs/tags/", "", t); sub("\\^\\{\\}$", "", t); print t }' || true)"
fi
[[ -n "$tags" ]] || exit 0
# Two release tags on one commit: the newest version names it.
strip "$(printf '%s\n' "$tags" | sort -uV | tail -1)"
