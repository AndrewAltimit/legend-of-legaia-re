# Battle action queue and Tactical Arts

## A Tactical Art is an ordinary attack-band action

There is no Arts *band*. A recognised art executes through the same category-3
attack band a plain physical swing does; the only thing that makes it an art is
the byte the strike loop stages. Three separate functions carry the chain, and
none of them is the one people reach for first.

### 1. The queue builder writes the art constant into the stream

`FUN_801EED1C` writes direction commands into `actor[+0x1DF..]` as
`0x0B + dir_code`, i.e. `0x0C..0x0F` (`0x801EEDDC`, `0x801EEFF8`), and the
combo matcher compares `stream[i] - 0x0B` against the art record's command
bytes (`0x801EF3E8`: `addiu v1,v1,-0xb`; `0x801EF3EC`: `bne v1,v0,…`).

On a match, the commit at `0x801EF6E8..0x801EF7A0` does **not** collapse the
combo into a single constant:

```text
801ef6f0  addiu v1,t3,0x18       ; t3 = 1 (known) / 2 (newly learned)
801ef6f8  sb    v1,0x1df(v0)     ; stream[last_matched_dir] = 0x19 or 0x1A
801ef714  ...                    ; shift stream[last+1 .. 0x0E] right by one
801ef794  addiu v0,s3,0x10       ; s3 = art record index + 0x0B
801ef7a0  _sb   v0,0x1df(v1)     ; stream[last+1] = art_id + 0x1B
```

So only the **last** direction of the match is overwritten - by the starter
marker `0x19` (art already known) or `0x1A` (art learned on this use) - and the
art constant `art_id + 0x1B` is *inserted* after it, shifting the tail right by
one. Every direction before it stays in the stream and still executes as its own
swing.

The port builds the same stream. `World::build_arts_action_queue`
(`engine-core`, `world/battle/command_flow.rs`) runs the entered arrows through
`legaia_art::tokenize` (leading arrows kept, the starter over the last matched
arrow, the constant inserted after it), the learn-on-use verdict per accepted
art (`0x1A` for a newly learned one), and the Miracle / MSB-clear / Super finish
(`legaia_engine_vm::battle_action::finish_action_queue`), so a three-direction
art is two swings plus the art on both sides. The older reading - the port's
entry resolver folding the leading directions into the art - is what
[What the port does](#what-the-port-does) replaced.

A second commit path exists at `0x801EF5BC..0x801EF644`, gated on
`ctx[+0x25F + slot] != 0`, which writes `0x1A` at the *start* of the match and
then walks the art record's `+0x0A..+0x0D` entries (`0x801EF620`:
`sltiu v0,a2,0x4`) writing `(art_index + k) + 0x11`; and a third,
whole-window overwrite at `0x801EF4F0..0x801EF524` sourcing
`0x801F64F4 + (char - 1) * 0x10` for the AI / auto-fight case.

### 2. The strike loop stages one byte per swing and applies no damage

The `0x1E` body's stage site is `0x801E3734..0x801E3764`:

```text
801e3734  lbu v0,0x4(s5)         ; cursor  (s5 = _DAT_8007BD24 + 0x11)
801e373c  addiu v1,v0,0x1
801e3748  sb  v1,0x4(s5)         ; cursor += 1   -- post-increment, exactly 1
801e374c  lbu v1,0x1df(v0)       ; b = stream[old cursor]
801e375c  sb  v0,0x1dc(s3)       ; actor[+0x1DC] |= 2   (one-per-clip latch)
801e3764  sb  v1,0x1da(s3)       ; actor[+0x1DA] = b    (the stage)
```

Two corrections fall out. The cursor is a **battle-controller** byte
(`_DAT_8007BD24[+0x15]`, reached as `0x4(s5)`), not an actor byte - the port
models it per actor, which is equivalent only because one actor acts at a time.
And the terminator is tested at the **new** cursor
(`0x801E3998..0x801E39AC`), with `0x00` routing to state `0x1F`
(`0x801E3A7C`).

**`FUN_801E295C` never calls a damage kernel.** `jal 0x801ec3e4` does not
appear anywhere in its 4099 instructions; case `0x1E`'s only calls are
`FUN_801D8DE8`, `FUN_801EED1C`, `FUN_801D5854` and the atan2 `FUN_80019B28`.
The `0x19` refill loop at `0x801E3A20..0x801E3A64` is the **War God Icon's
Attack x2 second pass**, not a Miracle continuation. Its guard chain settles
it: the acting slot must be a party one (`sltiu v0,v0,0x3` on `0x2(s5)` =
`ctx[+0x13]` at `0x801E39BC`), the acting character's record `+0xF4` must carry
bit `0x2000` (`0x801E39FC..0x801E3A08`) and `0x5(s5)` = `ctx[+0x16]` must still
read zero (`0x801E3A18`). It then rewinds the strike cursor
(`sb zero,0x4(s5)` = `ctx[+0x15]`), bumps `ctx[+0x16]`, and rewrites to `0x19`
every queue slot whose mark in the builder's side array `0x801F6990` reads
**exactly `1`** - so the whole action stream replays once with the
newly-learned starters demoted, and the learn verdict fires on the first pass
only. `ctx[+0x16]` is the counter the damage kernel's carry arm reads
(`s2 = 0xFF` while it is `< 2`).

The compare is `bne v0,a1,0x801E3A58` with `a1 = 1`, and the exactness is
load-bearing. The build loop writes `1` at each art it accepts
(`0x801EF788`); the Super tail-replace `FUN_801EF9E4` writes `4` at each
`0x1A` it stamps (`0x801EFBA8`), *after* the reorder. So a Super Art's starter
is the one starter the second pass leaves alone, and the War God Icon's extra
pass performs the Super again rather than a plain swing. A port that
reconstructs the marks from the finished queue bytes cannot tell the two apart
- both starters read `0x1A`.

Port: `legaia_engine_vm::battle_action`'s `attack_chain` (`attack_x2_refill`),
with the counter on `BattleActionCtx::attack_x2_pass` and the marks carried
from the builder on `BattleActor::starter_marks`
(`BUILD_STARTER_MARK` / `SUPER_STARTER_MARK`).

#### The per-frame drift and the `0x801F696C` flag

Both the stage path and the in-flight hold (`bne v1,zero,0x801E37C0` at
`0x801E3718`) fall into one block at `0x801E37C0` that runs on every frame of
the loop for a party actor (`ctx[+0x13] < 3`). It moves two actors a little
along their facings: the acting actor's live pair by
`trig(facing) * -3 * frame_dt * rate >> 15` and the target's by
`trig(target facing) * +3 * frame_dt * rate >> 15`, both scaled by the
**acting** actor's rate byte `+0x21D` (`0x801E386C..0x801E3994`; `frame_dt`
is the scratchpad byte `0x1F800393`). Two arms gate it on the committed
clip's header byte `ctx[+0x243]`:

- `ctx[+0x243] == 0`: the character record's `+0xF4` (`+0x6BC` off the
  record base) carries the War God Icon bit `0x2000`;
