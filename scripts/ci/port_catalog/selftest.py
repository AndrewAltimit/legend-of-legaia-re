"""`--selftest`: anchor resolution and disclosure precedence on a synthetic
corpus.
"""

import port_tag_reader

from port_catalog.anchors import collect_port_anchors
from port_catalog.disclosure import REPLACED_BY_RE, WIRED_ITEM_RE
from port_catalog.live import compute_live
from port_catalog.paths import CRATES_DIR
from port_catalog.rust_graph import build_rust_graph, reachable_fns
from port_catalog.rust_source import RustSource, collect_roots

# ---------------------------------------------------------------------------
# Self-test (--selftest)
#
# Unit-level control for the two per-anchor resolutions that have produced
# false audit rows: tag-to-item anchoring (a `///` doc block documents the
# first item after it, however long the block - module scope is only for `//!`
# blocks) and disclosure precedence (own `NOT WIRED` > own `WIRED:` > module
# blanket). Runs on a synthetic in-memory corpus, no repo state touched.
# ---------------------------------------------------------------------------

_SELFTEST_FILES = {
    "main.rs": """\
fn main() {
    let v = wired_fn();
    let _ = v + USED_CONST + wired_item() + TWIN_CONST;
    let _ = crate::blanket::QUAL_CONST;
}
""",
    # A `///` block long enough that any lookahead bound would demote it, with
    # the tag mid-block; then the two const cases; then a wired fn.
    "tags.rs": "/// Long disclosure block.\n"
    + "/// PORT: FUN_8001aa10\n"
    + "/// NOT WIRED: a very thorough disclosure follows.\n"
    + "/// NOT WIRED: filler.\n" * 60
    + """\
pub fn long_doc_fn() -> u32 {
    7
}

/// PORT: FUN_8001aa20
pub const USED_CONST: u32 = 1;

// PORT: FUN_8001aa30
pub const UNUSED_CONST: u32 = 2;

/// PORT: FUN_8001aa40
pub fn wired_fn() -> u32 {
    USED_CONST
}

pub const TWIN_CONST: u32 = 8;
""",
    # The colliding twin: same const name in another file, itself unused.
    "twin.rs": """\
/// PORT: FUN_8001cc10
pub const TWIN_CONST: u32 = 9;
""",
    "blanket.rs": """\
//! Mostly-inert module.
//!
//! PORT: FUN_8001bb10
//!
//! NOT WIRED: every port site in this file is inert unless it says otherwise.

/// PORT: FUN_8001bb20
/// WIRED: called from `main` - opted out of the module blanket.
pub fn wired_item() -> u32 {
    3
}

/// PORT: FUN_8001bb30
pub fn plain_item() -> u32 {
    4
}

/// PORT: FUN_8001bb40
/// WIRED: claims wired, but the line below still discloses.
/// NOT WIRED: the same-granularity disclosure wins.
pub fn both_markers() -> u32 {
    5
}

/// PORT: FUN_8001bb50
/// WIRED: referenced module-qualified from `main`.
pub const QUAL_CONST: u32 = 6;

/// A wiring note that RECOUNTS the disclosure it replaced. Every `NOT WIRED`
/// here is quoted, so the block describes the marker rather than carrying one -
/// the audit's "tagged NOT WIRED but analysed live" row was exactly this.
/// PORT: FUN_8001bb70
pub fn quoted_marker_only() -> u32 {
    7
}

/// # NOT WIRED
///
/// A genuine markdown-heading disclosure in a block that also names
/// `NOT WIRED:` in prose. The heading is unquoted, so this is a real marker.
/// PORT: FUN_8001bb80
pub fn heading_disclosure() -> u32 {
    8
}

/// PORT: FUN_8001bb60
/// REPLACED-BY:
pub fn empty_mechanism() -> u32 {
    7
}
""",
    # The `REPLACED-BY:` class: its precedence over both markers, its
    # non-empty-mechanism requirement, and the module blanket form.
    "replaced.rs": """\
//! Infra module the port replaces wholesale.
//!
//! PORT: FUN_8001dd10
//!
//! REPLACED-BY: the module blanket's mechanism.

/// PORT: FUN_8001dd20
/// NOT WIRED: a host is owed - and the line below says otherwise.
/// REPLACED-BY: a named Rust mechanism.
pub fn replaced_beats_not_wired() -> u32 {
    10
}

/// PORT: FUN_8001dd30
/// NOT WIRED: this one really is a wiring gap.
pub fn still_inert() -> u32 {
    11
}

""",
}


