# Lane G - per-art AP: cost side, menu display, per-character keying

Three user reports, one feature area. All three landed; one is closed with a
negative finding that changes how the feature had to be built.

## 1. Where an art's AP cost lives - it does not

**Retail stores no per-art AP cost.** Read off the disassembly of the party arts
queue-builder `FUN_801EED1C` (PROT 0898,
`ghidra/scripts/funcs/overlay_battle_action_801eed1c.txt`), the cost is
*computed*:

```text
801ef31c  lbu  a0,0x40(sp)      ; rows visited so far for this character
801ef328  _li  t4,0xb           ;   0      -> 11
801ef32c  li   t4,0xa           ;   1..3   -> 10
801ef33c  li   t4,0x6           ;   >= 4   -> 6
801ef378  srl  t4,t4,0x1        ; halved under the actor's 0x800 flag
...
801ef40c  _mult t4,s1  / 801ef414 mflo t7   ; required AP -> affordability gate
801ef474  mult  t4,v0  / 801ef478 mflo a2   ; charged AP
```

`cost = multiplier x command_count`. There is no record field to patch, so a
per-art cost only exists once a hook introduces one. That is the honest answer to
"find where the cost lives": nowhere.

Two further facts that were load-bearing:

- **The in-builder debit is not the spend.** Site C's `subu v0,v0,a2` is undone
  by the builder's own tail (`Spirit += actor[+0x224]` at `0x801EF988`); it exists
  so a *chained* art's gate accounts for what earlier arts in the run committed.
  The real charge is `subu v0,v0,a0` at `0x801E5D74` in the battle-action cleanup
  arm, out of the accumulator `+0x224`. A cost override has to move both, which
  the site-C routine now does.
- **Which AP.** This is the Spirit gauge `actor[+0x170]` (the `--spirit-ap`
  gauge), not the command-gauge arm width `DAT_801C9360[char][cmd]+0x74`. The
  `arts-command-gauge.md` page now separates the two explicitly.

## 2. The menu arts list reads a different byte

The number the pause menu shows is `+2` of the SCUS arts-name table record
(`DAT_80075EC4 + n*0x14`). Scanning every PROT entry plus SCUS for references to
`0x80075EC4` gives exactly **one** reader of that byte in the whole image:
`lbu a0,0x2(s2)` at `0x801D4524` in the menu overlay's status-panel renderer
`FUN_801D33D8` (PROT 0899). So it is a **genuinely separate display field**, not
a cached copy - which is why the patch worked while the list disagreed.

Retail keeps the two consistent by authoring, and that is now pinned by a
disc-gated test (`retail_menu_ap_mirrors_the_builder_formula`): for all 45 arts
the byte equals `multiplier x command_count` **byte-exact**, with the multiplier
keyed by the art's *visit order*, not its display index. Noa is the discriminator
- her display indices skip 2 and 3, so her index-4 Vulture Blade carries `10 x 5`
and not `6 x 5`. That test is an independent confirmation of the disassembly
reading above.

The patcher now rewrites that byte for every targeted art.

## 3. Grant vs cost in the list - what fits, and what does not

The field is drawn by `FUN_80034B78(value, 3, x, y)`: a right-aligned 3-cell
decimal, digit sprites only (`u = digit*8, v = 0xD0`), no sign path, and the
value is decomposed decimally so a "digit 10" cell can never be emitted.
**A literal `+`/`-` is not reachable without injecting an extra sprite draw into
overlay 0899**, which was out of this change's risk budget.

What shipped instead: a cost writes its own number, a grant writes **`0`**. No
retail art carries 0 (the retail minimum is 18) and the smallest configurable
cost is 1, so `0` is unambiguous in a patched game - the marker for "this art
pays you". The site copy, the CLI report and the docs all say so in those words.

Open thread if someone wants the real glyph: find a marker sprite in the menu
atlas (the digit strip's cell 10 at `u = 0x50, v = 0xD0` is the obvious
candidate but is **unverified** - do not assume it) and inject an 8-px sprite
draw at `x - 8` in 0899.

## 4. The sharing was ours, not the game's

`s3 - 0x0B` (the arts-table display index) is shared across characters, but the
builder **already holds the character**: `t6 = &DAT_8007BD10[slot]` (built at
`0x801EF30C`, read by retail itself as `lbu v0,0x0(t6)` at `0x801EF340`), and
`DAT_8007BD10[slot]` is the 1-based party-record id. The injected routines replay
that load, so the config index is now

```text
index = (DAT_8007BD10[slot] - 1) * 32 + (s3 - 0x0B)
```

over a `4 x 32` `i8` table. One art per cell. The "shared menu slot, so it also
applies to ..." note is **gone** from the UI, and the disc oracle asserts that a
Vahn-only override leaves Noa's and Gala's cells *and menu bytes* at retail.

`--arts-power` (damage) is unchanged and still combo-keyed - it rewrites the
shared art record - so only the damage note mentions collateral now.

## What changed

- `crates/patcher/src/arts_ap_grant.rs` - rewritten. Signed config
  (`> 0` grant, `< 0` cost, `0` retail), per-(character, row) keying, the cost
  arms in the guard + debit routines, and the display-byte edits.
  `ApMode` / `ArtApSpec` / `ResolvedArtAp` replace the old tuple + `ResolvedGrant`.
- `crates/patcher/src/mips.rs` - added `subu`.
- Placement moved: guard + debit in `ARENA1_VA` (236/256 B), refund in
  `ARENA2_VA`, the 128-byte config table in `SCUS_GAP_VA`. All three are
  read-watch-verified dead and all three are shiny-Seru's, so the existing mutual
  exclusion still covers it (and is asserted both ways in the oracle).
- CLI: `--arts-ap-cost`, and both AP flags now take `[CHARACTER:]COMBO=AMOUNT`.
- `crates/web-viewer/src/rom_patcher.rs` - new `arts_ap_costs` parameter
  (positional, after `arts_ap_grants`). **The wasm bundle was rebuilt** with
  `scripts/ci/build-wasm.sh`; `site/wasm/` and `crates/web-viewer/pkg/` are in
  the commit.
- `site/js/rom-patcher-app.js` - AP select is now Keep original / Costs AP /
  Gives AP back; the amount box shows in both active modes, keyed per character,
  with the sharing note removed.
- Docs: `arts-command-gauge.md` (new "What an art costs in AP" section +
  rewritten hook section), `art-data.md` (the `+2` byte is a display mirror),
  `randomizer.md`, `crates/patcher/README.md`.

## Verification status - read this before shipping

Proven:

- 6/6 disc-gated tests in `crates/patcher/tests/arts_ap_grant_real.rs` pass
  against the real disc (0 skips with `LEGAIA_DISC_BIN` set, 6 `[skip]` without).
- The formula test confirms the disassembly reading against all 45 arts.
- 8/8 unit tests pin every branch target and displaced-word replay in the three
  hand-assembled routines.

**Not proven: in-game behaviour.** Nobody has yet booted a patched disc, opened
the arts list and confirmed the number, nor fought a battle to confirm a cost
art gates and charges its configured amount. The routines are hand-assembled
MIPS in a battle-critical path; treat this as unshipped until someone with a pad
does that pass. The specific claims to check:

1. A cost art is refused below its configured cost and admitted at/above it.
2. Its charge lands once (watch `actor[+0x170]` across the action, not just
   during input - the builder's debit/refund pair will mislead a single sample).
3. A grant art still admits at 0 AP and clamps at 100.
4. The pause-menu arts list shows the configured number, and `0` for a grant.