- `ctx[+0x243] != 0`: the global `0x801F696C` is non-zero (`lw` at
  `0x801E3840`, the flag's one reader) and the latched clip id `+0x1DB` is
  outside `0x10..=0x1A`.

`0x801F696C` is the queue builder's special-trigger flag. `FUN_801EED1C`
clears it at its head (`sw zero,0x696c` at `0x801EED88`) and three sites set
it to `1`: the Miracle arm (`0x801EF5B8`), the Super tail match
(`0x801EFBD4`) and the auto-combo assembler `FUN_801F0450` (`0x801F0518`).
The last store heads that routine's **auto-fill** arm - it is the first
instruction past the `+0x16E & 0x404` veto - not its art insertion tail, so the
flag goes up for every party slot that arm takes, and a delegated member never
runs `FUN_801EED1C` to clear it again.

Port: `swing_drift_armed` / `swing_drift` in `attack_chain`, with the flag on
`BattleActionCtx::super_trigger` (set by
`finish_action_queue_with_trigger`; the basic-attack build clears it; the
auto-fill arm in `battle_action::dispatch` raises it).

#### The War God Icon's per-stage bump

The stage site's tail (`0x801E3768..0x801E37BC`) re-reads the acting
character's record `+0xF4` and, when it carries `0x2000`, increments
`ctx[+0x16]` - but only while the counter is already non-zero
(`beq v0,zero,0x801E37C0` at `0x801E37B4`). So the first pass stages with the
counter at `0`, the end-of-stream refill lifts it to `1` while the first
pass's last clip is still in flight (its hits read `1` and still carry), and
the second pass's first stage lifts it to `2` - the value that ends the damage
kernel's carry arm (`s2 = 0xFF` only while `ctx[+0x16] < 2`). Each further
second-pass stage bumps it once more. The refill's own `== 0` guard keeps the
pair at exactly two passes. Port: `attack_x2_stage_bump` in `attack_chain`.

### 3. Damage is one power byte per animation hit event

`FUN_801EC3E4` is called from the **anim** tick `FUN_80047430`
(`0x800478A0`, `0x80047BF0`) with the hit-event frame in `a2`, not from the
state machine. Each call consumes exactly one power byte, indexed by the
actor's own counter `+0x1F4`:

```text
801ec45c  lbu v1,0x1f4(v0)       ; strike index
801ec464  addu a1,a1,v1
801ec480  _sltiu v0,v1,0x4       ; BOUND, not a loop back edge
801ec494  lbu a0,0x0(a1)         ; the single power byte
...
801eecdc  lbu v0,0x1f4(v1)
801eece8  sb  v0,0x1f4(v1)       ; +0x1F4 += 1, once, in the epilogue
```

So **one staged art constant produces as many damage applications as its clip
has hit events**, capped at four - the art's power list is walked by the
animation, not by the stream. Every commit zeroes `+0x1F4` (`FUN_8004AD80`,
`0x8004B064`), and the tick's loop-window arm zeroes it again on every rewind
(`0x80047840..0x80047878`: reached only from the `+0x176` window test, for a
party slot whose committed id is `0x11` and whose latched id is `>= 0x2B`), so
a Hyper / Super clip that replays its window re-fires its hits each cycle.

**Which hit lands the total.** Every admitted hit adds its damage to the
target's combo word `+0x0` and its HP-bar word `+0x10` (`0x801EDB40` /
`0x801EDB58`); live HP `+0x14C` is written by one arm, `0x801EEA10..0x801EEA3C`
(`hp - total`, floored at zero, then `sw zero,0x0` at `0x801EEA74`), selected
by a mode register the kernel computes after the roll (`s2`, the two copies at
`0x801EDEE4..0x801EE130` and `0x801EE790..0x801EE980` are the party / monster
attacker branches):

- `s2 = 0` - the ordinary case: apply only when the strike cursor is parked
  (`ctx[+0x15] == 0xFF`, `0x801EE9A4`) **and** this is the clip's last listed
  beat (`entry[0x11 + idx] == 0 || idx == 3`, `0x801EE9DC..0x801EE9EC`).
- `s2 != 0` - a look-ahead over every remaining hit of the action (the rest of
  this entry's power run, then every remaining stream byte's entry) found none
  whose class bits can connect with the target's `+0x1E` size class
  (`0x801EE060..0x801EE0B4`, the limb-vs-height "Miss" law): the total lands
  **now**, since nothing after this hit can add to it. The class bits are
  `0x1` for a power byte in `0x01..=0x10`, `0x2` for one in `0x11..=0x15`, and
  both (ending the scan) for `>= 0x16`; a class-`2` target needs bit `0x1`
  present and a class-`3` target bit `0x2`. In the stream half of the walk a
  byte `>= 0x10` - an art starter or art constant - sets both bits and ends
  the scan, while a direction swing has its entry's whole power run folded.
