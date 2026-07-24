# L7 (battle core) handoff

Findings that belong to other lanes' files, or to the Gaza-2 softlock
investigation running in parallel. Nothing here is a task for L7.

## For the Gaza-2 softlock investigation

### One addition to the `actor[+0x10]` sign convention

The rest of the `+0x10` machinery matches the ground truth already circulated,
independently re-read off the listing here. One point that was not in it:

**`FUN_800402F4`'s `-delta` is a negated *stat change*, not a negated damage
amount.** The `s4` it negates at `0x800408F4` is the same value it folded into
the stat halfword sixty instructions earlier (`0x800408A8`:
`lhu v0,0x0(v1); addu v0,v0,s4; sh v0,0x0(v1)`), so damage arrives as a
negative `s4` and a heal as a positive one. Negating it therefore lands on the
*same* positive-means-the-readout-falls convention `FUN_801EC3E4` uses, rather
than the opposite one. Anyone reading the seed in isolation is likely to get
the sign backwards.

### A second, independent park of the same shape

`FUN_801E09F8`'s census head (`0x801E0A44..0x801E0BF0`) rebuilds `ctx[+0x249]`
from zero every frame as "visible actors mid-animation", where *visible* is
`actor[+0x4] != 0` and *mid-animation* is `actor[+0x1D9] != 0`, minus party
slots sitting in anim `8` (the downed pose). State `0x2E` (`MagicExit`) waits
on `+0x249 == 0`.

That makes the magic band's exit a **measurement**, not a latch, with exactly
the same failure mode as the HP-bar settle: any actor left with a non-zero
render word and a stuck `+0x1D9` holds every subsequent cast open forever, and
nothing re-syncs it. Worth checking alongside the `0x51` park when a Gaza-2
capture shows an endless orbit - the two are distinguishable by which state
byte `ctx[7]` holds (`0x51` vs `0x2E`).

`ctx[+0x24D]` (the state-`0x2D` recovery gate) counts non-zero entries in
`ctx[+0x252..=+0x255]`, but **only if** at least one entry of
`ctx[+0x24E..=+0x251]` is non-zero - retail returns from the whole tick before
the count otherwise (`0x801E0BA8`). So an empty kind array reads as "nothing
outstanding" no matter what the child array holds.

### Ported surfaces the investigation can drive

- `legaia_engine_vm::battle_hp_bar` - the ramp arithmetic, both seeding
  conventions, the re-sync.
- `legaia_engine_vm::battle_action::tick_hp_bars` / `tick_cast_census` -
  per-frame drivers over a `BattleActionHost`.
- `engine-core`: `World::apply_battle_hp_delta` (the single seeding entry
  point), `World::tick_battle_hp_bars`, `World::tick_battle_cast_census`.
- Engine-side reproduction of the `0x51` park, driven from the clamp
  asymmetry rather than a hand-set field: `engine-vm` test
  `state_51_park_from_the_clamp_asymmetry` (survivable hit bigger than the
  lagging readout → readout drains to `0`, live HP positive, accumulator `0`,
  gate never releases). Its simpler sibling
  `state_51_parks_forever_on_a_desynced_bar_with_a_zero_accumulator` starts
  from the absorbing state directly.

## For whoever owns the battle HUD (`engine-shell/.../window/hud.rs`)

`BattleHud::sync_slot` is fed live HP (`BattleActor::hp`). Retail draws the
**display** value `+0x172`. Now that `hp_display` actually animates, feeding
the HUD `hp_display.unwrap_or(hp)` would make the drawn bar slide the way
retail's does. That file is outside L7's scope, so it was left alone.

## For the field / minigame lanes - worklist rows mis-filed to L7

Ten of the nineteen rows assigned to L7 resolve to overlays L7 does not own.
`classify-worklist.py --explain` arbitrates each against the extracted images:

| addr | owning image | insns |
|---|---|---|
| `801d6704` | field(897) | 901 |
| `801d7518` | field(897) | 183 |
| `801d9c3c` | field(897) | 61 |
| `801da390` | field(897) | 99 |
| `801ddc20` | field(897) | 133 |
| `801de478` | field(897) | 20 |
| `801e6984` | field(897) | 108 |
| `801f1278` | field(897) | 201 |
| `801cf00c` | baka_fighter(976) | 223 |
| `801ce844` | gameover(902) | 193 |

All ten classify `REAL`, so they are genuine work - just not in
`crates/engine-vm/src/battle_*` or `crates/engine-core`'s battle path.
