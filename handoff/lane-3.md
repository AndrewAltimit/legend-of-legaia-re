# Lane 3 handoff - the battle-band worklist rows

Eight worklist rows, all in the `battle_action` (PROT 0898) band except
`801F1278`. Every one is ported and tagged in `crates/engine-vm`; none is wired,
and each carries a `NOT WIRED` disclosure in its module doc naming the specific
prerequisite. This page records the wiring each one needs, since in every case
the work lands in a crate Lane 3 does not own.

## Per-row wiring

### `FUN_801EC0DC` -> `engine-vm::battle_formulas::monster_escape_roll`

The **monster escape roll** - the enemy-side mirror of the party roll
`FUN_801E791C`. The retail caller is the monster action picker `FUN_801E9FD4`,
whose port is `engine-core::monster_ai`.

Wiring is small and worth doing: `monster_ai::decide` needs a "flee instead of
act" branch that, when the roll passes, seeds action category `+0x1DE = 5`. That
category already routes to `ctx[7] == 0x68` in `engine-vm::battle_action` (the
monster arm of the Run band), so the downstream state machine is already there.
Inputs the picker must supply:

- `ctx[+0x287]` (the no-escape byte `engine-vm`'s `EscapeFlags::no_escape`
  already models on the party side),
- per-slot `FleeActor { hp, max_hp, atk }` for both sides,
- each party slot's character-record `+0xF8` word, for the **No Escape** /
  Chicken Guard bit `0x400000` (`engine-core::accessory_passives` already
  aggregates that field),
- the fleeing monster's INT (`+0x168`), which `engine-core` already seeds into
  `battle_evasion`.

`rand` is a closure, not an array, because retail's third draw only happens once
the score compare passes - a caller that mirrors the RNG stream must not
over-consume.

### `FUN_801D0290` -> `engine-vm::battle_action::OverlayRng`

The battle overlay's own generator. Nothing to wire until a consumer is found:
its five call sites are all inside `FUN_801CFB94`, the overlay's leading
function, which is unported, and which battle quantities the draws feed is still
open. Note for whoever picks that up - because the state lives in overlay memory,
draws from it must **not** be routed through the engine's SCUS `rand()` stream,
or the determinism oracle diverges.

### `FUN_801D02C0` -> `engine-vm::battle_ground_grid`

The procedural battle floor. **`engine-shell` already has a mirror** -
`build_battle_ground_grid` in `play-window`, plus the tile constants in
`window/geometry.rs`. Wiring means that mirror calling this module for the grid
origin, the per-cell depth classes, the `3x3` lattice and the sub-tile UVs rather
than recomputing them. Two things the engine mirror should pick up while it is
there, both corrected in this wave (see below).

### `FUN_801E0080` -> `engine-vm::battle_scatter`

The arena's emitter/particle scatter. Needs two things neither of which is in
`engine-vm`:

- **`engine-core`**: the per-scene battle buffer `_DAT_8007BD30` with its emitter
  pool (`+0x1010`), particle pool (`+0x10`), spawn-definition pointer table
  (`+8`) and sprite-descriptor table (`+4`). These are disc-side; the
  `ScatterEnv` trait is the attach point.
- **`engine-render`**: a consumer for `SpriteDraw` (one `0x28`-byte textured quad
  per live particle).

### `FUN_801F0450` -> `engine-vm::battle_arts_auto_combo`

The AI-side Arts assembler. The **auto-fill arm is nearly wireable today**: it
needs only the character's learned-arts list (`record[+0x185]` count,
`record[+0x186 + i]` ids) and the live monster count, and it already composes with
`engine-vm::battle_action::redirect_dead_target`. `engine-core` currently drives
auto-fighting party members through a stand-in physical action; swapping in this
arm would replace a stand-in with retail behaviour.

The pool arm needs two disc tables reaching `engine-core`'s battle setup: the
per-(character, weapon) arts-command records `DAT_801C9360[slot][cmd]` with their
`+0x74` AP costs, and the four-entry status-guard mask table at `0x801F672C`.