- `s2 = 0xFF` - the attacker's ability bitfield (`char +0xF4`) carries bit
  `0x0D` (the War God Icon's *Attack x2*) and `ctx[+0x16] < 2`
  (`0x801EE0C0..0x801EE120`): the first action of the pair never applies, its
  total carries into the second.

Both copies of the kernel gate the whole of this on a **monster** target
(`sltiu` on the target slot against `3` at `0x801EDEB8` and `0x801EE724`, each
branching past the look-ahead *and* past the War God arm), which is what makes
the record-direct `0x801C9348[target - 3]` read in the decision well-defined: a
party target is always the `s2 = 0` arm.

A swing entry's power byte 0 is **equipment-spliced**, not the command: the
swing clips are per-item (`swing_battle_animations`), and the same `0x0E` reads
`0x13` on one save and `0x1D` on another (N = 2 captures).

### 4. The latch is what makes the attack camera reachable

`FUN_8004AD80` ends with an unconditional byte copy
(`0x8004AEB0`: `lbu v0,0x1da(s1)`; `0x8004AEB8`: `sb v0,0x1db(s1)`) that every
path converges on, so an art constant staged into `+0x1DA` reaches `+0x1DB`
unchanged. `+0x1DB` is the byte the per-art attack camera dispatches on
(`0x1A..=0x2D`, see
[battle-attack-camera-table.md](../formats/battle-attack-camera-table.md)),
which is why the camera is unreachable for any action whose stream carries only
direction swings.

### What the port does

The same three things, in the same seats:

- **The queue is byte-exact.** `World::build_arts_action_queue` tokenizes the
  entered arrows (§1), runs the learn-on-use verdict per accepted art and the
  Miracle / MSB-clear / Super finish, and `arm_battle_art_action` copies the
  window verbatim into the actor's action-parameter stream under category `3`.
  A saved-chain row is armed from its directional string through the same
  builder, so there is one arts path.
- **The strike loop only stages.** `attack_chain` (`engine-vm`,
  `battle_action/attack.rs`) stages one byte per clip behind the `+0x1DC`
  bit-1 latch, tests the terminator at the new cursor, and never touches HP;
  `attack_recovery` waits for the last clip's commit, stages idle over it and
  parks the cursor at `0xFF` (`STRIKE_CURSOR_PARKED`).
- **Damage is the anim tick's.** `World::tick_battle_hit_events`
  (`world/battle/loop_driver/hits.rs`) is the engine seat of the per-frame
  `FUN_801EC3E4` call. For every actor whose committed clip is in flight it
  runs the kernel's head guard chain
  (`legaia_engine_vm::battle_action::hit_event_admits`: `ctx[7] != 0x5A`,
  `entry[0]` in `0x0C..=0x1F`, `+0x1F4 < 4`, `entry[0x10 + idx] != 0`,
  `frame + 1 >= entry[0x10 + idx]`) against the playing clip's own head bytes
  (`MonsterAnimPlayer::hit_source`), resolves an admitted hit with the entry's
  power byte at that index (`land_melee_hit`: the weapon fold, the melee roll,
  the accumulate into the target's combo word `+0x0` and its HP bar), bumps
  `+0x1F4`, and lands the accumulated total on live HP **once** - on the hit
  that is its clip's last listed beat while the cursor is parked
  (`apply_combo_total`, retail's `0x801EE9A4..0x801EEA78` arm). The same pass
  runs the tick's event-path commit (`event_commit_due`:
  `entry[0x10] + 2 < frame` with `entry[+0x76] == 0`), which is what chains
  one swing into the next mid-clip. Each resolved hit is surfaced as a
  `BattleHitEvent` (`engine-core::battle_events`) carrying its index, power
  byte, damage and running total for the impact-FX and HIT / TOTAL layers.

A direction swing's entry carries its own power byte at `+0x00` (Vahn's high
swing reads `0x18`, his low swing `0x1D`) and one beat, so a swing is exactly
one hit resolved from the clip, not from the command; an art record's embedded
entry carries up to four. The art record is consulted only for what the entry
does not carry - the status effect and the per-hit sound cue - and the art is
identified from the latched staged id (`staged_art_constant`), party slots
only, so a monster's `0x1B+` clip indices never read as art constants.

Two fallbacks keep clip-less hosts and the synthetic catalog playable, both
disclosed at the code: a staged byte whose clip has no entry head resolves its
hits at stage time (`resolve_zero_length_clip_hits`), and a monster whose
catalog carries no attack entries keeps the AGL-budget immediate swings
(`apply_basic_attack`, still accumulate-then-apply). Neither is reachable with
disc data, where every entry carries its head.

All three arms of the apply-mode law in §3 are wired, beside the loop-window
re-zero of `+0x1F4` (`MonsterAnimPlayer::take_loop_rewound`, read by
`tick_battle_hit_events` under the same party / slot `0x11` / latched
`>= 0x2B` gate). `World::hit_apply_mode` runs the look-ahead and the mode
decision on every admitted hit
(`legaia_engine_vm::battle_action::remaining_hit_class_bits` +
`apply_mode`), and `resolve_hit_event` routes on the result: `APPLY_MODE_EARLY`
lands the total on this hit, `APPLY_MODE_CARRY` lands nothing, anything else
keeps the cursor-parked / last-beat pair.

The size-class byte the early arm needs is now carried:
`legaia_asset::monster_archive::MonsterRecord::swing_class` parses record
`+0x1E` and `MonsterDef::swing_class` projects it into the catalog, which is
also what fills the no-input attack queue's own class input
(`World::attack_swing_class_of`). A synthetic catalog leaves it `0` - the class
that connects with everything - so a disc-free session behaves exactly as it
did. The `ctx[+0x16]` pair counter the carry arm reads is written by the strike
loop's own Attack x2 refill and by the stage site's per-stage bump, both
described in §2.

**What the builder tokenizes against.** The records retail's inner loop walks
are the character's art-animation bank records (`record[0] +0x58`,
[battle-data-pack.md](../formats/battle-data-pack.md#art-animation-bank-record0-0x58)):
record `k` is constant `0x10 + k`, its `+0x00` combo is the arrow string
compared against the queue (`0x801EF3BC..0x801EF3EC`). Both hosts install
those records at battle entry, next to the bank's clips
(`World::install_art_bank_records`), so the live arts input matches the disc's
arts. Two records never rewrite the queue and are left out of the port's
catalog: the Miracle Art and the three Hyper Arts (ordinals `0..=3`,
constants `0x1B..0x1E`) take the loop's other arm (`sltiu a1,a0,0x4` at
`0x801EF330`), which writes nothing while the slot's `+0x25F` marker is clear;
and a fully matched **one-arrow** combo takes the `s1 == 1` exit
(`0x801EF420..0x801EF434`) with no rewrite - on the disc that is the Miracle
finisher's record, and admitting it would steal an arrow from every art that
contains one.

Still divergent after this, each with its prerequisite:

- **`0x20` is left at once.** Retail's return state holds while the target's
  committed anim is not idle / `8` and the actor's node `+0x74` still counts
  (`FUN_801E295C` state `0x20`), so the last clip finishes and the target
  settles - back to idle, or onto entry `8`, the downed party member's
  kneel the clip-tag ladder ends on - before `0x50`. The port transitions
  immediately, so the last clip's hit lands under `0x51` (still with the
  cursor parked, so the total applies). Prerequisite: a host query
  for "clip in flight" on the `BattleActionHost` trait (every host impl).
- **Empty-event art clips deal nothing through the driver.** The Miracle
  entry (Vahn's Craze, `0x1B`: 50 frames, `+0x10..+0x13 = 0`) has no hit
  events, so its damage path - the effect script or a chained clip - is
  unpinned; the typed path never reaches it (above), the Miracle replacement
  does.
- **The starter's rate.** The `0x1A` SpecialStarter arms `ctx[+0x243]`
  through its solo byte and the art-constant commit arm halves every actor's
  `+0x21D` while it is set (`0x8004BB78..0x8004BBB0`); the SM clears it at
  `0x38` and `0x50` (`0x801E4E94`, `0x801E5234`) and arms it itself at `0x3C`.
  The port mirrors the arm through `gauge_rearm_latch` and the Done clear;
  the two SM writes are not modelled.

## Action validator (`FUN_8003FB10`)

The 18-arm gate (`0x00..=0x0D` plus `0x80..=0x83`) the menu / battle UI runs against a candidate
slot before committing the player's action. Selects which validation rule fires from the outer
`param_1` arm (jump table at `0x80014D70`, bound `< 0x84`; unhandled slots return invalid) and,
for arm 6, a sub-case `param_2` through a second 7-entry table at `0x80014F80`. Reads HP / MP /
status / stat caps from the active record - the setup loop caches per-slot
`(hp, hp_max, mp, mp_max)` pointer quads from the battle-actor table `DAT_801C9370`
(`+0x14C/+0x14E/+0x150/+0x152`, 7 slots) when `_DAT_8007B83C == 0x15`, else from the character
records `0x80084708 + slot*0x414` (`+0x106/+0x104/+0x10A/+0x108`, 3 slots) - and writes a
per-slot validity bit at `gp + 0x9A8`. Source:
`ghidra/scripts/funcs/8003fb10.txt`.

Arms (ported wholesale as `legaia_engine_vm::battle_action::validate_action` over the
`ActionValidatorHost` trait, with the `gp + 0x9A8` byte modelled as an explicit `validity_bits`
parameter; the target-relevance arms are additionally re-implemented where they are consumed -
liveness/kind gating in `legaia-engine-core`'s `target_picker`, item-benefit arms in
`inventory_use::effect_benefits_target`):

| arm | meaning |
|---|---|
| `0x00` | Alive AND `hp < hp_max` (heal target). |
| `0x01` | Walk party - set bit per slot that's alive-and-not-full. |
| `0x02` | Alive AND `mp < mp_max` (restore-MP target). |
| `0x03` | Status-flag presence. Battle: `actor[+0x16E] != 0`, returned **without touching the validity byte**; field: record `+0x12E != 0` with the usual clear-then-set bit write. |
| `0x04` | Dead target (Revive item validator). |
| `0x05` | Alive (any-action target). |
| `0x06` | Stat-cap walker - alive slot AND sub-case-picked effective record stat(s) still below cap (strict `<`; character records regardless of mode): 0 = HP max (`+0x104` < 9999), 1 = ATK (`+0x112` < 999), 2 = UDF/LDF pair (`+0x114`/`+0x116` < 999), 3 = SPD (`+0x118` < 999), 4 = INT (`+0x11A` < 999), 5 = MP max (`+0x108` < 999), 6 = all of those plus AGL (`+0x110` < `0x118`). Sub-case ≥ 7 is invalid. |
| `0x07` | Alive (synonym of arm 5; separate code path with no upper bound). |
| `0x08` | Alive AND `(status & 3) != 0` ("can apply paralysis / sleep"). Battle branch skips the validity-byte write; the field branch reads the record status word **signed** and tests `& 0xFFFF0003`, so a status word with bit 15 set validates even with bits 0-1 clear (sign-extension quirk, kept by the port). |
| `0x09` / `0x0A` | Always valid; force the bitmask to the literal `0x07`. |
| `0x0B` / `0x0C` / `0x0D` | Per-slot exact match; only valid when `slot == arm - 0x0B`. |
| `0x80` | Out-of-battle; story flag `0x100000` clear AND system flag 5 clear. |
| `0x81` | Out-of-battle; story flag `0x200000` clear AND system flag 6 clear. |
| `0x82` | Out-of-battle; calls the external item-count validator (`FUN_80046898`). |
| `0x83` | Always valid. |

The retail dispatcher writes a per-slot validity bit at `gp + 0x9A8` with per-arm discipline:
most arms clear their slot's bit before testing and set it on success, arm `0x01` zeroes the
whole byte before its walk, arms `0x09`/`0x0A` force the byte to `7` and `0x0B..=0x0D` overwrite
it with the matched slot's mask, while the battle branches of `0x03`/`0x08` and all of
`0x80..=0x83` never touch it. The engine port (`validate_action`) keeps that discipline via its
`validity_bits` parameter; the consuming menu paths (`target_picker` for battle-target cursors,
`inventory_use` for item-menu greying) additionally surface the same signal where it is read.
The dump's only real callees are the system-flag test `FUN_8003CE64` (arms `0x80`/`0x81`, flags
5/6 in the `DAT_80085758` bank, alongside `_DAT_1F800394` bits `0x100000`/`0x200000`) and the
arm-`0x82` gate `FUN_80046898` - a 3-instruction leaf returning
`*(int *)(gp + 0x2E8) < 0xE0` (signed compare; `see ghidra/scripts/funcs/80046898.txt`),
ported as `battle_action::item_count_gate` over `ActionValidatorHost::inventory_count`. The
validator does **not** call the ability bit-test `FUN_800431D0` (an earlier attribution in
[`battle.md`](battle.md) / `reference/functions.md`).

