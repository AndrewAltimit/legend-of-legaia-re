"""The `live` verdict per port address: is its anchor reachable from a host
root, and what does its disclosure say?
"""

import os
from collections import defaultdict
from pathlib import Path

from port_catalog.anchors import item_reference_hit, item_reference_patterns
from port_catalog.rust_source import RustFn, RustSource


def compute_live(
    anchors: dict[str, list[dict]],
    srcs: dict[Path, RustSource],
    fns: list[RustFn],
    reach: set[int],
    reach_strict: set[int] | None = None,
) -> dict[str, dict]:
    """Resolve each anchor to live / not, then fold up to `{addr: ...}`.

    Address-level `live` is `any(anchor is live)`: one address can be ported
    into several crates, and it only takes one wired implementation for the
    retail behaviour to be on the frame path. The per-anchor verdicts stay on
    the row so `--live-audit` can report at the granularity a `NOT WIRED:` tag
    is actually written at.

    `reach_strict`, when supplied, is the same resolution run against the
    receiver-gated graph (`build_rust_graph(strict=True)`). It sets a second
    verdict, `live_strict`, which nothing but the stale-`NOT WIRED` test reads -
    see that function's docstring for why the two questions need two graphs.
    Absent it, `live_strict` mirrors `live`.
    """
    # A module anchor's scope is the file. But `foo.rs` next to a `foo/`
    # directory is often a pure module-declaration file with no `fn` of its
    # own - `engine-vm/src/field.rs` declares the field VM and holds none of
    # it. Reading such a file's `//! PORT:` block against an empty function
    # set reports the whole field VM inert, which is wrong. When the file
    # defines no non-test `fn`, widen the scope to its submodule subtree.
    def module_scope(reached: set[int]) -> dict[Path, bool]:
        # One pass for "does this file hold a reached non-test fn", then the
        # subtree test on plain string prefixes. Asking `Path.is_relative_to`
        # of every (facade, file) pair was most of a catalog run's wall clock.
        own_fns = {p: [f for f in s.fns if not f.is_test] for p, s in srcs.items()}
        reached_files = [
            str(p) for p, own in own_fns.items() if any(f.uid in reached for f in own)
        ]
        reached_set = set(reached_files)
        out: dict[Path, bool] = {}
        for p, own in own_fns.items():
            if own:
                out[p] = str(p) in reached_set
                continue
            # `lib.rs` / `main.rs` / `mod.rs` own the directory they sit in;
            # `foo.rs` owns the sibling `foo/`.
            sub = p.parent if p.stem in ("lib", "main", "mod") else p.parent / p.stem
            prefix = str(sub) + os.sep
            out[p] = any(q.startswith(prefix) for q in reached_files)
        return out

    def type_scope(reached: set[int]) -> dict[tuple[Path, str], bool]:
        out: dict[tuple[Path, str], bool] = defaultdict(bool)
        for f in fns:
            if f.impl_type and not f.is_test and f.uid in reached:
                out[(f.path, f.impl_type)] = True
        return out

    # An `item` anchor (a tag above a `const` / `static` / `type` alias /
    # `macro_rules!`) has no body to reach, so its verdict is *use*: live iff
    # some reachable non-test `fn` body references the item. Comments and
    # strings are already blanked in `stripped`, so a doc mention does not
    # count. The two graphs get the two biases they need (see
    # `build_rust_graph`): the permissive verdict matches the bare name
    # tree-wide - two same-named consts share it, over-approximating toward
    # "used" so the not-live list stays a hard floor. The strict verdict, read
    # only by the stale-`NOT WIRED` test, demands an *attributable* reference:
    # the referencing `fn` sits in the item's own file, or spells the item
    # qualified by its defining module's stem or its `impl` type
    # (`battle_helpers::GAUGE_STEP`, `CameraState::FIELD_RESET`). An
    # unqualified use behind a `use` import is under-counted there, which errs
    # toward "not live" - a missed stale tag, never a false accusation.
    item_keys: dict[tuple[Path, str], str | None] = {}
    for entries in anchors.values():
        for e in entries:
            if e["kind"] == "item":
                item_keys[(e["path"], e["symbol"])] = e["type_name"]
    item_pats = {
        (p, n): item_reference_patterns(p, n, ty)
        for (p, n), ty in item_keys.items()
    }

    def item_scope(
        reached: set[int], strict: bool
    ) -> dict[tuple[Path, str], bool]:
        out = {k: False for k in item_keys}
        remaining = set(item_keys)
        for f in fns:
            if not remaining:
                break
            if f.is_test or f.uid not in reached:
                continue
            body = srcs[f.path].stripped[f.body_start : f.body_end]
            for key in list(remaining):
                p, n = key
                bare, qual = item_pats[key]
                if strict:
                    hit = item_reference_hit(f.path, body, p, bare, qual)
                else:
                    hit = bare.search(body)
                if hit:
                    out[key] = True
                    remaining.discard(key)
        return out

    module_live = module_scope(reach)
    type_live = type_scope(reach)
    item_live = item_scope(reach, strict=False)
    strict_on = reach_strict is not None
    module_live_s = module_scope(reach_strict) if strict_on else module_live
    type_live_s = type_scope(reach_strict) if strict_on else type_live
    item_live_s = (
        item_scope(reach_strict, strict=True) if strict_on else item_live
    )
    # Which types the file gives an `impl` block to at all. A tag on a plain
    # data struct - no `impl`, its behaviour in free functions or in an `impl`
    # of some *other* type in the same file - has no method for the rule above
    # to find, so it could never be live however wired the port is. Those fall
    # back to the file's module scope.
    typed_in_file: dict[Path, set[str]] = defaultdict(set)
    for p, s in srcs.items():
        for _a, _b, ty, _tr in s.impl_spans:
            typed_in_file[p].add(ty)
    def verdict(e: dict, reached: set[int], mods, types, items) -> bool:
        if e["kind"] == "fn":
            return e["fn_uid"] in reached
        if e["kind"] == "type":
            hit = types.get((e["path"], e["type_name"]), False)
            if not hit and e["type_name"] not in typed_in_file[e["path"]]:
                hit = mods.get(e["path"], False)
            return hit
        if e["kind"] == "item":
            return items.get((e["path"], e["symbol"]), False)
        return mods.get(e["path"], False)

    out: dict[str, dict] = {}
    for addr, entries in anchors.items():
        for e in entries:
            e["live"] = verdict(e, reach, module_live, type_live, item_live)
            e["live_strict"] = (
                verdict(e, reach_strict, module_live_s, type_live_s, item_live_s)
                if strict_on
                else e["live"]
            )
        # An address is `replaced` only when EVERY inert anchor of it is
        # replaced. One anchor claiming an exemption must not carry a sibling
        # anchor that really is a wiring gap out of the denominator with it -
        # the normal shape for an address ported twice (one copy re-hosted,
        # one copy still waiting on a caller).
        inert = [e for e in entries if not e["live"]]
        out[addr] = {
            "live": any(e["live"] for e in entries),
            "live_strict": any(e["live_strict"] for e in entries),
            "anchors": entries,
            "not_wired_tag": any(e["not_wired_tag"] for e in entries),
            "replaced_tag": bool(inert) and all(e["replaced_tag"] for e in inert),
        }
    return out
