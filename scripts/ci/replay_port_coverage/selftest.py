"""`--selftest`: the item-verdict resolver on a synthetic corpus.
"""

from __future__ import annotations

import json
from pathlib import Path

from replay_port_coverage.catalog import load_catalog_module
from replay_port_coverage.coverage import join_anchor_coverage, load_coverage

# ---------------------------------------------------------------------------
# Self-test (--selftest)
#
# Unit-level control for the item-aware executed-verdict, following the
# catalog's own `--selftest` convention: a synthetic in-memory corpus plus a
# synthetic llvm-cov export driven through `load_coverage` and
# `join_anchor_coverage`, no repo state touched. The cases pin exactly the
# three shapes the naive next-function verdict got wrong: an executed
# attributable reference reads as executed, an unexecuted (or missing)
# reference reads as not-observable-const rather than entered OR never-entered,
# and a `NOT WIRED` const is never accused off a neighbouring function's
# coverage.
# ---------------------------------------------------------------------------

_SELFTEST_FILES = {
    "ribbon.rs": """\
//! PORT: FUN_8001d1b0

/// PORT: FUN_8001d1b0 (the emitter body)
pub fn cold_emitter() -> u32 {
    1
}

pub fn hot_sibling() -> u32 {
    2
}
""",
    "consts.rs": """\
/// PORT: FUN_8001d110
pub const USED_BY_EXEC: u32 = 1;

/// PORT: FUN_8001d120
/// NOT WIRED: nothing references this yet.
pub const DISCLOSED_CONST: u32 = 2;

/// PORT: FUN_8001d130
pub const REF_NOT_RUN: u32 = 3;

/// PORT: FUN_8001d160
pub const REF_QUAL: u32 = 4;

/// PORT: FUN_8001d170
pub const BARE_XFILE: u32 = 5;

/// PORT: FUN_8001d180
/// NOT WIRED: disclosed, yet executed code reads it.
pub const DISCLOSED_USED: u32 = 6;

/// PORT: FUN_8001d140
pub fn exec_fn() -> u32 {
    USED_BY_EXEC + DISCLOSED_USED
}

/// PORT: FUN_8001d150
pub fn cold_fn() -> u32 {
    REF_NOT_RUN
}
""",
    "user.rs": """\
pub fn qual_user() -> u32 {
    crate::consts::REF_QUAL + BARE_XFILE
}
""",
    "types.rs": """\
/// PORT: FUN_8001d190
pub struct ColdGauge {
    pub v: u32,
}

impl ColdGauge {
    pub fn tick(&mut self) {
        self.v += 1;
    }
}

/// PORT: FUN_8001d1a0
pub struct HotMeter {
    pub v: u32,
}

impl HotMeter {
    pub fn bump(&mut self) {
        self.v += 1;
    }
}
""",
    # A generic function is emitted only where it is instantiated. The
    # synthetic export drops its record, as a binary that never names it does;
    # the executed function below it must not lend it a verdict.
    "generic.rs": """\
/// PORT: FUN_8001d1e0
/// REPLACED-BY: the typed rows.
pub fn uninstantiated<F: Fn() -> u32>(f: F) -> u32 {
    f()
}

pub fn generic_neighbour() -> u32 {
    9
}
""",
    # A facade: the module tag sits on a file that only declares submodules,
    # so no coverage record ever names it. The verdict is read off the subtree.
    "hot_facade.rs": """\
//! PORT: FUN_8001d1c0

mod body;
""",
    "hot_facade/body.rs": """\
pub fn facade_hot() -> u32 {
    7
}
""",
    "cold_facade.rs": """\
//! PORT: FUN_8001d1d0

mod body;
""",
    "cold_facade/body.rs": """\
pub fn facade_cold() -> u32 {
    8
}
""",
}

# Which synthetic functions the synthetic ladder "executed".
_SELFTEST_EXECUTED = {"exec_fn", "qual_user", "bump", "hot_sibling", "facade_hot", "generic_neighbour"}

# Functions the synthetic binary carries no record for at all.
_SELFTEST_NO_RECORD = {"uninstantiated"}


