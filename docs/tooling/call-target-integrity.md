# Call-target integrity

Attribution in this project leans on decoded call targets: a `jal` from one
dump into another is what links a subsystem to its helpers, seeds the
[port catalog](port-catalog.md)'s feature-BFS, and justifies rows in
[`docs/reference/functions.md`](../reference/functions.md). This page states
what a decoded target does and does not prove, and how to detect the one case
where the corpus produces targets that look real and are not.

## A `jal` target is a property of the bytes, not of the base

MIPS encodes a `jal` target absolutely:

```
target = (PC + 4)[31:28] || imm26 || 00
```

Only the top four bits come from the program counter. Every Legaia load base
shares them - the always-resident executable sits at `0x800xxxxx`, the
swappable overlay slots at `0x801Cxxxx` and up, all nibble `0x8`. Re-basing a
program therefore shifts the address printed for the *call site* and can never
change the *decoded target*.

This has a direct consequence worth stating plainly, because it rules out an
attractive-looking hypothesis: **a wrong load base cannot manufacture a
phantom call target.** If a dump is disassembled at the wrong base, its
instruction addresses are wrong and its targets are still right. So
"the targets in this overlay look wrong, therefore the overlay is mis-based"
does not follow, and neither does its converse.

What a wrong base *can* do is put correct targets next to incorrect call-site
addresses, which is a labelling problem, not a decoding one.

## What a decoded target does not prove

A decoded target proves that some 32-bit word in the image encodes a call to
that address. It does not prove the word is an instruction, that the code
around it ever executes, or that the address is a function entry in the image
being compared against.

The check that closes that gap is cheap: resolve the target against a known
function entry in the target image. Genuinely resident code that genuinely
addresses the retail layout resolves essentially every time. The residual
misses are dominated by leaf stubs whose entry is a bare immediate load rather
than a stack-frame adjust, so a prologue-shaped heuristic under-counts entries
slightly and a handful of isolated misses is normal. The signal worth acting
on is a *collapse* across a contiguous window, not an isolated miss.

## The `0x8002CDD0` case

`0x8002CDD0` is **not** a function entry. It is an interior address of
`FUN_8002C69C`, and three independent lines of evidence agree:

- Ghidra resolves it that way. The dump header carries
  `(entry=8002c69c)`, which is the field to trust over the queried address.
- The word there is `sh v0,0xe(a1)` - a store in the middle of a straight-line
  basic block, consuming a `v0` that the two preceding instructions compute
  from `v1`. It is reached only by fallthrough; no branch in the function
  targets it. Entering there is not meaningful.
- The byte-to-address mapping underneath that claim is sound. Across the
  whole corpus, no address in the always-resident executable's range is given
  two different disassemblies by two different dumps, and intra-executable
  `jal`s land on prologues - including the one that targets `0x8002C69C`
  itself, whose prologue is a textbook `addiu sp,sp,-0x68`.

So the duplicate-looking rows in `functions.md` really are one function
described twice. `0x8002D988` (a branch) and `0x8002DAA4` (a store) are the
same artifact against the same entry.

The call sites that appeared to contradict this are real bytes, correctly
decoded, and still not evidence of an entry - because of where they live.

## Scope: the `overlay_0896` window below `0x801CE818`

Every call site targeting `0x8002CDD0`, and every other target in the corpus
that lands on a store, a branch or a delay slot, comes from `overlay_0896`
dumps whose address falls below `0x801CE818`. No dump of the always-resident
executable, the field overlay, the menu overlay or the world-map overlays
calls any of them. The containment is total, and it lines up exactly with a
boundary that [`crates/asset/data/static-overlays.toml`](../../crates/asset/data/static-overlays.toml)
derives independently.

That file keeps PROT 0896 (CDNAME `bat_back_dat`) out of the static-overlay
map on the grounds that its widely-cited base `0x801C5818` is an over-read
artifact: the entry's footprint runs into the neighbouring overlay's bytes, so
whole-file base recovery is dominated by the neighbour's code and returns
`0x801CE818 - 0x9000` by construction. PROT 0896's own link base is
unrecovered.

The resolve rate splits on precisely that seam. `overlay_0896` dumps at or
above `0x801CE818` - the over-read neighbour's code, correctly based - resolve
at 100%. Dumps below it resolve at roughly a third, and their misses land on
non-enterable addresses. For comparison, every other program in the corpus
sits near 100%.

The deltas from each unresolved target back to the nearest preceding prologue
are unrelated to each other, ranging from `0x20` to over `0x1400`. A uniformly
shifted build would produce one constant delta. These bytes do not address the
retail executable's layout under any single offset.

## Verdict

| Question | Answer |
|---|---|
| Is `0x8002CDD0` a function entry? | No. Interior address of `FUN_8002C69C`. |
| Do the `jal`s to it exist in the bytes? | Yes, and they decode correctly. |
| Are they evidence of an entry? | No. They originate in a window whose link base is unrecovered. |
| Are `overlay_0896` targets trustworthy at or above `0x801CE818`? | Yes. |
| Are they trustworthy below `0x801CE818`? | No. |
| Is the wider corpus affected? | No. The failures are confined to that window. |

The blast radius is contained. Citation edges derived from `overlay_0896`
dumps below `0x801CE818` should not be relied on; edges from every other
program stand. What the window actually holds is a separate question, tracked
with the other PROT 0896 threads in
[`open-rev-eng-threads.md`](../reference/open-rev-eng-threads.md).

## Detecting it

`scripts/ghidra-analysis/check-jal-target-integrity.py` sweeps the dump corpus,
resolves every call target aimed at the always-resident executable against the
set of known entries, and flags any dump whose resolve rate falls below a
threshold:

```bash
scripts/ghidra-analysis/check-jal-target-integrity.py --threshold 90
```

It prints the offending targets with the instruction each one lands on, which
is what distinguishes the two failure modes. A target landing on a stack-frame
adjust or a bare immediate load is an unrecognized entry and harmless. A target
landing on a store, a branch or a delay slot is not enterable, and a window
full of them means the bytes are not what the dump says they are.

Run it after adding overlay dumps at a base recovered from call targets rather
than from a documented anchor - that is the case where the base can be
self-consistently wrong, as it is here.

See [`ghidra.md`](ghidra.md) for the dump scripts themselves and
[`static-overlay-pipeline.md`](static-overlay-pipeline.md) for how an overlay's
base gets recovered and what makes a recovery load-bearing.
