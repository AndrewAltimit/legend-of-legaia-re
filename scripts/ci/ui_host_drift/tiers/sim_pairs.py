"""Tier 3 analysis: do paired injection sites name the same kernels?

Rows: `ui_host_drift.rows.sim_pairs`.
"""

import re

from ui_host_drift.roots import REPO
from ui_host_drift.rows.host_files import NATIVE_REDRAW
from ui_host_drift.rows.sim_pairs import NATIVE_REDRAW_STEP_FILES, SIM_PAIRS
from ui_host_drift.rust_text import (
    BLOCK_COMMENT_RE,
    fn_body,
    signature_end,
    strip_comments,
)

# Where this tier's rows are declared, as its own messages cite it.
ROWS_FILE = "scripts/ci/ui_host_drift/rows/sim_pairs.py"

# Splice depth guard: the step tree is shallow, so a deeper chain is a cycle.
NATIVE_STEP_SPLICE_DEPTH = 8


def native_step_bodies() -> dict[str, str]:
    """Comment-stripped body of every fn the native redraw step files define."""
    out: dict[str, str] = {}
    for rel in NATIVE_REDRAW_STEP_FILES:
        path = REPO / rel
        if not path.is_file():
            continue
        text = path.read_text(encoding="utf-8")
        for m in re.finditer(r"\bfn\s+([a-z_][a-z_0-9]*)\s*[<(]", text):
            brace = signature_end(text, m.start())
            if brace < 0:
                continue
            out[m.group(1)] = strip_comments(BLOCK_COMMENT_RE.sub(" ", fn_body(text, brace)))
    return out


def splice_native_steps(body: str, steps: dict[str, str] | None = None, depth: int = 0) -> str:
    """`body` with each `self.<step>(..)` call replaced by the step's body."""
    if steps is None:
        steps = native_step_bodies()
    if depth > NATIVE_STEP_SPLICE_DEPTH or not steps:
        return body
    # `self.` for the window's own step methods; `draw_cx.` for the draw-list
    # builder (`DrawCtx`), whose context the redraw constructs and calls.
    call_re = re.compile(
        r"\b(?:self|draw_cx)\.(" + "|".join(map(re.escape, steps)) + r")\s*\("
    )
    out: list[str] = []
    pos = 0
    for m in call_re.finditer(body):
        if m.start() < pos:
            continue
        # The call's closing paren (arguments may nest).
        i, par = m.end(), 1
        while i < len(body) and par:
            if body[i] == "(":
                par += 1
            elif body[i] == ")":
                par -= 1
            i += 1
        out.append(body[pos : m.start()])
        out.append(splice_native_steps(steps[m.group(1)], steps, depth + 1))
        pos = i
    out.append(body[pos:])
    return "".join(out)


def site_source(rel: str, fn_name: str | None) -> tuple[str | None, str]:
    """The source a [`SIM_PAIRS`] site names: one fn body, or the whole file.

    Returns `(text, problem)`; `text` is None when the site cannot be
    resolved, in which case `problem` says why. An unresolvable site is
    always an error - a pairing that cannot find its own anchor checks
    nothing, and would otherwise pass forever after a rename.
    """
    path = REPO / rel
    if not path.is_file():
        return None, f"host source {rel} is missing"
    text = path.read_text(encoding="utf-8")
    if fn_name is None:
        return strip_comments(BLOCK_COMMENT_RE.sub(" ", text)), ""
    m = re.search(rf"\bfn\s+{re.escape(fn_name)}\s*[<(]", text)
    if not m:
        return None, f"no `fn {fn_name}` in {rel} (renamed or deleted?)"
    brace = signature_end(text, m.start())
    if brace < 0:
        return None, f"`fn {fn_name}` in {rel} has no body"
    body = strip_comments(BLOCK_COMMENT_RE.sub(" ", fn_body(text, brace)))
    if rel == NATIVE_REDRAW or rel in NATIVE_REDRAW_STEP_FILES:
        body = splice_native_steps(body)
    if rel == NATIVE_REDRAW and fn_name == "handle_redraw":
        # The draw builder's leaf helpers (the per-draw cue / clip / key
        # fns, reached as `cues.<fn>(` or as a fn value) were closures and a
        # nested fn of this body before the split; their bodies belong to it.
        sources = "\n".join(
            strip_comments((REPO / r).read_text(encoding="utf-8"))
            for r in (NATIVE_REDRAW, *NATIVE_REDRAW_STEP_FILES)
            if (REPO / r).is_file()
        )
        spliced = set(re.findall(r"\b(?:self|draw_cx)\.([a-z_][a-z_0-9]*)\s*\(", sources))
        for name, step in native_step_bodies().items():
            if name not in spliced:
                body += "\n" + step
    return body, ""


