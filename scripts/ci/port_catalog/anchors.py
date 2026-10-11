"""Resolve each `// PORT:` tag to the Rust item it sits on, and build the
patterns that find references to that item.
"""

import re
from collections import defaultdict
from pathlib import Path

import port_tag_reader

from port_catalog.citations import PORT_ADDR_RE, PORT_TAG_RE
from port_catalog.disclosure import (
    MODULE_NOT_WIRED_RE,
    MODULE_REPLACED_BY_RE,
    NOT_WIRED_RE,
    WIRED_ITEM_RE,
    _not_wired_all_quoted,
    _replaced_by_text,
)
from port_catalog.paths import REPO
from port_catalog.rust_source import RustSource, _innermost_impl


def _tag_comment_block(src: RustSource, line: int) -> str:
    """The contiguous comment / attribute run containing `line` (1-based)."""
    lines = src.raw.splitlines()
    idx = min(max(line - 1, 0), len(lines) - 1)
    lo = idx
    while lo > 0 and lines[lo - 1].lstrip().startswith(("//", "#[")):
        lo -= 1
    hi = idx
    while hi + 1 < len(lines) and lines[hi + 1].lstrip().startswith(("//", "#[")):
        hi += 1
    return "\n".join(lines[lo : hi + 1])


def _module_doc_block(src: RustSource) -> str:
    keep = []
    for line in src.raw.splitlines():
        s = line.lstrip()
        if s.startswith("//!") or not s:
            keep.append(line)
        elif keep:
            break
    return "\n".join(keep)


def collect_port_anchors(
    srcs: dict[Path, RustSource],
) -> dict[str, list[dict]]:
    """Map each PORT-tagged address to the Rust symbols that carry the tag.

    Anchor resolution, in order:

    1. `///` / `//` tag above a `fn` - the anchor is that **function**. The
       precise case, and the one worth trusting.
    2. `///` / `//` tag above a `struct` / `enum` / `impl` - the anchor is the
       **type**: live if any non-test method in that type's `impl` blocks is
       reachable.
    3. `///` / `//` tag above a `const` / `static` / `type` alias /
       `macro_rules!` - the anchor is that **item**: live if any reachable
       non-test `fn` references it by name. Before this arm existed those tags
       fell through to module scope, which both accused careful `NOT WIRED:`
       disclosures of running (the module's siblings ran) and hid item-level
       wiring behind a file-level verdict.
    4. A tag written inside a function body - the anchor is the enclosing
       function.
    5. `//! PORT:` - a module-level claim, so the anchor is the **file**: live
       if any non-test `fn` in the file is reachable. Module scope belongs to
       `//!` blocks (and to the loose-tag remainder of case 4); it is not a
       fall-through for `///` doc blocks, which always document the first item
       after them regardless of block length.

    Disclosure (`not_wired_tag`), per anchor: the tag's own comment block
    saying `NOT WIRED` discloses it; failing that, a `//! NOT WIRED` opening
    the module doc discloses every anchor in the file - **except** an anchor
    whose own doc block opens a line with `WIRED:` (`WIRED_ITEM_RE`), which
    opts that one item out of the module blanket. An own-block `NOT WIRED`
    always wins over an own-block `WIRED:` - same-granularity disclosure beats
    same-granularity claim.

    Class (`replaced_tag` / `replaced_by`), per anchor: a `REPLACED-BY: <what>`
    opening a comment line in the tag's own block says the routine's job is
    done in the port by the named mechanism, so no host is owed. It wins over
    both markers above at the same granularity; a `//! REPLACED-BY:` module
    blanket applies to anchors whose own block says nothing about its class.
    A replaced anchor is neither live nor a wiring gap, and its mechanism text
    is carried through to `--live-audit` so the exemption is readable.
    """
    anchors: dict[str, list[dict]] = defaultdict(list)
    for path, src in srcs.items():
        # A PORT tag written inside a unit test is commentary on what the test
        # covers, not a claim that a port site lives there. Counting them would
        # add a permanently-inert anchor per test.
        if src.is_test_file:
            continue
        rel = str(path.relative_to(REPO))
        raw_lines = src.raw.splitlines()
        for lineno, line in enumerate(raw_lines, start=1):
            tag = PORT_TAG_RE.search(line)
            if not tag:
                continue
            # The tag's addresses come from the shared reader, wrapped list and
            # all: scraping the opening line alone left every address past a
            # wrap with no anchor, so `collect_ports` counted it ported while
            # this pass called it inert.
            got = port_tag_reader.marker_tail(raw_lines, lineno - 1)
            tail = got[1] if got and got[0] == "PORT" else tag.group(1)
            addrs = {m.group(1).lower() for m in PORT_ADDR_RE.finditer(tail)}
            if not addrs:
                continue
            is_module_tag = line.lstrip().startswith("//!")
            kind, symbol, fn_uid, ty = "module", "(module)", None, None
            if not is_module_tag:
                item = src.next_item_after(lineno)
                if item and item[0] == "fn":
                    fn = src.fn_at_line(item[2])
                    if fn is not None and fn.is_test:
                        continue
                    if fn is not None:
                        kind, symbol, fn_uid = "fn", fn.name, fn.uid
                elif item and item[0] in ("struct", "enum", "union", "impl", "trait"):
                    kind, symbol, ty = "type", item[1], item[1]
                elif item and item[0] in ("const", "static", "type", "macro_rules!"):
                    kind, symbol = "item", item[1]
                    # An associated const's cross-file spelling is
                    # `Type::NAME`; a free item's is `module::NAME`. Both
                    # qualifiers feed the strict item-scope matcher.
                    ty = _innermost_impl(
                        src.impl_spans, src.pos_of_line(item[2])
                    )
                else:
                    enc = src.enclosing_fn(src.pos_of_line(lineno))
                    if enc is not None and enc.is_test:
                        continue
                    if enc is not None:
                        kind, symbol, fn_uid = "fn", enc.name, enc.uid
            block = (
                _module_doc_block(src)
                if is_module_tag
                else _tag_comment_block(src, lineno)
            )
            # A `//! NOT WIRED` opening the module doc covers every port site in
            # the file - `mdec::st_ring` declares the whole module inert once
            # and then tags seven addresses inside function bodies. A per-item
            # `WIRED:` in the tag's own block opts that item out of the blanket;
            # a per-item `NOT WIRED` still discloses regardless of either.
            own_not_wired = bool(NOT_WIRED_RE.search(block))
            # Every occurrence quoted = the block talks ABOUT the marker rather
            # than carrying one. Reported, not acted on - see the pattern.
            quoted_only = own_not_wired and _not_wired_all_quoted(block)
            own_wired = (not is_module_tag) and bool(WIRED_ITEM_RE.search(block))
            module_doc = _module_doc_block(src)
            module_blanket = bool(MODULE_NOT_WIRED_RE.search(module_doc))
            # `REPLACED-BY:` is the third class, and it is the most specific
            # thing an anchor can say about itself: it does not claim a host
            # exists, it claims none is owed. So an own-block one wins over
            # every other marker in the block, and a module blanket applies to
            # every anchor whose own block stays silent about its class. An
            # empty mechanism is not a claim (see `REPLACED_BY_RE`).
            own_replaced = _replaced_by_text(block)
            module_replaced = _replaced_by_text(module_doc, MODULE_REPLACED_BY_RE)
            replaced_by = own_replaced
            if not replaced_by and not own_not_wired and not own_wired:
                replaced_by = module_replaced
            replaced = bool(replaced_by)
            disclosed = (own_not_wired or (module_blanket and not own_wired)) and (
                not replaced
            )
            for addr in addrs:
                anchors[addr].append(
                    {
                        "kind": kind,
                        "crate": src.crate,
                        "file": rel,
                        "line": lineno,
                        "symbol": symbol,
                        "fn_uid": fn_uid,
                        "type_name": ty,
                        "path": path,
                        "not_wired_tag": disclosed,
                        "replaced_tag": replaced,
                        "replaced_by": replaced_by,
                        "quoted_marker_only": quoted_only,
                    }
                )
    return anchors