def run_selftest() -> int:
    import tempfile

    print("replay-port-coverage item-verdict self-test")
    catalog = load_catalog_module()
    src_dir = catalog.CRATES_DIR / "__selftest__" / "src"
    srcs = {
        src_dir / name: catalog.RustSource(src_dir / name, "__selftest__", text)
        for name, text in _SELFTEST_FILES.items()
    }
    anchors = catalog.collect_port_anchors(srcs)

    # Synthetic llvm-cov export: one function record per parsed fn, count 1
    # for the executed set, 0 otherwise. Line spans come from the parsed
    # source so the fixture cannot drift from its own line numbers.
    functions = []
    for path, src in srcs.items():
        for f in src.fns:
            if f.name in _SELFTEST_NO_RECORD:
                continue
            hi = src.line_of(max(f.body_end - 1, f.body_start))
            functions.append(
                {
                    "filenames": [str(path)],
                    "regions": [
                        [f.line, 1, hi, 1, 1 if f.name in _SELFTEST_EXECUTED else 0, 0, 0, 0]
                    ],
                }
            )
    with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False) as tmp:
        json.dump({"data": [{"functions": functions}]}, tmp)
        tmp_path = Path(tmp.name)
    try:
        cov = load_coverage(tmp_path)
    finally:
        tmp_path.unlink()
        tmp_path.with_name(tmp_path.name + ".reduced").unlink(missing_ok=True)

    live = {
        "8001d110",
        "8001d130",
        "8001d140",
        "8001d150",
        "8001d160",
        "8001d170",
        "8001d190",
        "8001d1a0",
        "8001d1b0",
        "8001d1c0",
        "8001d1d0",
    }
    not_live = {"8001d120", "8001d180", "8001d1e0"}
    joined = join_anchor_coverage(
        catalog, anchors, srcs, [("selftest", cov)], live, not_live
    )

    def addrs(bucket: str) -> set[str]:
        val = joined[bucket]
        return set(val) if isinstance(val, set) else {a for a, _ in val}

    failures = 0

    def check(name: str, cond: bool, detail: str = "") -> None:
        nonlocal failures
        if cond:
            print(f"  ok    {name}")
        else:
            print(f"  FAIL  {name}{': ' + detail if detail else ''}")
            failures += 1

    check(
        "executed fn anchor is entered",
        "8001d140" in addrs("live_entered"),
        f"live_entered={addrs('live_entered')}",
    )
    check(
        "const referenced by an executed same-file fn is entered",
        "8001d110" in addrs("live_entered"),
    )
    check(
        "const referenced module-qualified by an executed cross-file fn is entered",
        "8001d160" in addrs("live_entered"),
    )
    check(
        "const whose only reference never ran is not-observable-const",
        "8001d130" in addrs("not_observable_const")
        and "8001d130" not in addrs("live_unentered")
        and "8001d130" not in addrs("live_entered"),
        f"const={addrs('not_observable_const')} unentered={addrs('live_unentered')}",
    )
    check(
        "cross-file bare-name reference does not count (strict rule mirrored)",
        "8001d170" in addrs("not_observable_const"),
    )
    check(
        "NOT WIRED const with no executed reference is never accused",
        "8001d120" in addrs("not_observable_const")
        and "8001d120" not in addrs("disclosed_entered")
        and "8001d120" not in addrs("inert_entered"),
        f"disclosed={addrs('disclosed_entered')} inert={addrs('inert_entered')}",
    )
    check(
        "NOT WIRED const an executed fn really references IS accused",
        "8001d180" in addrs("disclosed_entered")
        and "8001d180" in addrs("inert_entered"),
        f"disclosed={addrs('disclosed_entered')}",
    )
    check(
        "unexecuted fn anchor stays on the never-entered worklist",
        "8001d150" in addrs("live_unentered"),
    )
    check(
        "a module tag does not credit an address its own fn tag says never ran",
        "8001d1b0" in addrs("live_unentered") and "8001d1b0" not in addrs("live_entered"),
        f"entered={addrs('live_entered')}",
    )
    check(
        "type anchor with no executed method is never-entered, not const",
        "8001d190" in addrs("live_unentered")
        and "8001d190" not in addrs("not_observable_const"),
    )
    check(
        "type anchor with an executed method is entered",
        "8001d1a0" in addrs("live_entered"),
    )
    check(
        "a fn with no record does not borrow the next function's verdict",
        "8001d1e0" not in addrs("disclosed_entered")
        and "8001d1e0" not in addrs("inert_entered"),
        f"disclosed={addrs('disclosed_entered')} inert={addrs('inert_entered')}",
    )
    check(
        "a facade's module tag is entered when its subtree ran",
        "8001d1c0" in addrs("live_entered"),
        f"entered={addrs('live_entered')} unobservable={addrs('unobservable')}",
    )
    check(
        "a facade's module tag is never-entered when its subtree did not run",
        "8001d1d0" in addrs("live_unentered"),
        f"unentered={addrs('live_unentered')} unobservable={addrs('unobservable')}",
    )
    check(
        "only the record-less fn is unobservable",
        addrs("unobservable") == {"8001d1e0"},
        f"unobservable={addrs('unobservable')}",
    )

    if failures:
        print(
            f"\nself-test: {failures} case(s) failed - the item-aware verdict "
            "is not trustworthy, so its report rows mean nothing"
        )
        return 2
    print("\nself-test: all cases pass")
    return 0