The "inventory has room, against a 224-slot cap" reading of that compare is **falsified**, and
the two symbol names above preserve it only because three surfaces spell them. `gp` is
`0x8007B318` (`80026ca8` `lui gp,0x8008` + `80026cac` `addiu gp,gp,-0x4ce8`, cross-checked
against two known globals: the halfword camera pitch at `gp+0x478` = `_DAT_8007B790` and the
tile-board install pointer at `gp+0x138` = `_DAT_8007B450`). So `gp + 0x2E8` is
`_DAT_8007B600` - in the `0x8007Bxxx` overlay-scratch band, not the `0x80084xxx` save/game-state
window an inventory length lives in.

It is the **Incense window**. Its one writer, `FUN_80046870`
(`battle_helpers::top_up_cooldown`), is the whole of the applier's selector-`0x82` arm - class
`0x82` being Incense (item `0x8A`) - and tops it up by `0x40`, capped at `0x100`. The two overlay
sites that reach it by absolute address count it in field **walk-regen ticks**, not frames: the
walk tick `FUN_801D0B90` (PROT 0897, `0x801D0CD4..0x801D0CE8`) decrements it once per running tick
and, on the transition to zero, hands the field a "wore off" event (`_DAT_8007B450 = 0x801F2278`),
and the region encounter roll `FUN_801D9E1C` skips its whole roll while it is non-zero
(`0x801DA174`). So an Incense suppresses encounters outright for its window, and `0xE0` is the
threshold below which another may be used - which is why a host should return `0` ("no window
outstanding") and not plumb an inventory length in.

## Action queue and Tactical Arts trigger ordering

Before `FUN_801E295C` reaches the inner-state machinery, the battle code resolves the player's command-input sequence into a flat **action queue** of [`ActionConstant`](../formats/art-data.md#action-constants) bytes. The queue is built incrementally from directional inputs and accumulated arts; once the player commits, the runtime applies two trigger passes in order (retail: both inside the queue-builder `FUN_801EED1C` - see [the retail queue-builder](#the-retail-queue-builder-fun_801eed1c-and-super-applier-fun_801ef9e4)):

1. **Miracle Art match** - if the input command sequence equals the character's Miracle Art command string, the entire queue is replaced with the Miracle Art's replacement string (`L`/`R`/`D`/`U` × 4 → `SpecialStarter` → `art1, art2, ...`). The first 4 directional bytes carry the on-disc MSB-set quirk and are masked to `0x0C..=0x0F`.
2. **Super Art find/replace at tail** - for each chained art the runtime walks all the character's Super Art `find` patterns and replaces the matched tail with a `replace` tail ending in the Super Art's finisher action constant. Triggers require: the last art of `find` is the last action in the queue, and all participating arts paid AP.

Both passes are from-scratch ports in `legaia_art::MiracleMatcher` / `legaia_art::SuperMatcher`, applied together by `legaia_engine_vm::battle_action::resolve_action_queue`. The engine-vm `BattleActionHost` exposes an `art_record(char_id, art_id)` callback so the SM can fetch the [art record](../formats/art-data.md) for power-byte resolution, hit timing, and status-effect application during the `0x14..0x20` Attack chain.

### Miracle / Super in the live player-driven Arts submenu

Both matchers run against a flat **directional command string** with no connector bytes. The live path produces that string two ways: the retail per-press [Arts command input](battle-round-loop.md#arts-command-input) hands `World::build_arts_action_queue` the buffer the player typed, and the legacy saved-chain list (`legaia_engine_core::battle_arts`, behind `LEGAIA_ARTS_SAVED_LIST=1`) hands the same builder a stored `legaia_save::SavedChainRecord`'s directional string (`ArtRow::sequence`). Everything below applies to both - "the chain" is whichever string reached the matcher. Two trigger paths interact with that model differently:

- **Miracle Arts are wired.** A Miracle Art's trigger *is* an exact directional-string match (`MiracleMatcher::find`), so `battle_arts::miracle_for_chain` recognises a saved chain whose command string equals the caster's Miracle Art and flags the menu row (`ArtRow::miracle = Some(name)`). `World::build_battle_arts_rows` then resolves the row's per-strike profile from the Miracle's finisher-replacement queue via `resolve_action_queue`: each art constant in the replacement contributes its staged [`ArtRecord`](../formats/art-data.md) power bytes + status effect, or one tier-0 (`x12`) synthetic strike when that art's record isn't loaded (the same graceful-degradation fallback the no-disc-data path uses). The native `play-window` HUD shows the Miracle name on the row.
- **Super Arts are wired, with the queue connectors abstracted.** A Super fires when the player chains several named arts ending on a known combination. `SuperMatcher`'s `find` patterns match the **tail** of a queue with the *interleaved* shape `Starter Art <dir> Starter Art <dir> Starter Art` (e.g. Vahn's Tri-Somersault `find` = `19 27 0F 19 1F 0E 19 27` = `Starter Somersault Up Starter Cyclone Down Starter Somersault`; see [art-data.md](../formats/art-data.md#super-arts) § Super Arts). The live submenu reaches that match in two steps:
  1. **Recognize the named-art sequence.** `legaia_art::recognize_art_sequence` tokenizes a saved chain's flat directional `Command` string into the ordered named arts it performs, identifying each by its own `ArtRecord::commands` (greedy longest-match). `battle_arts::super_for_chain` runs this over the caster's loaded art catalog.
  2. **Tail-match the pinned art ordering.** `SuperMatcher::trigger_by_art_sequence` compares the recognized ordering against each Super's `SuperArt::art_sequence()` - the `find` pattern projected to its art constants only (`[0x27, 0x1F, 0x27]` for Tri-Somersault), with the `0x19` starters and the interleaved connector directions stripped. A tail match flags the menu row (`ArtRow::super_art = Some(name)`), and `World::build_battle_arts_rows` resolves the per-strike profile from the Super's finisher-replacement queue (`SuperArt::replace`) through the same `art_actions_strike_profile` helper the Miracle path uses. The `play-window` HUD shows the Super name on the row. Super is checked *after* Miracle, matching the retail "Miracle replacement runs before Super tail expansion" order.

  The match is deliberately **connector-abstracted**. The connector direction after each art is *combo-specific* - the same art appears with different connectors across Supers (Vahn's `0x27` is followed by `0F` in Tri-Somersault but `0E` in Power Slash) - because it is a **leftover of the physical input**, not something typed between arts: the retail tokenizer keeps a matched art's leading arrows and lets arts overlap, so the connector is whatever arrow the previous match left standing (`legaia_art::tokenize`; see [art-data.md](../formats/art-data.md#super-arts) - the byte-exact queue is derivable from the input, and every Super's input from its pattern).
  The live submenu matches the named-art ordering because a saved chain carries no connector bytes, not because the byte-exact strings are unknown.

  **The queue location is now pinned by capture:** it is the per-actor action-parameter byte stream at `actor[+0x1DF..+0x1F2]` - **not** `ctx[+0x274]`, which a capture showed is the turn-order active-actor index written by `recompute_battle_order` (`FUN_801DABA4`: `lbu v0,0x11(v1); sb v0,0x274`).
  Direction/connector bytes encode as `0x0C/0x0D/0x0E/0x0F` = Left/Right/Down/Up and `0x1A` = `SpecialStarter`; a Noa Miracle Art capture read that stream and it matched the engine's modeled replacement string byte-exact (probe `autorun_super_art_action_queue.lua`; runbook [`super-art-queue-capture.md`](../tooling/super-art-queue-capture.md)). A Vahn **Tri-Somersault** capture likewise confirmed the Super path: its resident queue tail `19 27 0F 19 1F 0E 1A 2B 2B 2B` is byte-identical to `super_art.rs`'s `Tri-Somersault` `replace`, validating the combo-specific connectors (`0x27 → 0F`, `0x1F → 0E`) and the finisher tail; the dequeue site is pc `0x801D89D8`.

  **All 15 Supers' `find`/`replace` strings are capture-validated.** The battle overlay keeps the whole trigger table resident; read out of live battle RAM (static-recomp endgame battle state, scene `jou ene`, mode `0x15`) it is:

  - `0x801F64F4` / `0x801F6504` / `0x801F6514` - the three Miracle-Art replacement strings ([art-data.md](../formats/art-data.md#miracle-arts)'s pinned trigger-entry VAs), leading `0x8C/0x8D/0x8E/0x8F` masked-direction bytes intact, byte-exact against `miracle.rs`;
  - `0x801F6524` - the 15 Super `find` entries, fixed 13-byte stride (`[len u8][bytes][zero pad]`), in `super_art.rs` table order (Vahn ×5, Noa ×5, Gala ×5);
  - `0x801F65E8` - the 15 Super `replace` strings, 16-byte stride, zero-padded, word-aligned, same order.

  Every resident string is byte-identical to `super_art.rs`'s modeled `find` / `replace` fields, and every resident replace preserves its find minus the final `[19, art]` pair then appends `[1A, finisher…]` - the pairing law locked by `super_art.rs`'s `replace_preserves_find_prefix_and_finisher_tail` test.
  So the byte-exact connector strings are no longer spreadsheet-only: the resident-table read validates the *strings* for all 15, and the *runtime queue effect* is live-executed for all 15 too - the in-the-wild Noa Miracle / Vahn Tri-Somersault captures above plus a per-Super applier-injection sweep
  (probe `autorun_super_art_queue_inject.lua`; each post-`FUN_801EF9E4` queue at `actor[+0x1DF]` is byte-identical to `super_art.rs`'s `replace`, re-checkable via the `super_queue_replace_*` library states + `crates/pcsxr/tests/super_art_queue_replace.rs`; see [`super-art-queue-capture.md`](../tooling/super-art-queue-capture.md#result---all-15-supers-live-executed-injection-probe)).
  The modeled tables feed the live path through `miracle_row_for` / `super_rows_for`, which project them into the resident row shapes; the queue arithmetic itself is the byte applier's, not `SuperMatcher`'s (see [the retail queue-builder](#the-retail-queue-builder-fun_801eed1c-and-super-applier-fun_801ef9e4) below).

### The retail queue-builder (`FUN_801EED1C`) and Super applier (`FUN_801EF9E4`)

The function that turns the player's committed directional chain into the final token stream at
`actor[+0x1DF..]` - emitting the art constants over the raw arrows and applying both trigger
passes against the resident tables above - is **`FUN_801EED1C`** in the battle overlay
(PROT 0898, file `+0x20504`; `see ghidra/scripts/funcs/overlay_battle_action_801eed1c.txt`).
The ActionSeed state `0x0C` of `FUN_801E295C` calls it for the acting party slot
(`jal 0x801EED1C` at `0x801E2C7C`, slot from `ctx[+0x274]`; a second site at `0x801E369C`
re-invokes it for the next queued actor of a multi-actor turn). The full retail chain:

1. **Preseed from the saved chain.** `FUN_801DA34C` (leaf, no frame; called from the round
   driver `FUN_801D0748` at `0x801D15C8`/`0x801D1734`) copies one of the character's two saved
   16-byte arts-input strings - char record `+0x76F` or `+0x77F` off `0x80084140 + (id-1)*0x414`
   (`lbu v0,0x76f(v1)` `0x801DA3F8`, `lbu v0,0x77f(v0)` `0x801DA4F8`) - byte-for-byte into
   `actor[+0x1DF..+0x1EE]` (`sb v0,0x1df(v1)` at `0x801DA404` / `0x801DA454` / `0x801DA504`),
   or zero-fills the queue when the selected slot is empty (`sb zero,0x1df` at
   `0x801DA490`/`0x801DA540`/`0x801DA584`). Slot pick + fallback are **asymmetric**: the u16
   pair `actor[+0x154]`/`[+0x156]` selects the leg (`sltu` at `0x801DA3A4`) - the
   `[+0x156] < [+0x154]` leg prefers the first string and falls back to the second when its
   head byte is zero, while the other leg reads only the second string and zero-fills on an
   empty head with **no** fallback (`beq` at `0x801DA4CC` lands on the `0x801DA51C` zero-fill,
   never on a `+0x76F` copy). The whole copy is gated on the stage byte `DAT_8007BD04`
   (zero → zero-fill, `0x801DA378`). Live pad edits during the Arts gauge then mutate the
   same bytes in place. Byte-level port: `legaia_engine_vm::battle_action::preseed_action_queue`.
   The **write-back twin** is `FUN_801DA59C`: after an arts action (category `+0x1DE == 3`,
   live actor), it copies `actor[+0x1DF..+0x1EF]` back into the char record's chain slot -
   the same `[+0x156] < [+0x154]` predicate picks `+0x76F` vs `+0x77F` (`sb` loops at
   `0x801DA638`/`0x801DA69C`), with no head-byte fallback: exactly one slot is overwritten.
   That is what the next preseed replays. Port:
   `legaia_engine_vm::battle_action::save_action_queue`.

   Both slots are record-relative `+0x1A7` / `+0x1B7`: the character record base
   `0x80084708 + slot*0x414` sits `0x5C8` bytes into the live-state window these
   two address off, so `+0x76F - 0x5C8 = +0x1A7`. `legaia_save` exposes them as
   `CharacterRecord::auto_command_string` over an `AutoCommandBand` the gauge
   pair selects, and `engine-core` drives both leaves - the write-back on the
   arts commit and the read at the Attack dispatch, where a replayed string wins
   over the no-input swing roll and an empty one falls through to it. See
   [save-record.md](../formats/save-record.md).
2. **Normalize arrows into art constants.** `FUN_801EED1C`'s player path walks the queue,
   matches each token run against the character's art command table (token compare via
   `addiu v1,v1,-0xb` at `0x801EF3E8` - the queue's `0x0C..0x0F` arrows against the art table's
   `0x01..0x04` direction bytes), and on a full match writes the starter over the run's **last**
   arrow and inserts the art constant after it: `addiu v1,t3,0x18; sb v1,0x1df(v0)` at
   `0x801EF6F0`/`0x801EF6F8` (`t3` = `FUN_801EFBFC`'s verdict, `1` known → `0x19`, `2` newly
   learned → `0x1A`), the shift-up loop `0x801EF708..0x801EF750` opening the slot (the 16-entry
   per-token side array at `0x801F6990` shifts with it, `0x801EF730..0x801EF744`), then
   `addiu v0,s3,0x10; sb v0,0x1df(v1)` at `0x801EF794`/`0x801EF7A0` (grid index + `0x10` → the
   `0x1B..` constant band). **The run's leading arrows stay in the queue** - it is not compacted -
   and the walk is tail-first (`s8` from 15 down, `0x801EF848`) restarting at `s8 + 1` after each
   match, so runs **overlap**: `↑↓↑` alone becomes `0F 0E 19 27`, and Tri-Somersault's whole
   input is seven arrows. Byte-level port + the Super-input derivation:
   [`legaia_art::tokenize`](../../crates/art/src/tokenize.rs) (see
   [art-data.md](../formats/art-data.md#super-arts)). Each accepted art is validated against the character's learned
   list by `FUN_801EFBFC` (`jal` at `0x801EF44C`; count at char record `+0x74D`, ids at
   `+0x74E..`), pays its AP (`lhu/subu/sh +0x170` at `0x801EF490..0x801EF49C`) and accrues the
   spent counter `+0x224` (`0x801EF4B4`). `FUN_801EFBFC` is more than a membership check - it is
   also the **arts learn-on-use inserter**: when the id is absent it returns `2` after an
   ascending-sorted insert into `+0x74E..` (shift loop `0x801EFD64..0x801EFDB0`, count bump
   `0x801EFE24`), but only for ids **above** the per-character innate cap at
   `0x801F686C + char_id - 1` (`sltu` at `0x801EFD14`; the zero id passes the gate as an edge)
   and only when the learn gate opens: `actor[+0x266] == 0`, **or** a 1/512 roll
   (`FUN_80056798() & 0x1FF == 0` at `0x801EFCC4`), **or** the debug byte `DAT_8007BD0C == 'O'`
   (`0x801EFCD4`). Returns `1` when already known, `0` when unknown and not learnable.
   Byte-level port: `legaia_engine_vm::battle_action::check_and_learn_art`.
   The engine runs it: `engine-core`'s `TacticalArtsTracker` holds the `+0x74D` count and the
   `+0x74E..` ascending id list per character, and the queue builder
   `World::build_arts_action_queue` calls `World::notify_art_used` once per accepted art in the
   builder's own **tail-first** order, rewriting that art's starter to `0x1A` when the call just
   learned it - retail's own seat (`jal 0x801efbfc` at `0x801EF44C`, verdict `+ 0x18` at
   `0x801EF6F0`). The order is load-bearing for one queue shape: with the same art entered twice
   the *last* occurrence is the one the check sees unknown, so it is the one that gets the `0x1A`
   - and step 4's reorder is what walks that verdict back to the art's first performance. So an art is learned on
   its first performance, and the learn banner fires once. Two retail inputs are
   supplied rather than read: the gate `ctx[+0x266 + slot]` has no engine analogue and reads as
   clear (gate open), and the innate cap at `0x801F686C` is un-parsed battle-overlay disc data
   that defaults to `0` until a host sets it.
3. **Miracle replacement (inline).** When the slot's Miracle marker `ctx[+0x25F + slot]` is set
   (`lbu v0,0x25f(v0)` at `0x801EF4C8`), the builder overwrites the whole 16-byte queue from the
   character's Miracle replacement string - the loop at `0x801EF4E8..0x801EF524` copies from
   `0x801F64F4 + (char_id-1)*0x10` (`addiu a1,v0,0x64f4` at `0x801EF4EC`; `sb v0,0x1df(v1)` at
   `0x801EF518`), i.e. the three resident strings at `0x801F64F4/0x6504/0x6514`, then flags
   `ctx[+0x28D + slot] = 1` and the shared trigger flag `0x801F696C = 1` (`0x801EF5A8`/`0x801EF5B4`).
4. **MSB clear + marked-starter reorder.** After the build loop the builder sweeps the 16-byte
   queue window clearing bit 7 of every byte (`0x801EF85C..0x801EF898`). It does it with a
   signed load and an *add*, not an AND: `lb v0,0x1df(a0)` / `lbu v1,0x1df(a0)` /
   `bgez v0, skip` / `addiu v0,v1,0x80` / `sb v0,0x1df(a0)` - adding `0x80` to a byte that
   already has bit 7 set wraps it off, so the effect is `& 0x7F`. This is the runtime half of
   the on-disc MSB quirk: it is what turns the Miracle row's leading `0x8C..0x8F` direction
   bytes back into `0x0C..0x0F`, and it runs **after** the Miracle copy of step 3 and **before**
   the Super applier of step 5. A second pass at `0x801EF8A0..0x801EF968` then walks the side
   array `0x801F6990` and, for each marked index `i > 0` whose queue byte is a `SpecialStarter`
   (`0x1A`), scans `j < i` and swaps `queue[j]` with `queue[i]` wherever
   `queue[j + 1] == queue[i + 1]` - no early exit, so the swap can fire more than once per `i`.
   The marks it reads come from the build loop, not from the Super applier, which runs later:
   each accepted art writes `1` at the index its starter lands on (`li v0,0x1` / `sw v0,0x0(v1)`
   at `0x801EF788..0x801EF78C`), the array is zeroed at the builder's head
   (`0x801EED5C..0x801EED74`) and every insert shift moves a mark with its byte
   (`0x801EF69C..0x801EF6B0`, `0x801EF730..0x801EF744`). Two details a paraphrase loses: the outer
   bound is `0xF`, not `0x10` (`sltiu v0,v0,0xf` at `0x801EF960` - index `i` always reads
   `queue[i + 1]`), and the inner loop reloads both compared bytes every iteration.
   Ports: `legaia_engine_vm::battle_action::clear_queue_msb`,
   `reorder_marked_starters` over `build_starter_marks`, both run by `finish_action_queue`
   between the Miracle copy and the Super applier.

   What the reorder accomplishes in queue terms: when one art appears more than once in a built
   queue, the `0x1A` newly-learned starter is exchanged with the `0x19` starter of the same art
   earlier in the stream, so the learn verdict travels to the art's **first** performance of the
   turn. Because the inner scan never exits early, a queue with three same-art slots swaps twice
   and the *last* match decides what index `i` keeps.
5. **Super find→tail-replace (helper call).** At its end (`jal 0x801EF9E4` at `0x801EF9AC`) the
   builder invokes **`FUN_801EF9E4`** (file `+0x211CC`;
   `see ghidra/scripts/funcs/overlay_battle_action_801ef9e4.txt`), which measures the queue
   (zero-terminator scan over `+0x1DF..` at `0x801EFA14..0x801EFA30`), then for each of the
   character's five Super rows compares the `find` pattern - `0x801F6524 + row*13 + char*65`
   (`addiu t6,v0,0x6524` at `0x801EFA3C`; 13-byte `[len][bytes...]` entries) - against the
   queue **tail** (`queue[len - find_len + j]`, the `subu v0,t4,a3` indexing at `0x801EFAC8`).
   On a full match it overwrites that tail from the `replace` table `0x801F65E8 + row*16 + char*80`
   (`addiu t8,v0,0x65e8` at `0x801EFA5C`; `sb a1,0x1df(v0)` at `0x801EFB7C`), marks the side
   array `0x801F6990[pos] = 4` for each written `0x1A` SpecialStarter (`0x801EFB84..0x801EFBA8`)
   and sets `0x801F696C = 1` (`0x801EFBD4`). Miracle-before-Super ordering is therefore
   structural: the Miracle branch runs inside the builder body, the Super applier only at its end.
   Two more of its laws matter to a byte-faithful mirror: rows are scanned **in table order and
   the first full match wins** (the match path exits the row loop by forcing the counter to 5),
   and the replace copy stops at the replace string's own terminator **without re-terminating
   the queue** - a replace longer than its find legally spills past byte 16 of the 19-byte
   stream. Byte-level port: `legaia_engine_vm::battle_action::apply_super_tail_replace`
   (equivalence with the structural `SuperMatcher` over the shipped tables is test-asserted).
   The applier is called **unconditionally**, including after a Miracle replacement - the
   Miracle row's tail matches no `find` row, so the two do not interact - and it applies at most
   one replace per builder invocation: the row loop exits on the first full match, and the
   builder calls it once.

The engine's entry point `legaia_engine_vm::battle_action::resolve_action_queue` - what
`engine-core` calls once per committed arts input - runs steps 3, 4 and 5 in that order on a raw
`ACTION_QUEUE_CAP`-wide byte window, so the live path is the byte applier's arithmetic rather
than the structural `legaia_art` matchers'. Two retail laws that reach the simulation through
that change: the Super scan takes the **first matching row in resident-table order** (not the
longest `find`), and it applies **once** (not to a fixpoint). Retail's Miracle gate has two halves and the port
now runs both. The per-slot marker `ctx[+0x25F + slot]` is **not** written by an input
recognizer - it has exactly one writer in the corpus, the party battle-actor seeding routine
`FUN_80053CB8` (`sb v1,0x25f(v0)` at `0x80054270`), which raises it when one equipment byte of
the acting character's record is occupied: `+0x761` (record-relative `+0x199`) for every roster
char id except `2`, and `+0x760` (`+0x198`) for id `2` (`beq v0,a3,0x80054228` with `a3 = 2` at
`0x800541E4`). Since `crates/save/src/character.rs` records the per-character weapon index
`_DAT_8007B42C` as `2, 3, 2`, the byte this gate reads is in every case the **other** member of
that pair - the slot the equip screen's row map never exposes, i.e. the Ra-Seru. Real memory-card
saves agree: the gate byte is small and per-character-banded (Vahn `1` early, `7`/`9` at the end;
Noa `15`/`17`; Gala `23`/`25`) while the paired weapon byte carries the ordinary shop ids, and it
reads zero for a member who has not bonded with a Ra-Seru yet. Port
`legaia_engine_vm::battle_action::miracle_marker_armed`, read at queue-build time by
`World::miracle_marker_armed_for` (the byte cannot change between battle entry and an arts
commit). The second half is the builder's own combo compare against the ordinal-`0` art record,
for which the engine keeps its whole-string match against the character's Miracle command table.

The marker gates more than the Miracle: the builder routes **every** special record - ordinal `0`
(Miracle) and ordinals `1..=3` (the three Hyper Arts) - through the same `+0x25F` test at
`0x801EF4C8`, and with the marker clear that arm writes nothing at all (`bne t5,zero,0x801EF7B4`
at `0x801EF6E0`). So a party member without a Ra-Seru can enter a Hyper or Miracle string and get
an ordinary chain out of it.

The consuming side is unchanged: the strike loop reads `actor[+0x1DF + +0x15]` and the round
driver's queue clear runs the `sb zero,0x1df(v0)` loop at `0x801D89D8` inside `FUN_801D88CC`
(called from `FUN_801D0748` at `0x801D0E84`/`0x801D0ED0`). `FUN_801EED1C`'s non-player heads
(the Tetsu-tutorial forced chain `0E 0F 0E 0F` at `0x801EEDE0..0x801EEE04`, the
no-directional-input arm below) share the same emission sites.

### The no-directional-input attack queue

`FUN_801EED1C` has one arm that produces a complete attack queue from **no player input at
all**. Its head selects the arm on `(&DAT_8007BD10)[slot] == 4` at `0x801EEE40..0x801EEE48`; the
same table's `!= 4` fall-through is the ordinary player path that normalizes recorded arrows.
`DAT_8007BD10` is the slot -> **roster character id** table, not a control-mode byte: three
routines index the character records with it as `0x80084140 + (byte - 1)*0x414`
(`0x801EF344..0x801EF360` here, `0x80053CEC..0x80053D10` in the actor seeding, and
`0x801E39CC..0x801E39F8` in the strike loop). So the arm's condition is "the character seated in
this slot is roster id 4" - the AI-driven guest - rather than a mode flag.

The arm's own body is short, and every store in it is a queue store:

```text
801eef0c  jal   0x80056798        ; rand()
801eef18  beq   v0,zero,801ef030  ; (rand & 1) == 0 -> actor[+0x1DE] = 0, no action
801eef28  sb    v0,0x1de(v1)      ; else category 3 = Attack
801eef2c  jal   0x80056798        ; rand()
801eef3c  lbu   v1,0x1(v1)        ; ctx[+0x01] = seated monster count
801eef6c  mfhi  v1                ;   rand % count
801eef78  sb    v1,0x1dd(v0)      ; target = 3 + that, re-rolled while +0x14C == 0
801eefb0  addiu v1,v1,-0x6cb8     ; 0x801C9348, the monster record-pointer table
801eefc8  lbu   v1,0x1e(v0)       ; target record +0x1E
801eefd0  beq   v1,v0,801ef028    ;   == 2 -> single low swing
801eefd4  _li   v0,0xe
801ef02c  _sb   v0,0x1df(a0)      ;            queue[0] = 0x0E
801eefd8  jal   0x80056798        ; else rand()
801eeff8  addiu v0,v0,0xc         ;   0x0C + (rand % 2)
801ef000  _sb   v0,0x1df(v1)      ;            queue[0] = 0x0C | 0x0D
801eeffc  jal   0x80056798        ; rand()
801ef01c  addiu v0,v0,0xc         ;   0x0C + (rand % 2)
801ef024  _sb   v0,0x1e0(v1)      ;            queue[1] = 0x0C | 0x0D
```

So a basic Attack with no directional input is **two arm swings**, each independently rolled
Left (`0x0C`) or Right (`0x0D`), against an ordinary target - or **one low swing** (`0x0E`)
against a target whose record `+0x1E` reads `2`. The `% 2` is retail's signed-safe
`v0 - (v0/2)*2` idiom at `0x801EEFE0..0x801EEFF0`. No terminator is written: the window is
already zeroed by the round-boundary clear, and `0x00` is what the attack band stops on.

`+0x1E` sits between the record's element byte `+0x1D` and its size class `+0x1F`, and is parsed
as `legaia_asset::monster_archive::MonsterRecord::swing_class`. It is not a rare byte: across the
186 decodable records of the archive it reads `0` for 127, `1` for one, **`2` for 52** and
**`3` for six** - so both of the classes the two kernels branch on are ordinary enemies, and both
arms are reachable in normal play. Two kernels read it, both
record-direct through `0x801C9348` and never off the actor: this arm, and the damage kernel - in
its head, where a party hit whose power byte is of the wrong class **misses** outright
(`0x801EC488..0x801EC554`,
[battle-formulas.md](battle-formulas.md#the-limb-vs-height-miss)), and in its apply-mode
look-ahead (§3, `0x801EE080`), which asks the same question of the hits still to come. A class-`2`
target connects only with power bytes in `0x01..=0x10` and a class-`3` target only with
`0x11..=0x15`. Together they read as the height / posture class behind retail's limb-vs-height
"Miss"; the disassembly pins the effects, not the name.

**Port.** `legaia_engine_vm::battle_action::basic_attack_queue`, byte-for-byte including the
two-draws / no-draws RNG split. `engine-core`'s `World::seed_basic_attack_queue` calls it from
both party arming sites - the command menu's Attack confirm (and its no-valid-target fallback)
and the auto / confused party turn `arm_party_physical`. The engine's Attack command is
precisely this situation: it resolves a target and carries no direction input, so this is the
retail kernel that applies. The `+0x1E` class reaches it through
`World::attack_swing_class_of` - the seated monster id resolved through
`MonsterDef::swing_class` - so a class-`2` disc target takes the single low swing. A party
target, an empty slot or a synthetic catalog reads `0` and takes the two-arm-swing shape, which
is retail's own answer for every non-class-`2` target.

#### Why the seed is load-bearing, and where the damage goes

State `0x1E` is a walk over the stream; it is not a strike primitive with a stream attached.
An Attack that arms the band without seeding `actor[+0x1DF..]` reads its `0x00` terminator on
byte 0 and drops to recovery on the first frame, so **the entire retail strike loop is skipped**
- nothing is staged into `+0x1DA`, the equipment swing clips at action-table slots
`0x0C..0x0F` never commit, no effect script is installed, and the move-power record the
weapon-trail streak projects from never resolves (its key is this stream's first byte, read at
`engine-core`'s `step_actor_effect_script`). The port had exactly that shape: damage still
landed because `engine-core`'s live loop applied it through its own edge-triggered path, so
nothing failed loudly.

That leaves one reconciliation to state, because both halves can now fire. The authoritative
seam is retail's: `FUN_801EC3E4` resolves one hit per **committed arms command** (SCUS calls it
at `0x800478A0`), so the port applies one strike per swing byte the chain stages, keyed on the
staged byte being a direction swing (`0x0C..=0x0F`). The live loop's edge-triggered
`apply_basic_attack` now runs only when the chain consumed **zero** bytes - which is the
monster band, whose swing count is the AGL budget (`FUN_801E9FD4`) rather than a queue. The
staged byte is also what picks the defence half and the command power scalar for its own
strike, so a low swing and an arm swing no longer resolve against the same number.

When the active actor's `chosen_art` is set and `art_record` returns a record, `attack_chain` (state `0x1A`) calls a second host hook `apply_art_strike(ArtStrikeInfo)` alongside the existing `apply_damage`. `ArtStrikeInfo` carries the strike-indexed power byte, dmg_timing, hit cue, and the art's flat status effect. Engines drive HP deduction, status application, sound-effect scheduling, and visual hit-cue dispatch off this struct; tests feed synthetic `ArtRecord` instances and assert the per-strike `(power, timing, effect, cue)` resolution rather than going through `apply_damage`'s legacy `(icon, page, target, slot)` parameter pack.

The engine-side translator at `crates/engine-battle/src/art_strike.rs` (`apply_art_strike(attack, defense, info) -> ArtStrikeOutcome`) folds an `ArtStrikeInfo` into a concrete HP delta + status flag + scheduled SFX cues using the `art_strike_damage` formula in `legaia_engine_vm::battle_formulas`. The world's `BattleActionHost::apply_art_strike` impl resolves the per-slot weapon attack from `World::battle.attack` and the right defense (UDF or LDF, picked from `World::battle.defense_split`) before calling the translator, then emits a `BattleEvent::ApplyArtStrike` with the resolved `ArtStrikeOutcome`. Engines apply each strike's `damage` / `enemy_effect` / `cues` through whatever runtime they have for HP / status / SFX dispatch.

`World::fold_battle_event` folds the `ApplyArtStrike` outcome: HP / status into the target, and the outcome's **sound cues** (`cue.is_sound()`, the `HitCue::kind` SfxBank ids - distinct from the move-power `+0x0d` `FUN_8004fcc8` namespace) into a per-frame `BattleSfxCue` queue the host drains via `World::drain_battle_sfx_cues` (the audio sibling of `drain_battle_hit_fx`). The host plays each through `SfxBank::play_one_shot` at the cue's `timing_frames` delay. The live battle loop wires this end to end: the SFX bank is decoded from the user's executable at boot and the cues key on through the per-scene VAB (see [`battle.md`](battle-round-loop.md#sfx-bank--scheduler)).

### Spirit / Run in the live command menu

The live player-driven command menu (`legaia_engine_core::battle_input::BattleCommand`) carries
all six commands: Attack (target cursor + physical strike), Arts / Magic / Item (host-submenu
hand-offs), **Spirit** and **Run**. Spirit resolves without a target: the live loop charges the
caster's AP gauge (`ApGauge::charge_spirit`, the retail Square-press +5) and raises a per-slot
guard stance - the engine model of the retail pending-action byte `+0x1DE == 4` the damage
finisher's guard-halve stage reads (`DamageFinish::defender_guarding`, `over >>= 1`) - held
until that actor's next turn starts. Run arms the ported run band (`Begin` -> category 5 ->
`RunBegin`/`RunWait`/`RunEscape`) with the roll outcome staged on `absorbed_seru`; a success
tears the battle down `Escaped` (no loot, downed members floored alive at 1 HP), a failure
consumes the turn through the Done band. The escape *probability* is the retail
[`FUN_801E791C` roll](battle-action-helpers.md#the-escape-roll-fun_801e791c) - party vs enemy speed/missing-HP scores
plus the two Chicken accessory bits - ported as `battle_formulas::escape_roll` and rolled by
`World::roll_battle_escape`.