def run_selftest() -> int:
    src_dir = CRATES_DIR / "__selftest__" / "src"
    srcs = {
        src_dir / name: RustSource(src_dir / name, "__selftest__", text)
        for name, text in _SELFTEST_FILES.items()
    }
    # Graph first: `build_rust_graph` assigns the fn uids the anchors capture,
    # same order `main()` uses. Strict sibling too, so the item-scope split
    # (permissive name match vs attributable reference) is exercised.
    fns, edges = build_rust_graph(srcs)
    roots = collect_roots(srcs)
    reach = reachable_fns(fns, edges, roots)
    _, edges_s = build_rust_graph(srcs, strict=True)
    reach_strict = reachable_fns(fns, edges_s, roots)
    anchors = collect_port_anchors(srcs)
    compute_live(anchors, srcs, fns, reach, reach_strict)

    def one(addr: str) -> dict:
        entries = anchors.get(addr, [])
        assert len(entries) == 1, f"{addr}: expected 1 anchor, got {len(entries)}"
        return entries[0]

    failures = 0

    def check(name: str, cond: bool, detail: str = "") -> None:
        nonlocal failures
        if cond:
            print(f"  ok    {name}")
        else:
            print(f"  FAIL  {name}{': ' + detail if detail else ''}")
            failures += 1

    a = one("8001aa10")
    check(
        "long ///-doc tag anchors to the fn it documents",
        a["kind"] == "fn" and a["symbol"] == "long_doc_fn",
        f"got {a['kind']}/{a['symbol']}",
    )
    check("long ///-doc NOT WIRED discloses", a["not_wired_tag"])

    a = one("8001bb10")
    check(
        "//! module-doc tag anchors to the module",
        a["kind"] == "module",
        f"got {a['kind']}/{a['symbol']}",
    )
    check("//! NOT WIRED module blanket discloses its own tag", a["not_wired_tag"])

    a = one("8001aa20")
    check(
        "tag above a used const anchors to the item, live",
        a["kind"] == "item" and a["symbol"] == "USED_CONST" and a["live"],
        f"got {a['kind']}/{a['symbol']} live={a.get('live')}",
    )
    a = one("8001aa30")
    check(
        "tag above an unreferenced const anchors to the item, not live",
        a["kind"] == "item" and a["symbol"] == "UNUSED_CONST" and not a["live"],
        f"got {a['kind']}/{a['symbol']} live={a.get('live')}",
    )
    a = one("8001aa40")
    check(
        "tag above a pub fn anchors to that fn, live via main",
        a["kind"] == "fn" and a["symbol"] == "wired_fn" and a["live"],
        f"got {a['kind']}/{a['symbol']} live={a.get('live')}",
    )

    a = one("8001bb20")
    check(
        "own-block WIRED: overrides the module blanket",
        not a["not_wired_tag"] and a["live"],
        f"not_wired_tag={a['not_wired_tag']} live={a.get('live')}",
    )
    a = one("8001bb30")
    check("sibling without WIRED: stays blanket-disclosed", a["not_wired_tag"])
    a = one("8001bb40")
    check("own-block NOT WIRED beats own-block WIRED:", a["not_wired_tag"])

    a = one("8001cc10")
    check(
        "colliding const name: permissive live, strict not live",
        a["kind"] == "item" and a["live"] and not a["live_strict"],
        f"got {a['kind']} live={a.get('live')} strict={a.get('live_strict')}",
    )
    a = one("8001bb50")
    check(
        "module-qualified const use is strict-live and un-blanketed",
        a["kind"] == "item"
        and a["live"]
        and a["live_strict"]
        and not a["not_wired_tag"],
        f"got {a['kind']} live={a.get('live')} strict={a.get('live_strict')} "
        f"not_wired={a['not_wired_tag']}",
    )

    check(
        "WIRED_ITEM_RE does not match a NOT WIRED line",
        not WIRED_ITEM_RE.search("/// NOT WIRED: still a disclosure"),
    )

    # The quoted-marker DIAGNOSTIC. It must not change the classification (both
    # anchors below still read disclosed), only say which of the two is a
    # quotation - and it must not accuse a heading disclosure that happens to
    # mention the marker in prose further down the same block.
    a = one("8001bb70")
    check(
        "a block that only QUOTES the marker is flagged as a quotation",
        a["not_wired_tag"] and a["quoted_marker_only"],
        f"not_wired={a['not_wired_tag']} quoted={a.get('quoted_marker_only')}",
    )
    a = one("8001bb80")
    check(
        "a heading disclosure beside a quoted mention is not flagged",
        a["not_wired_tag"] and not a["quoted_marker_only"],
        f"not_wired={a['not_wired_tag']} quoted={a.get('quoted_marker_only')}",
    )

    a = one("8001dd20")
    check(
        "own-block REPLACED-BY beats own-block NOT WIRED",
        a["replaced_tag"]
        and not a["not_wired_tag"]
        and a["replaced_by"] == "a named Rust mechanism.",
        f"replaced={a['replaced_tag']} not_wired={a['not_wired_tag']} "
        f"by={a['replaced_by']!r}",
    )
    a = one("8001dd30")
    check(
        "own-block NOT WIRED under a REPLACED-BY blanket stays a wiring gap",
        a["not_wired_tag"] and not a["replaced_tag"],
        f"replaced={a['replaced_tag']} not_wired={a['not_wired_tag']}",
    )
    a = one("8001bb60")
    check(
        "REPLACED-BY with no mechanism is not a class change",
        not a["replaced_tag"] and a["replaced_by"] == "" and a["not_wired_tag"],
        f"replaced={a['replaced_tag']} by={a['replaced_by']!r} "
        f"not_wired={a['not_wired_tag']}",
    )
    a = one("8001dd10")
    check(
        "//! REPLACED-BY module blanket carries its own tag",
        a["kind"] == "module" and a["replaced_tag"],
        f"got {a['kind']} replaced={a['replaced_tag']}",
    )
    check(
        "REPLACED_BY_RE needs the marker to open the line",
        not REPLACED_BY_RE.search("/// the node pool is REPLACED-BY: a Vec"),
    )

    # The audit partition: every (tag, permissive, strict) combination lands
    # in exactly one class. The permissive-live / strict-dead tagged case is
    # the one that used to land in none, which emptied the declared wiring
    # worklist while the tree still carried disclosures.
    from port_catalog.render.live_audit import (
        DISCLOSED,
        LIVE,
        REPLACED,
        STALE,
        TAGGED_PERMISSIVE_ONLY,
        UNDISCLOSED,
        anchor_class,
    )

    def cls(nw: bool, rb: bool, live: bool, strict: bool) -> str:
        return anchor_class(
            {
                "not_wired_tag": nw,
                "replaced_tag": rb,
                "live": live,
                "live_strict": strict,
            }
        )

    check("untagged live anchor is live", cls(False, False, True, True) == LIVE)
    check(
        "untagged permissive-only anchor is live (the floor)",
        cls(False, False, True, False) == LIVE,
    )
    check("untagged dead anchor is a gap", cls(False, False, False, False) == UNDISCLOSED)
    check("tagged strict-live anchor is stale", cls(True, False, True, True) == STALE)
    check(
        "tagged permissive-only anchor has its own class",
        cls(True, False, True, False) == TAGGED_PERMISSIVE_ONLY
        and cls(False, True, True, False) == TAGGED_PERMISSIVE_ONLY,
    )
    check("NOT WIRED dead anchor is disclosed", cls(True, False, False, False) == DISCLOSED)
    check("REPLACED-BY dead anchor is replaced", cls(False, True, False, False) == REPLACED)

    # The shared marker reader ships its own cases; run them here so the one
    # `--selftest` invocation covers the reader every tag-reading gate uses.
    # A tag whose address list wraps is the case that used to lose addresses.
    reader_failures = port_tag_reader.selftest()
    check(
        "port_tag_reader selftest",
        reader_failures == 0,
        f"{reader_failures} reader case(s) failed",
    )
    check(
        "a wrapped PORT list reaches collect_ports",
        port_tag_reader.addresses(
            next(
                tail
                for _n, kind, tail in port_tag_reader.iter_markers(
                    "/// PORT: FUN_801d6704,\n///       FUN_801cf00c\npub fn f() {}\n"
                )
                if kind == "PORT"
            )
        )
        == {"801d6704", "801cf00c"},
    )

    if failures:
        print(
            f"\nself-test: {failures} case(s) failed - the resolver is not "
            "trustworthy, so its audit rows mean nothing"
        )
        return 2
    print("\nself-test: all cases pass")
    return 0
