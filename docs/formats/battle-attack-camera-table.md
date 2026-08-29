# Battle attack-camera track table (`0x801F4E10`)

Twenty rows of two signed halfwords in the battle-action overlay's data tail
(PROT entry `0898`), read by the per-art attack camera `FUN_801D71B8` while a
party member's swing animation plays. Each row is a camera offset the arm adds
into the pose it hands the tween builder.

The second halfword is **not** a later phase of the same swing. `ctx[+0x26D]`,
the byte that selects the column, has exactly one writer in the corpus -
`FUN_8004E13C` in `SCUS_942.54` (`0x8004E2DC`), which stores `rand() % 2`
beside `ctx[+0x6DA] = (rand() % 2) * 0x800 + 0x280` and `ctx[+0xD] = 0`. So a
row holds **two alternative offsets and retail coin-flips between them once per
action**, which is why the same swing frames from two visibly different angles
on successive turns. `FUN_801D5854` forces the column to `0` when the acting
character id is `3` (`0x801D6A5C`).

Parser: `legaia_asset::battle_attack_camera_table`. Engine side:
`legaia-engine-vm::battle_attack_camera`.

| Property | Value | Confidence |
|---|---|---|
| Runtime VA | `0x801F4E10` | Confirmed |
| PROT entry | `0898` (battle-action overlay) | Confirmed |
| File offset | `0x265F8` (`VA − 0x801CE818`) | Confirmed |
| Row stride | 4 bytes (`u16[2]`) | Confirmed |
| Rows | 20 (`0x50` bytes total) | Confirmed |
| Element type | signed halfword, little-endian | Confirmed |
| Per-row meaning | per-arm; see [Row map](#row-map) | Confirmed |

## Addressing

An arm forms one base and then reads fixed displacements off it:

```text
801d731c  sll   a0,t2,0x1        ; t2 = ctx[+0x26D], the phase cursor
801d7324  lui   v0,0x801f
801d7328  addiu v0,v0,0x4e10     ; table base
801d732c  addu  a0,a0,v0         ; base + cursor * 2
801d7338  lhu   v1,0x0(a0)       ; row 0 at this cursor
801d734c  lhu   v0,0x4(a0)       ; row 1 at this cursor
```

So `value(row, column) = base[row * 4 + column * 2]`. The cursor `ctx[+0x26D]`
is binary everywhere it is used (`beq t2,zero,…` at `0x801D7398` and its
siblings), which agrees with its writer storing `rand() % 2`.

One read escapes the cursor entirely: `0x801D7F94` reads `0x801F4E58`
directly - row 18, column 0 - so Gala's `0x1D` arm folds a fixed column in its
`cursor == 0` branch rather than the one the coin flip picked.

Retail indexes with a **fixed** displacement off a cursor-shifted base, so an
out-of-range cursor would silently read the next row's first halfword. That
cannot happen in the dumped arms; the parser refuses it rather than reproducing
the over-read.

## Extent

Two independent measurements agree on twenty rows, and neither is a guess about
where the data "looks like it stops":

- **Which rows the code reads.** Sweeping every `0x801F4E10` base computation
  in `overlay_battle_action_801d71b8.txt` and collecting the `lhu`
  displacements off the pointer each one forms yields `0x00, 0x04, …, 0x4C` -
  dense, twenty rows, nothing above `0x4C`.
- **Where the values stop.** `0x801F4E10..0x801F4E5F` reads as camera offsets;
  the halfword pair at `0x801F4E60` is `(0, 0)` and no arm addresses it.

The neighbours pin it from both sides: the per-character camera-height table is
at `0x801F4D2C` ([`battle-camera-table`](#see-also)) with its trailing pointer
list above, and the move-power table is at `0x801F4F5C`
([`move-power.md`](move-power.md)) below.

## Row map

Each arm picks its own rows *and* its own destination, so the table is not a
column-typed record - row 3 is an eye-space depth wherever it appears, but only
because the three arms that read it happen to agree. The map below is taken
site by site from the disassembly; the pose components are the arm's stack
triple (`sp+0x10` rotation, `sp+0x18` translation, `sp+0x20` look-at).

| Row | Folded into | Read by (arm entry) |
|---|---|---|
| 0 | pitch | `0x801D7308`, `0x801D76EC`, `0x801D7B4C` |
| 1 | yaw | `0x801D7308` |
| 2 | yaw | `0x801D7308`, `0x801D76EC`, `0x801D7B4C` |
| 3 | eye-space Z | `0x801D7308`, `0x801D76EC`, `0x801D7B4C` |
| 4 | yaw | `0x801D7B4C` |
| 5 | yaw | `0x801D74A8` |
| 6 | yaw | `0x801D7568` |
| 7 | yaw | `0x801D7650` |
| 8 | yaw | `0x801D76EC`, `0x801D7870` |
| 9 | yaw (**subtracted**) | `0x801D78F0` |
| 10 | yaw | `0x801D797C` |
| 11 | yaw | `0x801D79F8` |
| 12 | yaw | `0x801D79F8` |
| 13 | yaw | `0x801D76EC` |
| 14 | yaw | `0x801D76EC` |
| 15 | eye-space X | `0x801D79F8` |
| 16 | eye-space X | `0x801D79F8` |
| 17 | yaw | `0x801D7B4C` |
| 18 | yaw | `0x801D7EA0` |
| 19 | eye-space Z (**subtracted**) | `0x801D7B4C` |

Two folds subtract rather than add: row 19 at `0x801D7B84` (`subu v1,v1,v0`
at `0x801D7B90`) and row 9 at `0x801D7934`, where the row and the `ctx[+0x87C]`
accumulator are summed first and the **sum** is subtracted from the yaw
(`subu v0,v0,v1` at `0x801D794C`). Two dispatched arms - `0x801D7D7C` and
`0x801D81FC` - read no row at all and are built from literals and multiples of
the `ctx[+0x26E]` ramp.

## Who reads it: three jump tables, not one

`FUN_801D71B8` dispatches on the character id `DAT_8007BD10[ctx[+0x13]]` and
then on `actor[+0x1DB] - 0x1A`, through a table that is **per character**.

`actor[+0x1DB]` is the **latched battle-animation id**, not a Tactical-Arts
`ActionConstant`: `FUN_8004AD80` copies the staged id `actor[+0x1DA]` into it
each animation tick (`0x8004AEB0..0x8004AEB8`), before an id `>= 0x10` is
rewritten to the dynamic art-bank slot it materialises into. So the arm that
runs is chosen by the clip that is playing, and the band `0x1A..=0x2D` is
art-bank records `0x0A..=0x1D`.

The character id is the per-slot active-member id (`1` Vahn, `2` Noa, `3`
Gala - see [`character-mesh.md`](character-mesh.md#battle-form---assembled-from-the-player-files)),
and the table's file offset is its VA less the overlay base `0x801CE818`.

| Character id | Bound | Jump table | File offset | Live arms |
|---|---|---|---|---|
| `1` Vahn | `0x11` | `0x801CEA88` | `0x0270` | `0x1A`, `0x1C`, `0x1D`, `0x1E`/`0x2A` |
| `2` Noa | `0x14` | `0x801CEAD0` | `0x02B8` | `0x1A`, `0x1D`/`0x2C`, `0x1E`/`0x2D`, `0x1F`, `0x20` |
| `3` Gala | `0x11` | `0x801CEB20` | `0x0308` | `0x1A`, `0x1C`, `0x1D`, `0x1E`/`0x2A` |

Character `2` therefore accepts three art ids the other two reject, and most
slots in every table are the shared epilogue `0x801D828C`. Thirteen distinct
arm bodies exist. The earlier "seventeen per-art arms" reading generalised
character `1`'s table to all three; the bounds `sltiu v0,v1,0x11` /
`0x14` / `0x11` at `0x801D72E0` / `0x801D76C4` / `0x801D7B24` settle it.

### There is no spare arm

The thirteen arm bodies and the seventeen live slots cover each other exactly:
every arm is some slot's target, and **no arm is reached from two different
characters' tables**. Four arms take a second slot inside their own
character's table - `0x801D74A8` from `0x1E` and `0x2A`, `0x801D78F0` from
`0x1D` and `0x2C`, `0x801D797C` from `0x1E` and `0x2D`, `0x801D81FC` from
`0x1E` and `0x2A`. The other nine are single-slot, and the remaining 37 slots
hold the epilogue.

So nothing in the overlay is an unclaimed arm. Pointing a slot at a different
arm necessarily aliases one that another art still dispatches to, and any edit
to that arm - a moved `slti` threshold, a changed row fold - follows the alias
into that other art. **Exchanging** two slots' targets keeps the live set and
every arm's slot count unchanged; retargeting one does not.

## The cursor cascade: each arm's thresholds are clip-sized literals

Nine of the thirteen arms load the animation cursor `actor[+0x22C][+0x68]`
and run a cascade of `slti` tests on it - the arm's first act in eight of
them; `0x801D76EC` first branches on `actor[+0x21B] == 2` to a framing that
reads no cursor at all, and reaches its own cascade on the other path. Each
band
the cascade selects writes a different set of literals, ramp folds and table
rows into the pose triple, so **the cascade is how one swing gets several
shots instead of one framing**. The thresholds are immediates in the
instruction stream, not data anywhere:

| Arm | Char | Art constants | Cursor thresholds | Framings |
|---|---|---|---|---|
| `0x801D7308` | 1 | `0x1A` | `0x61` | 2 |
| `0x801D74A8` | 1 | `0x1E`, `0x2A` | `0xE0` | 2 |
| `0x801D7568` | 1 | `0x1D` | `0xF0` | 2 |
| `0x801D7650` | 1 | `0x1C` | none | 1 |
| `0x801D76EC` | 2 | `0x1A` | `0x90` | 2 |
| `0x801D7870` | 2 | `0x20` | none | 1 |
| `0x801D78F0` | 2 | `0x1D`, `0x2C` | none | 1 |
| `0x801D797C` | 2 | `0x1E`, `0x2D` | none | 1 |
| `0x801D79F8` | 2 | `0x1F` | `0xE0` | 2 |
| `0x801D7B4C` | 3 | `0x1A` | `0x70`, `0xA0` | 3 |
| `0x801D7D7C` | 3 | `0x1C` | `0x40`, `0x70`, `0xA0` | 4 |
| `0x801D7EA0` | 3 | `0x1D` | `0xB0`, `0x110` | 3 |
| `0x801D81FC` | 3 | `0x1E`, `0x2A` | `0xC0` | 2 |

Thresholds are in the cursor's own units - **sixteenths of a keyframe** - so
`0xE0` is keyframe 14 and `0x110` keyframe 17. `0x61` is the one that is not a
whole keyframe (`0x61 >> 4` = 6, remainder 1); its arm immediately re-uses
`cursor − 0x60` as a ramp, clamped to `0x100` by the `sltiu` at `0x801D737C`,
which is a magnitude clamp rather than a second band.

Named by the arts-table index the constant carries (`constant − 0x1B`):
`0x801D7D7C` is Gala's Explosive Fist, changing shot three times across
keyframes 4 / 7 / 10 - the most band-rich arm in the dispatcher; `0x801D79F8`
is Noa's Vulture Blade and `0x801D74A8` Vahn's Tornado Flame, both switching
once at keyframe 14; `0x801D7650` is Vahn's Burning Flare, one of the four
arms that never reads the cursor at all.

**A threshold is sized for the clip the art plays in retail, and nothing
rescales it.** An arm reads no frame count and has no other notion of where it
is in the swing, so against a longer clip the whole cascade completes inside
the opening and the last framing then holds for the remainder; against a
shorter one the later bands are never reached. The highest threshold in the
dispatcher is `0x110`, keyframe 17, so **every arm has spent its whole
choreography by keyframe 17 of whatever clip is playing** - which is a
proportion of the swing only for clips of about the length the literals were
cut against. The four cursor-blind arms are the flat case: one framing for the
entire clip, immune to its length and with no choreography to re-time.

Every row of the table is read site by site out of
`ghidra/scripts/funcs/overlay_battle_action_801d71b8.txt`; the art constants
come from the three jump tables at PROT 0898 file `0x0270` / `0x02B8` /
`0x0308`. The `slti` shape is what identifies a threshold - the dispatcher's
own bounds checks are `sltiu`, a different opcode, and the arms are branch
cascades, so a straight-line liveness read of the cursor register calls it
dead on the very paths that jump over its reuse.

## The two ramp counters the arms fold beside the rows

Each arm also adds its own literals and multiples of two battle-context
counters, per band of the [cursor cascade](#the-cursor-cascade-each-arms-thresholds-are-clip-sized-literals)
above:

- `ctx[+0x26E]` - a `0..=0xC8` ramp advanced by `8 * frame_step` and clamped in
  `FUN_801D5854`'s prologue (`0x801D58F8..0x801D5960`), which runs on **every**
  call to that function.
- `ctx[+0x87C]` - a 32-bit accumulator taking the same increment, read either
  whole (`lw` + shift) or truncated (`lhu`). It does not saturate.

Seven arms re-zero one or both through the latch `ctx[+0x26F]` when the swing
crosses an animation-frame threshold, so the reset fires once per crossing. In
every such arm the ramp is read **after** the reset, so the crossing frame
contributes nothing.

Both counters and the latch are ported
(`legaia-engine-vm::battle_attack_camera::AttackCamCtx`), so the arms carry
their literals and ramps rather than only their table folds.

`actor[+0x22C][+0x68]` is the animation cursor in **sixteenths of a keyframe**:
`FUN_80047430` clamps it against `clip[+0x85] << 4` and `clip[+0x86] << 4`
(`0x800477D4` / `0x8004781C`), so a threshold of `0xB0` is keyframe 11.

## What has to be true for an arm to run at all

The arms are keyed on `actor[+0x1DB]`, the **latched** staged anim id, and that
byte only enters the `0x1A..=0x2D` band when the action-parameter stream the
attack band walks carries an art action constant. The chain is short and every
link is load-bearing:

1. the queue builder inserts `art_id + 0x1B` into `actor[+0x1DF..]`
   (`FUN_801EED1C` at `0x801EF7A0`);
2. the strike loop stages that byte into `actor[+0x1DA]`
   (`FUN_801E295C` case `0x1E` at `0x801E3764`);
3. the anim commit copies it verbatim to `actor[+0x1DB]`
   (`FUN_8004AD80` at `0x8004AEB0`/`0x8004AEB8` - unconditional, every path
   converges there);
4. `FUN_801D71B8`'s outer gate passes (real target slot, category `3`, party
   seat) and `character_arm` resolves the participant id.

Steps 1-3 are documented in
[battle-action.md § A Tactical Art is an ordinary attack-band
action](../subsystems/battle-action.md#a-tactical-art-is-an-ordinary-attack-band-action).

**An action whose stream holds only direction swings `0x0C..0x0F` therefore
reaches no arm, and that is retail behaviour, not a defect.** A plain physical
Attack is exactly that action: the port's `basic_attack_queue`
(`FUN_801EED1C`'s no-directional-input arm) writes two `0x0C`/`0x0D` swings and
nothing else, so `art_arm` answering `None` for it is correct. The band the
table serves is reached by an **arts chain**, and until the port routed its
Arts path through the attack band
(`engine-core`'s `World::run_battle_art`) nothing in either host ever put a
constant in that range - which is why the whole per-art channel measured as
dead code while being correctly ported.

`0x1B` is the one art constant with no arm in any of the three tables (index
`0x1B - 0x1A = 1` is `None` for characters 1, 2 and 3), so a turn whose art is
`Art1B` still frames from the case-6 action pose. That too is retail.

## See also

- [`move-power.md`](move-power.md) - the sibling PROT 0898 table, and the
  worked example of how an overlay table's file offset is pinned.
- [`battle.md`](../subsystems/battle.md) - the battle camera the arms feed.
- [`battle-action.md`](../subsystems/battle-action.md#a-tactical-art-is-an-ordinary-attack-band-action) -
  how an art constant gets into the stream in the first place.