### `FUN_801D71B8` -> `engine-vm::battle_attack_camera`

The per-art attack camera. Blocked on a **disc parser**: the seventeen per-art
arms each fold a halfword track out of the per-phase table at `0x801F4E10` in
PROT 0898's tail. `ArmPhase::track_offset` records the indexing (phase stride 2,
track stride 4) so the table can be parsed without re-deriving it. The engine
side also needs a per-art camera channel; today it frames battles with a
phase-scripted snap through `BattleActionHost::camera_bounds`.

### `FUN_801E805C` -> `engine-vm::battle_value_readout`

The multi-cast value readout. Its producer is the summon side band (the
`0x801F6980` value window and `0x801F6988` slot list, written by PROT 0900 / the
`readef` streaming slots) and its output is ordering-table quads. `engine-ui`
draws battle damage numbers through its own `TextDraw` path, so wiring means
either routing the side band into `engine-core` or - cheaper - having `engine-ui`
adopt `decimal_digits` and `label_quad` so the two agree on digit suppression and
label geometry.

### `FUN_801F1278` -> `engine-vm::field_party_cursor`

The field VM's op-`0x49` party-member picker, enter half. The resume/close half is
`FUN_801F159C`, unported. Wiring means a picker submode on the engine's field
side: `engine-core`'s menu runtime holds nothing for the cursor context, and the
three portrait cells would need to reach `engine-ui`. Worth flagging for whoever
does: the picker is **centre-weighted**, not left-packed (one member in the middle
cell, two members in the outer cells) - see `seed_member_cells`.

## Doc claims corrected in this lane

Both are disassembly-grounded, and both had a committed doc asserting the
opposite.

1. **`FUN_801D0290`'s final `addu` is exactly a rotate.**
   `docs/subsystems/battle-action.md` and `docs/reference/functions/battle.md`
   both said "an `addu` of the halves, so a carry propagates and it is **not** a
   rotate". It cannot carry: `v << 16` has sixteen zero low bits and `v >> 16`
   (an `srl`, not an `sra`) has sixteen zero high bits, so the operands are
   disjoint and the `addu` is bit-for-bit an `or`. Both pages are fixed, and
   `overlay_rng.rs` asserts the equivalence over the halfword boundaries plus
   20 000 probes instead of restating it.

2. **`FUN_801D02C0`'s sub-tiling is deterministic, not a random per-cell pick.**
   `docs/subsystems/battle.md` says "each cell samples one sub-tile with a
   per-cell random corner mirror". The emit loop runs 2x2 times per visible cell
   and advances the sub-tile row pointer by `0x10` each time, so a single 64x64
   texture is stretched across one whole `0x200` cell as four quads, sub-tile
   `= sub_row * 2 + sub_col`, with no RNG anywhere in the routine. The random
   corner mirror is real but belongs to the *particle* scatter `FUN_801E0080`
   (`rand() % 4` -> two mirror bits). **`docs/subsystems/battle.md` is outside
   Lane 3's file scope** - the corrected reading is written up in
   `docs/reference/functions/battle.md#801d02c0`; whoever owns `battle.md` should
   fix that sentence there and drop the "two distinct variants, each duplicated
   across the row" gloss, which is a claim about the texture content and not
   about this routine.

## Notes for the coordinator

- `801D0290` was independently confirmed by capstone over
  `extracted/overlays/overlay_battle_action_0898.bin` at base `0x801CE818`
  **before** the port was written, not from the misleading `overlay_0897` dump at
  that VA (a five-instruction field-VM label slice). The reproduction command is
  now recorded in `battle-action.md`'s PRNG section so the next reader does not
  have to rediscover it.
- `801F30C4` and `801D84C0` were mentioned as Lane 3 rows in coordination
  traffic; they are **not** in Lane 3's brief and are not ported here. They
  belong to another lane's assignment.
- `801F1278`'s primary reference is `docs/subsystems/script-vm.md`, which is
  outside Lane 3's file scope. That page's row for the address is accurate; it
  just does not yet say the routine is ported.
