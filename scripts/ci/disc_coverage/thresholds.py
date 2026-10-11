"""The classification thresholds and the PROT class sets the report groups by.
"""

from __future__ import annotations

# MIPS I primary opcodes the R3000A actually issues. Used only to tell a gap of
# code from a gap of data; it is a statistical test over a whole gap, never a
# per-instruction decode.
PLAUSIBLE_OPS = {
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
    16, 17, 18, 32, 33, 34, 35, 36, 37, 38, 40, 41, 42, 43, 46, 47, 49, 57,
}
# A gap counts as code when nearly every word decodes to a plausible opcode AND
# it is not dense with 0x80xxxxxx words (the signature of a pointer table).
CODE_PLAUSIBLE_MIN = 0.90
CODE_PTR_MAX = 0.03
# Gaps shorter than this are inter-function alignment padding, not a finding.
TINY_GAP_WORDS = 8

# How many unattributed extents the lag report names before it
# summarises. The list is a worklist for one CSV re-run, not a
# census, and the pre-commit hook prints it with --quiet.
LAG_EXTENTS_SHOWN = 20

# PROT classes that are placeholders or absence rather than an unparsed format.
# `pochi_filler` is a DOCUMENTED class (docs/formats/pochi.md), so it counts as
# explained; it is broken out separately because calling reserved dev filler
# "content we understand" overstates the result.
PLACEHOLDER_CLASSES = {"pochi_filler", "mostly_zeros", "zero_sector_high_entropy"}

# The subset of PLACEHOLDER_CLASSES a STRUCTURAL detector confirms is filler,
# and so the only bytes the content figure (`pct_content_parsed`) drops from
# its denominator. `mostly_zeros` / `zero_sector_high_entropy` are statistical
# fallbacks - the absence of a finding, not a finding (docs/tooling/
# disc-coverage.md, "A statistical class is not a verdict") - so they stay in
# the denominator and count against the figure: a real format misfiled there
# must still read as a gap.
FILLER_CLASSES = {"pochi_filler"}
UNEXPLAINED_CLASSES = {
    "unknown", "unknown_other", "unknown_high_entropy", "unknown_low_entropy",
}