def item_reference_patterns(
    path: Path, name: str, type_name: str | None
) -> tuple[re.Pattern, re.Pattern | None]:
    """`(bare, qualified)` patterns for references to a data item.

    `bare` matches the item's unqualified name anywhere; `qualified` matches it
    spelled through its defining module's stem or its `impl` type
    (`battle_helpers::GAUGE_STEP`, `CameraState::FIELD_RESET`), or is `None`
    when the item has no usable qualifier (a `lib.rs` / `mod.rs` free item with
    no `impl` scope). One definition of the rule, two consumers: `compute_live`
    below and `scripts/ci/replay-port-coverage.py`, whose executed-verdict for
    item anchors must agree with this liveness verdict or the two reports argue
    about the same const.
    """
    bare = re.compile(rf"\b{re.escape(name)}\b")
    quals = {q for q in (path.stem, type_name) if q and q not in ("lib", "mod")}
    qual = (
        re.compile(
            r"\b(?:"
            + "|".join(sorted(re.escape(q) for q in quals))
            + rf")\s*::\s*{re.escape(name)}\b"
        )
        if quals
        else None
    )
    return bare, qual


def item_reference_hit(
    fn_path: Path,
    body: str,
    item_path: Path,
    bare: re.Pattern,
    qual: re.Pattern | None,
) -> bool:
    """The strict *attributable*-reference test for one `fn` body.

    True when the function sits in the item's own file and names it bare, or
    spells it qualified from anywhere. An unqualified use behind a `use`
    import is deliberately under-counted - toward "not live" in the liveness
    pass and toward "not observable" in the replay join, never toward a false
    accusation.
    """
    if fn_path == item_path and bare.search(body):
        return True
    return qual is not None and bool(qual.search(body))
