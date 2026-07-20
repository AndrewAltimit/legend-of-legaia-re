#!/usr/bin/env python3
"""Markdown intra-repo link + heading-anchor checker for committed docs.

The site has a link gate (`check-site-links.py`); the Markdown corpus had none,
and rot accumulated where nothing was measuring. Nearly every defect found on
the first full run was one systematic bug: a heading's `" - "` slugifies to
three hyphens (`---`), but the link was written with two (`--`), so the anchor
silently resolved to nothing. GitHub renders a dead fragment as a jump to the
top of the page, which is why this survives review -- the link still "works".

Checks, for every file in scope:

  * **Relative file links** -- `[x](../formats/tim.md)` must resolve on disk.
  * **Anchors** -- `[x](foo.md#bar)` must match a real heading in `foo.md`.
  * **Same-page anchors** -- `[x](#bar)` must match a heading in this file.

Scope: every `docs/**/*.md`, every `crates/*/README.md`, and every top-level
`*.md`. Mirrors `check-doc-density.py`'s scope so the two gates agree on what
"a committed doc" means. External URLs (`http`, `https`, `mailto`) are not
checked -- that needs the network and would make the gate flaky.

Anchors are matched with GitHub's slug rules: strip inline markup, lowercase,
drop everything that is not alphanumeric / space / hyphen, then spaces become
hyphens. Duplicate headings get the `-1`, `-2`, ... suffixes GitHub appends.
Explicit `<a name="">` / `id=""` anchors count too.

The checker **exits non-zero when it finds a broken link**. The pre-commit hook
runs it on the staged doc set; bypass with `LEGAIA_SKIP_PRECOMMIT=1`.

A caveat worth knowing before you "fix" a failure: when a link and a heading
disagree, the link is almost always the thing to change. A heading is an anchor
target that other pages -- and the site's hand-mirrored HTML -- may already point
at, so renaming it to satisfy one link breaks every other inbound reference.

Usage:
    scripts/ci/check-md-links.py                 # scan the whole corpus
    scripts/ci/check-md-links.py --staged        # only staged md files (hook)
    scripts/ci/check-md-links.py --quiet         # suppress the success line

Pure standard library; ASCII-only; no external dependencies.
"""

import argparse
import glob
import os
import re
import subprocess
import sys

LINK_RE = re.compile(r"\[(?:[^\]]*)\]\(([^)\s]+?)(?:\s+\"[^\"]*\")?\)")
HEADING_RE = re.compile(r"^#{1,6}\s+(.*?)\s*$", re.M)
EXPLICIT_ANCHOR_RE = re.compile(r"<a\s+(?:name|id)=\"([^\"]+)\"")
FENCE_RE = re.compile(r"^(```|~~~)")


def in_scope(path):
    """True if path is a doc we lint. Mirrors check-doc-density.py's scope."""
    p = path.replace("\\", "/")
    if not p.endswith(".md"):
        return False
    if p == "crates/web-viewer/pkg/README.md":
        return False
    if p.startswith("docs/"):
        return True
    if "/" not in p:
        return True
    parts = p.split("/")
    if len(parts) == 3 and parts[0] == "crates" and parts[2] == "README.md":
        return True
    return False


def corpus_files():
    files = list(glob.glob("docs/**/*.md", recursive=True))
    files += glob.glob("crates/*/README.md")
    files += glob.glob("*.md")
    return sorted(f for f in files if in_scope(f))


def staged_files():
    out = subprocess.run(
        ["git", "diff", "--cached", "--name-only", "--diff-filter=ACMR"],
        capture_output=True,
        text=True,
    ).stdout.split()
    return sorted(f for f in out if in_scope(f) and os.path.exists(f))


def strip_fences(text):
    """Blank out fenced code blocks. A link-looking string in a shell example is
    not a link, and a `#` comment line is not a heading."""
    out = []
    in_fence = False
    for line in text.splitlines():
        if FENCE_RE.match(line.strip()):
            in_fence = not in_fence
            out.append("")
            continue
        out.append("" if in_fence else line)
    return "\n".join(out)


