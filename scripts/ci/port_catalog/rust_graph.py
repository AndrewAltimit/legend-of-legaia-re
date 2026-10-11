"""The Rust call graph over every parsed source, permissive or
receiver-gated, and reachability from the host roots.
"""

from collections import defaultdict, deque
from pathlib import Path

from port_catalog.rust_source import (
    BARE_CALL_RE,
    FIELD_COLON_RE,
    IDENT_RE,
    METHOD_CALL_RE,
    QUAL_CALL_RE,
    RUST_KEYWORDS,
    RustFn,
    RustSource,
)


def build_rust_graph(
    srcs: dict[Path, RustSource],
    strict: bool = False,
) -> tuple[list[RustFn], dict[int, set[int]]]:
    """Name-resolved forward call graph over every non-test `fn` body.

    Resolution order per call site, most to least specific:

    - `Qual::name` - matched against `impl Qual` methods, then against a module
      (file stem) named `Qual`, then falls back to the bare name.
    - `.name(...)` - matched against every in-tree method of that name. Receiver
      types are not inferred, so this is the main over-approximation.
    - `name(...)` - a free call; matched against every in-tree `fn name`.
    - bare `name` not followed by `(` - matched against **free** functions only,
      which is how a function value reaches `map` / `sort_by_key` / a struct
      field holding a callback. Methods are deliberately excluded here: a local
      binding named `new` or a field named `id` would otherwise link to every
      `Type::new` / `Type::id` in the workspace.

    ## The two graphs, and why sharpening this one in place would be wrong

    `strict=False` (the default) is the permissive graph described above, and it
    is what every `live` / `--not-live` verdict is read off. Its
    over-approximation is load-bearing: biasing every ambiguity toward
    "reachable" is what makes the not-live list a hard floor, so an address it
    calls inert is one no plausible edge could reach.

    That same bias is wrong for the opposite question. `--live-audit`'s "tagged
    `NOT WIRED` but analysed live" section asks whether a disclosure is stale,
    and there a spurious edge manufactures a false accusation: one
    `session.tick(...)` reaching all 63 in-tree `tick`, one `new(` reaching all
    192 `new`. Measured over the whole tree, 54 of 55 rows in that section were
    false edges rather than stale tags.

    So `strict=True` builds a *second* graph with the two sharpenings that
    ambiguity allows, and only the stale-tag test reads it. Nothing is traded:
    each direction consults the graph whose error mode is safe for it. (An
    earlier attempt applied the receiver gate to the single shared graph and was
    reverted for exactly the reason above - it is the sharing, not the gate,
    that was the mistake.)

    The two sharpenings, both of which only ever *remove* edges:

    1. **Struct fields are not function values.** An identifier immediately
       followed by `:` - and not `::` - is a field declaration or a struct
       literal key, so it cannot be a function value reaching `map`. Without
       this the field `stat_deltas` links to a free `stat_deltas` in another
       crate.
    2. **A receiver gate on ambiguous method edges.** A `.name(...)` or
       `name(...)` edge onto an `impl Type` method survives only if the calling
       file names `Type` at all, or defines the method itself. A file that never
       writes `EscapeTimer` cannot be calling `EscapeTimer::tick`. Free
       functions and `Qual::name` edges are untouched - they are already
       resolved.
    """
    fns: list[RustFn] = []
    for src in srcs.values():
        fns.extend(src.fns)
    for i, f in enumerate(fns):
        f.uid = i

    by_name: dict[str, list[RustFn]] = defaultdict(list)
    by_qual: dict[tuple[str, str], list[RustFn]] = defaultdict(list)
    by_mod: dict[tuple[str, str], list[RustFn]] = defaultdict(list)
    for f in fns:
        by_name[f.name].append(f)
        if f.impl_type:
            by_qual[(f.impl_type, f.name)].append(f)
        by_mod[(f.path.stem, f.name)].append(f)
    methods_by_name: dict[str, list[RustFn]] = defaultdict(list)
    free_by_name: dict[str, list[RustFn]] = defaultdict(list)
    for f in fns:
        (methods_by_name if f.impl_type else free_by_name)[f.name].append(f)
        # A trait default method is reachable both ways: as `.name(...)` on a
        # host that does not override it, and - since it was a free function
        # to every earlier pass - through the bare-identifier edge. Listing it
        # in both tables is purely additive, so it cannot introduce a new
        # false negative.
        if f.is_trait_default:
            free_by_name[f.name].append(f)

    # Note on a rule that was tried and removed: restricting `.method(` edges
    # to types the calling crate names by hand. It does not help - the
    # collisions that matter are intra-crate (`.tick()` inside `engine-core`
    # reaching `engine-core`'s own inert `SaveScreenMachine::tick`) - and it
    # trades away the graph's one useful property, that every ambiguity
    # resolves toward reachability. Keeping the graph purely
    # over-approximating is what makes `--not-live` a hard floor.

    # Receiver gate (strict only): the set of type names each file mentions
    # anywhere in its own stripped source. A method edge into `impl Type` is
    # kept only if the calling file names `Type`, or defines that method.
    named_types: dict[Path, set[str]] = {}
    if strict:
        all_impl_types = {f.impl_type for f in fns if f.impl_type}
        for p, s in srcs.items():
            words = set(IDENT_RE.findall(s.stripped))
            named_types[p] = words & all_impl_types

    def gated(caller: RustFn, cands: list[RustFn]) -> list[RustFn]:
        """Apply the receiver gate, but only where there is ambiguity to resolve.

        A method name with exactly one definition in the tree is already
        unambiguous, so the gate has nothing to decide and must not fire: the
        receiver is routinely a local binding whose type the calling file never
        spells (`ctrl.run_horizon_emitter(..)`), and gating on the spelling
        would drop a real edge. Dropping a real edge is the one failure mode
        this graph must not have - it converts a correct `NOT WIRED` disclosure
        into a silent omission from the audit.
        """
        if not strict or len(cands) < 2:
            return cands
        return [
            c
            for c in cands
            if not c.impl_type
            or c.path == caller.path
            or c.impl_type in named_types.get(caller.path, ())
        ]

    edges: dict[int, set[int]] = defaultdict(set)
    for src in srcs.values():
        for f in src.fns:
            body = src.stripped[f.body_start : f.body_end]
            targets: set[int] = set()
            consumed: set[tuple[int, int]] = set()
            for m in QUAL_CALL_RE.finditer(body):
                qual, name = m.group(1), m.group(2)
                consumed.add(m.span(2))
                if qual == "Self" and f.impl_type:
                    qual = f.impl_type
                # Fall back to *free* functions only, never to methods. An
                # unresolved qualifier is nearly always an external type
                # (`Vec::new`, `String::from`, `Duration::from_secs`), and
                # letting those reach every in-tree `Type::new` wires up whole
                # modules that nothing constructs.
                cands = (
                    by_qual.get((qual, name))
                    or by_mod.get((qual, name))
                    or free_by_name.get(name, [])
                )
                targets.update(c.uid for c in cands)
            for m in METHOD_CALL_RE.finditer(body):
                consumed.add(m.span(1))
                targets.update(
                    c.uid for c in gated(f, methods_by_name.get(m.group(1), []))
                )
            for m in BARE_CALL_RE.finditer(body):
                if m.span(1) in consumed or m.group(1) in RUST_KEYWORDS:
                    continue
                consumed.add(m.span(1))
                targets.update(c.uid for c in gated(f, by_name.get(m.group(1), [])))
            for m in IDENT_RE.finditer(body):
                if m.span(1) in consumed or m.group(1) in RUST_KEYWORDS:
                    continue
                # A field declaration (`name: Type`) or a struct-literal key
                # (`Foo { name: v }`) is not a function value. `::` is not a
                # field separator, so exclude it from the lookahead.
                if strict and FIELD_COLON_RE.match(body, m.end(1)):
                    continue
                targets.update(c.uid for c in free_by_name.get(m.group(1), []))
            targets.discard(f.uid)
            edges[f.uid] |= targets
    return fns, edges


def reachable_fns(
    fns: list[RustFn], edges: dict[int, set[int]], roots: list[RustFn]
) -> set[int]:
    seen = {r.uid for r in roots}
    queue: deque[int] = deque(seen)
    while queue:
        for nxt in edges.get(queue.popleft(), ()):
            if nxt not in seen:
                seen.add(nxt)
                queue.append(nxt)
    return seen