def sim_pair_divergence(pair: dict) -> tuple[list[str], list[str]]:
    """Evaluate one [`SIM_PAIRS`] row.

    Returns `(hard_errors, divergences)`. A hard error (an unresolvable site,
    a malformed row) always fails; a divergence fails unless the row carries
    `blocked_on`.
    """
    sites: dict = pair["sites"]  # type: ignore[assignment]
    errors: list[str] = []
    bodies: dict[str, str] = {}
    for host, (rel, fn_name) in sites.items():
        text, problem = site_source(rel, fn_name)
        if text is None:
            errors.append(f"SIM {pair['what']!r}: {problem}")
        else:
            bodies[host] = text
    # A row may name any number of sites from two up. This used to demand
    # EXACTLY two and return `([], [])` otherwise - so a three-site row was
    # not a failure and not an error, it was silently unevaluated, and the
    # gate reported clean while checking nothing. That is the worst shape a
    # checker can have, and it hid behind the fact that every row happened to
    # be a pair when the guard was written. A row with fewer than two sites is
    # now a hard error, because it cannot compare anything either.
    if len(bodies) < 2 and not errors:
        errors.append(
            f"SIM {pair['what']!r}: needs at least two resolvable sites, got {len(bodies)}"
        )
    if errors:
        return errors, []

    hosts = sorted(bodies)
    a, b = hosts[0], hosts[1]
    mode = pair.get("mode")
    diffs: list[str] = []

    def where(host: str) -> str:
        rel, fn_name = sites[host]
        return f"{rel}::{fn_name}" if fn_name else rel

    if mode in ("symbols_all", "symbols_same"):
        for sym in pair.get("symbols", []):  # type: ignore[union-attr]
            seen = {h: re.search(rf"\b{re.escape(sym)}\b", bodies[h]) is not None for h in hosts}
            if mode == "symbols_all" and not all(seen.values()):
                missing = [h for h in hosts if not seen[h]]
                diffs.append(
                    f"`{sym}` is not called at {', '.join(where(h) for h in missing)}"
                )
            elif mode == "symbols_same" and len(set(seen.values())) > 1:
                has = [h for h in hosts if seen[h]]
                lacks = [h for h in hosts if not seen[h]]
                diffs.append(
                    f"`{sym}` is called at {', '.join(where(h) for h in has)} "
                    f"but not at {', '.join(where(h) for h in lacks)} - "
                    f"all must, or none may"
                )
    elif mode == "pattern_same":
        pat = re.compile(pair["pattern"])  # type: ignore[arg-type]
        found = {h: {m.group(1) for m in pat.finditer(bodies[h])} for h in hosts}
        # Every site must agree with the union, so a third host cannot carry a
        # stray match that a pairwise `a != b` comparison would never look at.
        union: set[str] = set().union(*found.values())
        for h in hosts:
            extra = sorted(found[h] - set().union(*(found[o] for o in hosts if o != h)))
            missing = sorted(union - found[h])
            if extra:
                diffs.append(f"only {where(h)}: {', '.join(extra)}")
            if missing:
                diffs.append(f"missing at {where(h)}: {', '.join(missing)}")
    else:
        errors.append(f"SIM {pair['what']!r}: unknown mode {mode!r}")
    return errors, diffs


def check_sim_pairs() -> tuple[list[str], list[str]]:
    """Compare every [`SIM_PAIRS`] row.

    Returns `(problems, pending)`. `pending` lists the rows whose divergence
    is disclosed by `blocked_on`; a `blocked_on` row with NO divergence is a
    problem, not a pass - the marker has outlived the gap and must go.
    """
    problems: list[str] = []
    pending: list[str] = []
    for pair in SIM_PAIRS:
        errors, diffs = sim_pair_divergence(pair)
        problems.extend(errors)
        if errors:
            continue
        blocked = pair.get("blocked_on")
        if diffs and not blocked:
            problems.append(
                f"SIM DRIFT ({pair['what']}):\n"
                + "".join(f"      {d}\n" for d in diffs)
                + f"      The two hosts hand different models to the same kernel. "
                f"Make the sites agree, or record why not with `blocked_on`."
            )
        elif diffs:
            pending.append(f"{pair['what']}\n      " + "\n      ".join(diffs))
        elif blocked:
            problems.append(
                f"STALE blocked_on ({pair['what']}): the two sites now agree, so "
                f"the marker describes a gap that is closed. Drop `blocked_on` "
                f"from the row in {ROWS_FILE} - a pending marker that "
                f"outlives its gap is a permanent exemption wearing a temporary "
                f"name."
            )
    return problems, pending