def slug(text):
    """GitHub's heading -> anchor slug."""
    text = re.sub(r"`([^`]*)`", r"\1", text)
    text = re.sub(r"\*\*([^*]*)\*\*", r"\1", text)
    text = re.sub(r"\*([^*]*)\*", r"\1", text)
    text = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", text)
    s = text.strip().lower()
    s = re.sub(r"[^\w\s-]", "", s)
    return s.replace(" ", "-")


def anchors_of(path, cache):
    """Every anchor `path` exposes: slugged headings (with GitHub's duplicate
    suffixes) plus explicit <a name>/id anchors."""
    if path in cache:
        return cache[path]
    found = set()
    if path.endswith(".md") and os.path.exists(path):
        with open(path, encoding="utf-8", errors="replace") as fh:
            text = strip_fences(fh.read())
        counts = {}
        for title in HEADING_RE.findall(text):
            s = slug(title)
            n = counts.get(s, 0)
            counts[s] = n + 1
            found.add(s if n == 0 else "%s-%d" % (s, n))
        found.update(EXPLICIT_ANCHOR_RE.findall(text))
    cache[path] = found
    return found


def ignored_targets(dests):
    """Subset of `dests` that git ignores.

    A link into a gitignored tree resolves on the machine that produced
    those artifacts and nowhere else, so `os.path.exists` calls it healthy
    while CI and every fresh clone see a dead link. Ask git instead of the
    filesystem. One batched call - `check-ignore` per link is slow enough
    to be felt in the pre-commit hook.
    """
    if not dests:
        return set()
    proc = subprocess.run(
        ["git", "check-ignore", "--stdin"],
        input="\n".join(sorted(dests)),
        capture_output=True,
        text=True,
    )
    return {os.path.normpath(line) for line in proc.stdout.splitlines() if line}


def check_file(path, cache, seen_dests):
    violations = []
    with open(path, encoding="utf-8", errors="replace") as fh:
        text = strip_fences(fh.read())
    for raw in LINK_RE.findall(text):
        if raw.startswith(("http://", "https://", "mailto:", "#!")):
            continue
        target, frag = (raw.split("#", 1) + [""])[:2] if "#" in raw else (raw, "")
        if target == "":
            dest = path
        else:
            dest = os.path.normpath(os.path.join(os.path.dirname(path), target))
            if not os.path.exists(dest):
                violations.append("missing file -> %s" % target)
                continue
            seen_dests.setdefault(dest, []).append((path, target))
        if not frag:
            continue
        anchors = anchors_of(dest, cache)
        # A non-markdown target exposes no anchors we can parse; skip rather
        # than guess (never fail on something we cannot actually check).
        if anchors and frag not in anchors:
            violations.append("dead anchor -> %s#%s" % (target or os.path.basename(path), frag))
    return violations


def main():
    ap = argparse.ArgumentParser(description="Markdown intra-repo link checker.")
    ap.add_argument("--staged", action="store_true", help="only check staged markdown files")
    ap.add_argument("--quiet", action="store_true", help="suppress the success summary line")
    args = ap.parse_args()

    root = subprocess.run(
        ["git", "rev-parse", "--show-toplevel"], capture_output=True, text=True
    ).stdout.strip()
    if root:
        os.chdir(root)

    files = staged_files() if args.staged else corpus_files()
    cache = {}
    seen_dests = {}
    total = 0
    for path in files:
        for msg in check_file(path, cache, seen_dests):
            print("%s: %s" % (path, msg))
            total += 1

    for dest in sorted(ignored_targets(seen_dests)):
        for path, target in seen_dests[dest]:
            print(
                "%s: gitignored target -> %s (resolves only where that artifact "
                "was generated; not in a fresh clone or CI)" % (path, target)
            )
            total += 1

    if total:
        print(
            "[check-md-links] %d broken link(s) across %d file(s)" % (total, len(files)),
            file=sys.stderr,
        )
        return 1
    if not args.quiet:
        print("[check-md-links] OK -- %d file(s), no broken links" % len(files))
    return 0


if __name__ == "__main__":
    sys.exit(main())
