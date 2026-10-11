"""The numeric thresholds and name sets each signal is tuned by.
"""

from __future__ import annotations

from port_provenance.paths import REPO

# ---------------------------------------------------------------------------
# Tuning. Every threshold here is a precision/recall dial; the defaults were
# picked by triaging the report by hand, not by fitting a number.
# ---------------------------------------------------------------------------

# A data address is "distinctive" when at most this many dumps in the whole
# corpus form it. Above the cut it is a shared runtime global (the game-state
# window, the item-name table) and carries no subsystem information.
DISTINCTIVE_DF_MAX = 8

# Same cut for callees. Without it the signal is masked: the text writer
# `0x80036888` is called from 82 dumps and the string blitter `0x8002b994` from
# 39, so "these two routines share a callee" is true of almost any pair and
# corroborates nothing.
CALLEE_DF_MAX = 8

# `split-table` reports across module boundaries, where a wrong hit costs a
# reader more, so it wants the rarest addresses only.
SPLIT_DF_MAX = 3

# PSX main RAM. The abstract interpreter can synthesise an address from a `lui`
# whose low half never arrives on the path taken; anything outside the 2 MB the
# game can address is such an artifact and is dropped rather than ranked.
RAM_LO = 0x80000000
RAM_HI = 0x801FFFFF

# Overlays are linked over this window, several of them at the same base, so a
# VA at or above it identifies an object only together with the image it was
# read in. Below it the address is always-resident and means one thing.
OVERLAY_BASE = 0x801C0000

# The image name a dump below `OVERLAY_BASE` is given when its header carries
# none. Spelled the way the extracted images are, so the two agree in output.
SCUS_IMAGE = "SCUS_942.54"

# Per-address owning image, read from `dump-extent-attribution.csv`: which
# extracted image actually holds a VA's bytes, decided by reproducing the
# dump's own instruction window against each image's content. `consensus`
# prefers the dumps from that image, so a dump taken at the same VA under a
# different overlay never votes on what the routine is.
ATTRIBUTION_CSV = (
    REPO / "scripts" / "ghidra-analysis" / "dump-extent-attribution.csv"
)
# Only these attribution classes name an owner worth deferring to. `misbased`
# says the bytes are somewhere else entirely and `short` / `unresolved` say the
# instrument could not decide, so neither settles which dump to believe.
ATTRIBUTION_TRUSTED = frozenset({"unique", "resolved_by_table"})

# `module-orphan` needs enough siblings for "shares nothing" to mean anything.
ORPHAN_MIN_SIBLINGS = 3

# What share of its own address span a dump has to print before "this routine
# touches nothing its siblings touch" is a statement about the routine rather
# than about the dump. A jump-table dispatcher is routinely dumped as its head
# plus its shared epilogue and nothing in between - `FUN_801EA9B0` prints 25
# instructions across a 250-instruction span, so 90% of it was never read.
#
# Two distinct shapes live under 1.0 and the floor sits between them: a stub of
# that kind prints a tenth or less, while a body with one arm skipped prints
# about half (`FUN_801DD9D4`: 69 instructions over a 147-slot span, 0.47). A
# floor at 0.5 would have taken the second kind with the first - and did, in
# testing, silence a known-real defect.
DUMP_COVERAGE_MIN = 0.25

# One fixed distinctiveness cut cannot read every module, and the failure is not
# random: it is a property of what the module ports.
#
# A window-descriptor **painter** has exactly one datum of its own - its private
# overlay string pool - and calls only the corpus-wide text writer `0x80036888`,
# number writer `0x80034B78` and marker blitter `0x8002B994`. Its siblings share
# the choice-state word and the panel-install callee, both far above the cut. So
# every painter in a painter module reads as sharing nothing with every other
# one, and the module reports as many orphans as it has members. The same shape
# one level up produced eleven more from one actor-hub module.
#
# The fix is to let the cut follow the module. A module whose members really do
# belong together links up at *some* rarity; where that happens says how private
# its shared vocabulary is, and only a member that stays unlinked there is
# saying anything. `menu/window_painters.rs` links at 96 and has no orphan
# left; `world/field_movement.rs` is already cohesive at the base cut and its
# orphan stays one at every cut on the ladder.
DF_LADDER = (8, 12, 16, 24, 32, 48, 64, 96)

# ...but a per-module estimate needs pairs to estimate from. Five tags are ten
# pairs and several of those are vacuous; the cut such a module "needs" is one
# pair's accident. Below this many tagged addresses the base cut stands, and
# the cheapest way to see why is `menu/equip_stat_compare.rs`: four tags,
# two of them corroborating, and its one genuinely wrong tag stops reading as an
# orphan the moment the cut reaches 16.
ADAPTIVE_MIN_TAGS = 6

# What "the module reads as cohesive" means: this share of its tagged addresses
# corroborating at least one other. Under it, "shares nothing" is the module's
# normal condition and says nothing about any member.
#
# This is a fitted number and it should be read as one. It was chosen by
# sweeping it against the 66 hand-reviewed verdicts behind
# `docs/tooling/port-provenance.md`, under the constraint that every one of the
# four real defects keeps firing. 0.95 breaks that constraint - it absorbs three
# of the four - so the constraint, not the sweep, is what fixes the ceiling.
ADAPTIVE_TARGET = 0.85

# Tokens too generic to make two module names "related".
STOPWORD_TOKENS = {
    "mod",
    "lib",
    "main",
    "src",
    "crates",
    "engine",
    "core",
    "render",
    "util",
    "utils",
    "common",
    "state",
    "data",
    "table",
    "tables",
}

# The one dump window whose decoded `jal` targets are a property of a wrong
# load base (docs/tooling/call-target-integrity.md).
UNTRUSTED_JAL_IMAGE = "overlay_0896"
UNTRUSTED_JAL_BELOW = 0x801CE818
