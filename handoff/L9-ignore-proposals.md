# L9 handoff - `scripts/ci/port-catalog-ignore.toml` corrections

Lane L9 owned the `801c****` / `801d****` half of the "dumped but undocumented"
list (126 addresses). **No new ignore entries are proposed: all 126 are already
in `port-catalog-ignore.toml`.** What follows is the opposite - six merged rows
whose *reason strings* name a true VA that the bytes do not resolve to, plus one
row whose verdict deletes a real function.

`worklist-classification.md --audit-ignored` has a rule for exactly this ("the
merged reason names a true VA the dumped bytes do not resolve to → re-raise"),
so these are re-raises the audit should have produced and did not, because the
wrong VAs came from the same ambiguous multi-hit resolutions the audit reuses.

Method for every row below: take the dump's opening instruction window,
canonicalise it the way `check-dump-base-integrity.py` does, and compare it
against the extracted image at the *batch* delta measured from that dump
program's other dumps. A batch delta plus an exact token match beats a
short-window match at an arbitrary offset. Full per-address table:
`docs/tooling/phantom-print-index.md`.

## Reason strings naming a wrong true VA

| Row | Merged reason says | Bytes actually at | Evidence |
|---|---|---|---|
| `801d0e78` | `801df638` `+0xe7c0` field | `801df690` `+0xE818` field | 7/7 tokens at the batch delta; the `+0xe7c0` hit is one of three and non-canonical |
| `801d3730` | `801df400` `+0xbcd0` field | `801e1f48` `+0xE818` field | 8/8 tokens at the batch delta |
| `801d7d4c` | `801d7538` `-0x814` debug_menu 0971 | `801e6564` `+0xE818` field (0897 dump) and `801dd564` `+0x5818` field (0896 dump) | 8/8 and 7/8 tokens; the one differing token is a `break` operand, which Ghidra and capstone render differently (10-bit code vs 20-bit immediate) |
| `801d873c` | `801d8a08` `+0x2cc` slot_machine 0975 | `801e6f54` `+0xE818` field | 7/7 tokens at the batch delta |
| `801da8bc` | `801e7d88` `+0xd4cc` field | `801e90d4` `+0xE818` field | 8/8 tokens at the batch delta |
| `801db49c` | `801db81c` `+0x380` menu 0899 | `801e0cb4` `+0x5818` field | 6/6 tokens at the batch delta; `+0x380` was a 121-hit resolution |

The verdicts (`worklist_misbased_print`) stand in every case. Only the true VA in
the reason is wrong, and a wrong true VA is not harmless: it is the pointer a
future lane follows to find the real routine.

## One row whose verdict is wrong

`801d5a24` is filed under `[va_aliased_overlay_local]` on the grounds that "the
0897 copy is a 124 B mid-function fragment (no prologue, unaff…)". That describes
the **mis-based** `overlay_0897_801d5a24.txt` dump, whose bytes live at field
`0x801E423C`, inside `FUN_801E3E00`.

At the correct base, `overlay_field_0897.bin` has a genuine routine at
`0x801D5A24`: it opens `addiu sp,sp,-0x18`, is preceded by a clean `jr ra` +
epilogue pair, and returns on its own frame. It is a parameterised actor-spawn
helper - `FUN_80020DE0(&DAT_801F26D8, _DAT_8007C34C)`, then `actor[+0x54] = 0`
and `actor[+0x50] = <argument>`. It is now written up in
`docs/reference/functions.md` under the field-VM helper table.

The VA *is* aliased - fishing 0972 has interior code there (inside
`FUN_801D56E4`) and baka_fighter 0976 likewise (inside `FUN_801CF388`) - so the
row should stay in `va_aliased_overlay_local`, but with a reason that names the
field-overlay routine as a real port site rather than as a fragment.

## Reasons that are correct but weaker than they need to be

The `overlay_0896_*` rows whose printed offset from `0x801C0000` is at or above
`0x9000` currently read "VA is below `0x801CE818` … no overlay image can contain
it", with no true VA. They are all re-keyable at `+0x5818`:

`801c9c04` `801cac44` `801cae64` `801cc810` `801cce38` `801cd510` `801cd628`
`801cd728` `801cd844` `801cd9c0` `801cdafc` `801cdb1c` `801cdb48`.

Resolved VAs are in `docs/tooling/phantom-print-index.md`. `801c9c04` is the odd
one: `+0x5818` lands at `0x801CF41C`, inside the field overlay's leading data
segment, so it decodes data as code and there is no routine at either address.

The six rows whose printed offset is **below** `0x9000` (`801c5cf8` `801c5e28`
`801c6534` `801c8178` `801c81a8` `801c82e0`) really are unrecoverable: that
window is PROT 0896's own content and its link base is still unrecovered.
