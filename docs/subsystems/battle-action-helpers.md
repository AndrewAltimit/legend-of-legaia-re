# Battle action helper functions

## Cross-references with other battle helpers

### `FUN_8004E2F0` - battle range / reach metric

Called from states `0x14`, `0x16`, `0x19` (during the attack chain) and from the anim tick's root-motion gate (`FUN_80047430`). Returns a 16-bit distance metric; `0` = "in range". The full law, from the disassembly (`ghidra/scripts/funcs/8004e2f0.txt`):

```text
if DAT_8007BD71 != 0xFF or either slot >= 8: return 1
base = 0; size = 0
if attacker < 3: base = i16 DAT_80078878[(DAT_8007BD10[attacker] - 1) * 2]
else:           size = monster_rec(attacker)[+0x1F]
if target >= 3:
    t = monster_rec(target)[+0x1F]
    size = t if size == 0 else ((size + t) * 3) / 5     ; unsigned divide
if attacker < 3 and target < 3: base >>= 1              ; sra - party on party
a = (FUN_80019B28(tgt[+0x40], tgt[+0x3C], att[+0x38], att[+0x34]) + 0x800) & 0xFFF
d = base + |(|att.x - tgt.x| * sin[a]) >> 12| + |(|att.z - tgt.z| * cos[a]) >> 12|
in_range = (d < i16 DAT_80078870[size*2])   if size < 3   ; signed slt
         = (d <u size << 4)                 otherwise     ; unsigned sltu
return 0 if in_range else (i16)d
```

Note the asymmetric position read: the **attacker's live** pair `+0x34`/`+0x38` against the **target's seat** pair `+0x3C`/`+0x40`. `DAT_80078870` = `{256, 384, 1024}` (small-class thresholds); `DAT_80078878` = per-character reach offsets `{+43, 0, -53, -100}` for roster char ids `1..=4` - **added to the distance**, so a positive value is a shorter reach. Engine port: `legaia_engine_vm::battle_action::motion::range_metric`, assembled from live state by `World::battle_range_metric` (`engine-core::world::battle::locomotion`).

### `FUN_80042558` - per-frame stat aggregator

Not called *directly* from `FUN_801E295C`, but the global ability bitmask it maintains (4×u32 at `0x80074358..0x80074368`) is read indirectly here:

- State `0x28` reduces MP cost by half (bit `0x20`, `cost - cost>>1`) or by a quarter (bit `0x10`, `cost - cost>>2`) based on the character record bits - `0x20` takes priority when both are set (the bit indices match the bitmask layout `FUN_80042558` populates).
- States `0x1E` (attack drift) and `0x46` (spirit-arts HP-bar) read character record bits `0x100`/`0x200` for impact-magnitude scaling.

The bitmask is cited via `*(uint *)(((byte)(&DAT_8007BD10)[ctx[+0x13]] - 1) * 0x414 + -0x7FF7B804)` - i.e. the active character's record at `0x80084708 + (party_id - 1) * 0x414 + 0xF4`, which is exactly the per-character `+0xF4..0x100` block that `FUN_80042558` OR-aggregates into the global bitmask.

**Field map (character record `0x80084708 + n*0x414`, `n = 0..2`), from
`ghidra/scripts/funcs/80042558.txt`.** Every field `FUN_80042558` reads or writes lives in the
record's `+0xF4..+0x13D` region:

| Offset | Field |
|---|---|
| `+0xF4..0x103` | 128-bit ability/passive bitfield (4×u32). Cleared, then each active passive sets bit `index` (`index < 0x40`); also OR-aggregated into the globals `DAT_80074358..0x80074364`. |
| `+0x104..0x11B` | Effective (passive-boosted, capped) stat block, seeded from `+0x11C..0x12D`. `+0x104` HP (cap `9999`), `+0x108` MP (cap `999`), `+0x10C` (cap `100`), `+0x110` AGL-class (cap `0x118` = 280), `+0x112/0x114/0x116/0x118/0x11A` combat stats (cap `999`); `+0x106/0x10A/0x10E` are running-minimum companions. |
| `+0x11C..0x12D` | Base (unmodified) stat block - the source the effective block is rebuilt from each frame. |
| `+0x13C` | Count of learned Seru/ability entries. |
| `+0x13D..` | That many ability/Seru id bytes (ids `0x99..0xA0` handled). |
| `+0x196..0x19D` | 8 equipped-item ids; each item's descriptor (`kind==1`→equip-bonus `+5`, `kind==2`→item-effect `+3`) supplies the passive index bit set in `+0xF4`. |

The percent boosts applied per ability bit are the accessory-passive magnitudes (`+10%` = base/10,
`+25%` = base>>2, `+20%` = base/5; see [accessory-passive-table.md](../formats/accessory-passive-table.md)).

**Scope correction (do not re-walk).** `FUN_80042558` touches **only** this character-record
`+0xF4..+0x13D` block. It does **not** write the *battle-actor runtime* struct (the
`DAT_801C9370` actor pool) - `actor[+0x14C]` (HP), `actor[+0x150]` (MP) and `actor[+0x176]` are
runtime fields written by the battle loader and this action SM (`FUN_801E295C`), a different
struct. The earlier attribution of `+0x14C..+0x176` to `FUN_80042558` is wrong.

### `FUN_801E752C` - per-round status DoT ticker

Not an SM state: the round driver `FUN_801D0748` calls it once per round (state
`0x14`, gated on the round counter `ctx[+0x28A] != 0`, so the first round never
ticks). Applies the Venom / Toxic HP drains off the `+0x16E` status bits -
exact arithmetic, caps, and the never-kill clamp in
[battle-formulas.md](battle-formulas.md#per-round-status-dot-ticker---fun_801e752c);
ported as `engine-vm::status_effects::toxic_tick_damage` / `venom_tick_damage`
(`StatusEffectTracker::tick_actor`). The same walk pays the Life Grail / Magic
Grail per-round recoveries for party slots.

### `FUN_801DD4B0` / `FUN_801DD6B4` - the two per-move damage-roll wrappers

Two sibling **damage kernels** the action path calls to resolve one hit. Each
draws an attacker roll and a defender roll from the two battle-actor records
(`(&DAT_801C9370)[slot]`), calls the affinity scale `FUN_801DD864` and then the
closed-form finisher `FUN_801DDB30`, and returns `attacker_roll - defender_roll`
as the net damage. They differ in two ways: `FUN_801DD4B0` mixes the INT-working
stat `+0x168` into both rolls and passes finisher **`param_5 = 0`**
(the equipment resist ladder - jewels / elemental guards / All Guard - runs);
`FUN_801DD6B4` uses the ATK-working stat `+0x158` and passes **`param_5 = 1`**,
which makes the finisher **skip the whole party-defender resist block**. The
`param_5 = 1` path is the **resist-BYPASS** wrapper: a hit routed through it
takes no Earth/Luminous-Jewel or All-Guard reduction even when the defender is
elementally warded, which is the mechanism behind the non-elemental capture-class
boss casts (Bloody Horns / Terio Punch). The affinity scale still reads the
caster's slot element either way, so the attacker's element is applied - only the
defender's jewel stage is dropped. Full stat fields, the finisher stage list, the
per-spell module census, and the engine mirror (`damage_finish::bypass_party_resist`)
are in
[battle-formulas.md § Summon-magic damage roll](battle-formulas.md#summon-magic-damage-roll---fun_801dd0ac).
See
`ghidra/scripts/funcs/overlay_battle_action_801dd4b0.txt` /
`_801dd6b4.txt`.

### The queued-magic follow-up guard (`FUN_801F3C34`)

State `0x36` calls this once per summon / Seru-magic return-from-fade
(`jal 0x801f3c34` at `0x801E4CB8`, the SM's only reference to it). It resolves
the acting actor's queued action `+0x1DF` inside the caster's own learned-spell
list - record `+0x13D` ids against the parallel `+0x161` levels, the character
selected through `(&DAT_8007BD10)[ctx[+0x13]]` - and, when that spell's level is
`>= 3` and no follow-up is already pending (`*(0x801F6960) == 0`), installs the
follow-up routine pointer `0x801CFA20` into `*(0x800775B4)`, prints message
`0x66` through `FUN_801D8DE8`, and mirrors `0x66` into `ctx[+0x18]`. Action ids
`0x85`, `0x8E` and everything from `0x96` up return before the record is read.

Its sibling `FUN_801F3D3C` is the **installer** that seeds the latch the guard
reads. It repeats the level scan, then runs a suppression roll before choosing a
record out of the `[element][level band]` table at `0x801F6870` (`0x20` bytes per
element = four 8-byte records; band = `(level - 3) >> 1`). The roll indexes
`0x801F53E8` - the same **element-affinity matrix** the damage path uses, not a
separate table - with the two records' `+0x1D` element bytes
(`(*(0x801C9358))[+0x1D]` acting side, `(*(0x801C9348))[+0x1D]` opposing), and
suppresses when that affinity percent is **below** `0x65`: the follow-up needs
the opposing side to be elementally weak. Three shapes bypass the roll
entirely - `ctx[+0x287] == 0`, an acting element of `5`, and a `rand()`
divisible by five. An acting element below `7` then dispatches through the
seven-entry per-element jump table at `0x801CFA2C` instead of reaching the
installer tail.

Ported as `legaia_engine_vm::move_no_effect_guard` and live: state `0x36`'s
settle body calls it (`battle_action::summon`, the `jal 0x801f3c34` site at
`0x801E4CB8`) with the caster's spell list from
`BattleActionHost::caster_spell_list`, which `World` answers from the
character record, and every player Seru cast writes the `0x801F6960` /
`0x801F6964` latch pair it reads.

### AI-delegated (`0x380`) party members - what is and isn't pinned

`FUN_80047430` sets `actor[+0x16E] |= 0x380` each frame on a party slot whose
character record carries ability-bitfield bit 45 (`+0xF8 & 0x2000` = accessory
passive `0x2D` Rage / Evil Medallion - the neighbouring bits byte-match the
[accessory-passive index table](../formats/accessory-passive-table.md): `0x100`/
`0x200` = AP Boost, `0x800` = AP Used Down, `0x20`/`0x40` = HP/MP After). The
SM and the next-actor selector treat the bits as "AI-controlled", but the code
that *chooses* an action for a delegated **party** member is not in the dumped
corpus: the round driver `FUN_801D0748` routes party slots to the player
command menu with no `0x380` test, `FUN_801DABA4` calls the AI picker
(`FUN_801E9FD4`, fully dumped + ported as `engine-core::monster_ai`) only for
monster slots (`a0 = active_index - 3`, gated `active_index >= 3` at
`0x801DAEF8`), and `FUN_801EED1C`'s auto-fight block is keyed to **character id
4** (`(&DAT_8007BD10)[slot] == 4`; `DAT_8007BD10[slot]` is the per-slot roster
character id - byte-confirmed `01 02 03` = Vahn/Noa/Gala in
`evil_medallion_rage_battle` - and the block indexes the live char record
`(id-1)*0x414 + 0x800847FC`), **not** to `0x380`. So char id 4 is the
AI-controlled companion (Terra in the retail roster), not a Rage delegate.
Pinning the **Rage** party-side auto-pick *writer* (does it cast? does the
pattern vary?) still needs a runtime capture watching the writers of
`actor[+0x1DE]`/`+0x1DD` during the command phase - the `0x380` flag is consumed
in **only** three dumped battle functions (`FUN_801E295C`, `FUN_801E9FD4`,
`FUN_801DABA4`) plus the charm redirect `FUN_801E7320`, none of which fills an
arts-combo stream for a controllable character, so the captured arts combo below
was set upstream (the command-menu controller, undumped). The monster-side
confuse behaviour *is* pinned (picker `& 0x380` guards + `FUN_801E7320` retarget
at ActionSeed).

**Char-id-4 (Terra) auto-AI pick - pinned.** `FUN_801EED1C`'s `== 4` block
chooses by **battle seat 0's** gauge (`+0x14C` current / `+0x14E` max) and
status word (`+0x16E`), and writes **battle seat 1**. The `0xC8` gauge seed is
a separate, earlier gate on `DAT_8007BD11 == 4` (`0x801EEE10`) and lands on
seat 1's `+0x174` / `+0x150` / `+0x172` / `+0x14C`.

| condition (seat 0) | category `+0x1DE` | detail | writer PC |
|---|---|---|---|
| `+0x14C == 0` | `2` (Magic) | spell id `0x16`, target 0 | `0x801EEE70` |
| `+0x14C < +0x14E >> 1` | `2` (Magic) | spell id `0x0D` | `0x801EEEAC` |
| healthy, `+0x16E != 0` (statused) | `2` (Magic) | spell id `0x11` | `0x801EEEE0` |
| healthy, no status | `3` (Attack) or none | `rand() & 1`: half Attack, half category `0` (stand by) | `0x801EEF28` |

The spell id lands in `+0x1DF` (`0x801EEEF8`) and `+0x1E7 = 9` (`0x801EEF00`).

**Two corrections to an earlier reading of this block.** It does *not* watch
its own gauge: `lw v1, -0x6c90(0x801D0000)` at `0x801EEE50` is
`actor_table[0]`, while every write goes through `lw v1, 4(s0)` -
`actor_table[1]`, a fixed `+4`. So it is a healer watching the party leader.
And the physical arm is not "a 1-2 hit directional stream of
`0x0C`/`0x0D`/`0x0E`" - see [the physical arm](#the-companions-physical-arm).

Ported as `legaia_engine_vm::battle_action::ai_companion_pick`, wired from
`World::arm_party_physical` for the party slot whose roster character id is
`4`.

##### The companion's physical arm

`0x801EEF2C..0x801EF024`, in draw order:

1. `rand() % ctx[+1] + 3` picks a monster seat and is stored to `+0x1DD`
   (`0x801EEF74`);
2. if that seat's `+0x14C` is zero the roll is **redrawn** - the loop at
   `0x801EEF98` is unbounded, so a row with no living monster spins;
3. the chosen seat's monster **record** `+0x1E` is read through
   `0x801C9348[target - 3]` (`0x801EEFC8`) - the swing class;
4. on `+0x1E == 2` the stream is the single command `0x0E` and no further
   draw happens (`0x801EF028`);
5. otherwise `+0x1DF` and `+0x1E0` each take an independent
   `rand() % 2 + 0x0C`, so the stream is always exactly two commands, each
   `0x0C` or `0x0D`.

The port bounds the redraw at
`legaia_engine_vm::battle_action::AI_COMPANION_MAX_TARGET_DRAWS` and stands by
instead of spinning; every other step is the bytes'.

**One delegated pick is now observed** (`evil_medallion_rage_battle`; disc +
library gated `rage_delegated_pick`). In the battle-actor pool, exactly the
Evil-Medallion wearer carries the delegation bits `+0x16E & 0x380 == 0x380`
(the other party slots read `+0x16E == 0`), and its already-resolved pick is
category `+0x1DE == 3` (Attack) with the `+0x1DF` action stream
`[0x22,0x26,0x25,0x22,0x21]` - a five-element multi-strike, not a single plain
attack. Two qualifications: (a) within the **battle-actor** struct the `+0xF8`
bit `0x2000` is set on every party slot at this instant, so there it is not the
per-actor delegation discriminator - `+0x16E & 0x380` is (the
`FUN_80047430`/`+0xF8 & 0x2000` relation above is on the **character record**,
a different struct); (b) it is a single sample.

**The writer is `FUN_801F0450`'s auto-fill arm, and the sample fits it.** The
arm's gate reads the **character record**, not the actor: `lw v0,0x6c0(v1)` at
`0x801F04D4` with `v1 = 0x80084140 + (id - 1) * 0x414` is record `+0xF8`, and
bit `0x2000` there is ability index `0x2D`, Rage - the Evil Medallion's passive
([`accessory-passive-table.md`](../formats/accessory-passive-table.md)). With
`actor[+0x16E] & 0x404` clear the arm stamps category `3`, rolls a target, then
loops: stop on `rand() % 7 == 0`, else draw `record[+0x186 + rand() % count]`,
discard a draw under the floor (`4`, or `6` for id `2`) and store `id + 0x1B`
otherwise, up to fifteen (`0x801F05C4..0x801F06CC`). The capture's
`[0x22,0x26,0x25,0x22,0x21]` is five learned arts `7, 11, 10, 7, 6`, all at or
above either floor and drawn with replacement, which is that loop's output
shape; its length and order are per-roll variability. The port runs this arm
from its retail call site (`auto_fill_party_queues` in `engine-vm`), so the
Rage pick is no longer a stand-in.

**`FUN_801F0450` - the auto arts-combo assembler (a candidate writer).** A REAL
928-instruction battle-action body (`--explain 801f0450` => `REAL`, entry
`801f0450`, `jr ra`) that, for each party slot with `ctx[+0x266 + slot] != 0`
and action category `actor[+0x1DE] == 3` (Attack), **fills the `actor[+0x1DF+n]`
arts-command stream** the strike loop later walks - the AI-side counterpart to
the player queue-builder `FUN_801EED1C`. It scans the per-command arm entries of
the arts-command table `DAT_801C9360[slot][cmd]` (`cmd` `0xC..=0xF`), reads each
command's AP cost byte `+0x74` (the same [arts AP-gauge cost](arts-command-gauge.md)
the randomizer edits), builds a weighted candidate pool - dropping any command
whose per-command elemental/status guard mask `DAT_801F672C[cmd-0xC]` collides
with the target's status word `actor[+0x16E]` - then draws from the pool with
`func_0x80056798` (RNG) until the actor's special gauge `actor[+0x154]` no longer
covers the cheapest command. (The record `+0xF8 & 0x800` halving an earlier
reading put here belongs to the art insertion tail below, where it halves the
per-arrow Spirit cost, not this gauge.) It is the natural producer of the observed delegated
`[0x22,0x26,0x25,0x22,0x21]` multi-strike, but the outer gate keys on `+0xF8 &
0x2000` / `+0x16E & 0x404` rather than the `0x380` delegation bits directly, so
confirming it is *the* Rage-delegate path (versus a shared auto-fight assembler)
still wants the runtime `actor[+0x1DF]`-writer capture this section calls for. No
engine consumer yet. See
`ghidra/scripts/funcs/overlay_battle_action_801f0450.txt`.

Two refinements from the disassembly. The gate is a **fork, not a filter**: the
`+0xF8 & 0x2000` set / `+0x16E & 0x404` clear case takes a *simpler* arm that
skips the arts-command table entirely - it seeds category `+0x1DE = 3`, rolls a
target over the live monster slots through `FUN_801DB124`, and then draws blindly
from the character's own learned-arts list (`record[+0x185]` count,
`record[+0x186 + i]` ids), appending `id + 0x1B` per keep with a per-character
floor of `6` for participant id `2` and `4` otherwise, stopping on a `rand() % 7
== 0` roll or at fifteen entries. The pool arm above is the *other* side of that
fork. And the weight a command earns is a four-rung ladder (`8` default, `1` low
band, `4` high band, `2` both) over two byte ranges selected by the **target
monster's type byte** `+0x1E` - type `3` reads `..=0x10` / `0x16..=0x1A`, type
`2` reads `0x11..=0x15` / `0x1B..=0x1F`, and any other type leaves every command
at `8`, which is enough pushes to overrun the `0x10`-byte candidate scratch.

Both arms plus the ladder, the guard reject and the gauge-spend loop are ported
as `engine-vm::battle_arts_auto_combo`; the arm-by-arm decode lives in
[`reference/functions/battle.md`](../reference/functions/battle.md#801f0450).
The pool arm's gate `ctx[+0x266 + slot]` is the per-fighter **Auto** flag the
command SM's Auto pick writes (see
[`minigame-muscle-dome.md`](minigame-muscle-dome.md)), so the pool arm and its
tail are the Auto command's queue builder.

#### The routine runs every round, and Auto rebuilds the queue

State `0x00` is not a once-per-battle arm. Its one zero-writer is the flow SM's
`0xFE` arm (`sb zero,0x7(v0)` at `0x801D3224`), and the flow reaches `0xFE`
from the commit confirm's Begin (`li v0,0xfe` at `0x801D31AC`, in the `0x6E`
handler) and from the Run confirm - so every round opens with `jal 0x801f0450`.
A seat flagged Auto whose category is Attack therefore has its `+0x1DF` queue
**rebuilt** by the pool arm at the round's start: the directions drawn from the
weighted pool and spent against the action gauge, then the tail's arts
spliced over them (the pool arm's `sb a0,0x1df(v0)` at `0x801F0B04`). The
saved command string the Attack confirm pre-seeds (`FUN_801DA34C`) is what the
queue review shows; it is not what an Auto round plays.

The flag's writers, all in `FUN_801D0748`: `0` every frame the ring is up
(`0x801D11A8`), `1` on the prompt's `Auto` chip (`0x801D17D0`), `0` on
`Command` (`0x801D1760`), and the option word itself when the `Automatic`
option skips the prompt (`sb s0,0x266` at `0x801D164C`, `s0 = 1`). Its readers
are this routine (`0x801F0704`), the queue review's cancel arm (`0x801D23A0`)
and two HUD gates.

The port runs the pool arm and the tail from `World::begin_round_execution`
(`engine-core`'s `world::battle::auto_combo`) for every Attack a player
committed off an Auto pick, and the member's dispatch plays the parked queue.
The command costs and leading entry bytes come from the equipped swing
records, the four guard masks from PROT 0898 at `0x801F672C`, and the tail's
records from the character's art-animation bank. The **delegated** arm
(record `+0xF8 & 0x2000`) and the formation arm run in the same place, once per
round, through `vm::battle_action::round_state_zero` (`World::run_round_state_zero`),
before the round's first action dispatches; the per-action `Begin` the port
re-arms finds the round's pass done and skips it.

#### The art insertion tail (`0x801F0B4C..0x801F1274`)

After the spend loop has written a run of direction swings, the tail walks
the character's art-animation bank (`*(DAT_801C9360[slot] + 0x58)`, `0xD0`
stride, [battle-data-pack.md](../formats/battle-data-pack.md#art-animation-bank-record0-0x58))
and splices learned arts' arrow strings over the end of the still-free part of
the queue, paying out of a **local copy** of the Spirit gauge `actor[+0x170]`
(nothing is stored back):

- The walk starts at record `rand() % 5 + 0xB` - bank index `0xB` is
  learned-art id `0` - and Noa (`char_id == 2`) steps over `0xD` / `0xE`.
- **Spirit gate**: each pass stops the walk unless `rand() % 7 + 0x12` is
  below the budget and at least two free slots remain.
- A record needs at least two arrows (byte `1` non-zero). Its need is counted
  from byte `1` on - byte `0` is spliced but never counted (`li s2,0x1` at
  `0x801F0DA0`) - against a census of the free region.
- Cost is `len * per_input`, with `per_input` `0xB` on the first pass, `0xA`
  after, `6` from the fifth, halved under record `+0xF8 & 0x800`. Without the
  slot's Miracle marker `ctx[+0x25F + slot]` the first four passes demand
  `100` of arrow `1`, so nothing can be spliced before the cheap passes.
- Then a `rand() % 100` roll under `50` (below the tier bound: `0x11` for Noa,
  `0xF` otherwise) or `75`, a learned-list hit, not the art just placed, and
  not below the floor a placed low-tier art raises to that bound.
- Accept: the free region's head is refilled with `rand() % 4` directions
  drawn from what the census has left, its last `len` slots become the combo
  as `arrow + 0xB`, the region shrinks by `len` and the walk re-seeds at
  `rand() % 3 + 0xB`.
- Reject: skip ahead by twice the zero-terminated run at record `+0x0B` - the
  loop's `a0` is loaded once and its delay-slot increment fires on both
  edges. On the retail banks that run is empty on every Vahn and Gala record
  and one byte on one Noa record.
- Every pass then steps `rand() % 2 + 1`.

Port: `battle_arts_auto_combo::insert_arts`, run by the player's Auto attack
(above).

### Enemy AGL action-budget (`FUN_801E9FD4`)

The monster AI picker `FUN_801E9FD4` (fully dumped + ported as
`engine-core::monster_ai`) queues **more than one action per turn** out of an
AGL-scaled budget - the enemy analogue of the party's [Arts command
gauge](arts-command-gauge.md). Its physical branch fills the actor's
action-parameter byte stream at `actor[+0x1DF..]` by repeatedly rolling
candidate moves (each candidate's tag byte at `+0x00` in the `0x0C..0x1F`
command band, its cost the same `+0x74` swing-record byte the party gauge
reads) and appending them while the budget holds. The budget is the per-round
**AGL gauge** at `actor[+0x154]`, seeded from the monster record's AGL
(`+0x0E`) and reset to base at the start of each round by `FUN_801D88CC`; each
appended action debits the move's cost. The fill is bounded at 15 queued
actions and 16 failed candidate rolls so a low-cost roster can't loop forever.
So an agile enemy takes several strikes per turn, the same "wide gauge = more
commands" mechanic the party's arm width drives ([arts-command-gauge.md
§ How the gauge consumes it](arts-command-gauge.md#how-the-gauge-consumes-it)).

Because both sides of the budget are *disc data* - the AGL seed is the record's
`+0x0E` halfword and each candidate's price is its entry's `+0x74` byte in the
monster archive (PROT 867) - the per-turn hit count is a randomizer target: the
patcher's `--enemy-attack-count` multiplier rescales the affordable attack
entries' cost bytes in place
([randomizer.md § Enemy attack count](../tooling/randomizer.md#enemy-attack-count)).

### `FUN_801DFDF8` - effect-bundle public spawn API

`FUN_801E295C` does **not** call `FUN_801DFDF8` directly. Effect spawning happens through one of two indirections:

- **`FUN_801D8DE8(element, mode)`** - the hottest battle utility, called 30+
  times across the state machine. It is the **screen-element spawner**, not an
  effect spawner: `element` indexes the placement table `0x80076C10 + element *
  0x18`
  ([`memory-map.md`](../reference/memory-map.md#0x80076c10---one-table-three-names)),
  the record is seated as a text / chrome widget through `FUN_8003541C`, and
  `FUN_801DB7B0` glides it between the record's two seats (`mode & 1` picks the
  spawn seat, `mode & 2` suppresses the glide). Its only calls are
  `FUN_8003541C`, `FUN_801DB7B0`, `FUN_8003563C`, `FUN_80035F04` and the string
  helpers `FUN_8003CA78` / `FUN_8003CAC4` (`see
  ghidra/scripts/funcs/overlay_battle_action_801d8de8.txt`) - none reaches the
  effect pool. Element `0x59` is the Seru-absorb message and `0x65` the
  magic-level message, both rendering the context buffer `ctx + 0x1F9`
  ([below](#the-battle-message-banner-elements-0x59-and-0x65)). An earlier
  revision of this bullet called the argument an effect id and routed it into
  the effect pool; the port followed it and played an unrelated `efect.dat`
  script at every HUD raise.
- **`FUN_801DBF9C(party, spell_id)`** + **`FUN_801DC0A0(actor, anim_id)`** - chained from state `0x29` and `0x2A..0x2D` to drive spell visuals. These ultimately fan out to the [effect VM](effect-vm.md) which uses `FUN_801DFDF8` for the actual sprite-anim spawn.

So the effect dataflow is `FUN_801E295C` → `FUN_801DBF9C` / `FUN_801DC0A0` → effect VM (`FUN_801DE914` / `FUN_801E0088`) → `FUN_801DFDF8`; `FUN_801D8DE8` is the HUD's path, not this one. The callers of the pool spawner `FUN_801DFDF0` are the per-actor effect-script walk `FUN_801DEA50`, `FUN_801E09F8`, `FUN_801E22C8` and SCUS `FUN_8004998C` / `FUN_80047430`; the port routes the walk's requests through `World::route_battle_effect_spawns` on both hosts. Note this path drives the **2D UI/sprite** layer (`FUN_801DFDF8` emits `POLY_FT4` billboard quads into the effect pool); the 3D summon model is a separate mechanism (next).

### The battle message banner (elements `0x59` and `0x65`)

Two screen elements carry a sentence rather than a label: `0x59`, raised by the
Done band right after `FUN_801E92DC` teaches an absorbed Seru (`0x801E6240`),
and `0x65`, raised by the magic-level arm of `FUN_801E70BC` (`0x801E722C`).
Both render the same string: the result-message builder `FUN_801D84C0` points
each record's `+0x14` content word at the context's message buffer
(`sw v1,0x86c(a0)` / `sw v1,0x98c(a0)` at `0x801D850C` / `0x801D8514`, with
`a0 = 0x80076C10`). The two records share their geometry - seat A `(16, -24)`,
seat B `(16, 14)`, width 280, kind 3 - so a raise glides the framed line down
onto the top banner's pen and the unload glides it back out.

Who writes the buffer differs. The spawner's own `0x59` arm composes it on a
raise only (`bne s5,zero` at `0x801D914C`): `strcpy(ctx + 0x1F9,
prefix[char - 1])`, then the Seru's spell name (`0x800754C8[(ctx[+0x269] +
0x80) * 12 + 8]`), then a suffix (`0x801D9154..0x801D91D0`). The prefix table
`0x801F4DFC` is indexed by character and names that character's Ra-Seru
(parser `legaia_asset::absorb_caption`). `0x65`'s line is composed by
`FUN_801F452C` before its raise.

The port keeps the line on `World::battle.message_banner` from the raise to the
matching unload (`world::battle::message_banner`), and both play hosts draw it
through `engine-core::battle_hud::battle_banner_message` into the top banner
widget. A newly learned **art** is not announced here: retail's cue for it is
the `NEW ARTS!!` sprite banner the SpecialStarter `0x1A` commit raises
(`engine-vm::battle_action::flash_ramp`).

### `FUN_801F30C4` - the move VM's battle escape (op `0x17`)

A third spawn indirection, and the only one the action SM never touches: the
battle overlay's half of the move-VM extension pair. `FUN_80023070` case `0x17`
calls `FUN_801F30C4(actor, op[1])` exactly as case `0x2F` calls the field
overlay's `FUN_801D362C` - so `0x17` is battle-resident-only in the same sense
`0x2F` is field-resident-only.

Its `mode` operand takes `0` or `1` and nothing else. Either arm seats **twelve
child actors** through `FUN_80050ED4` → `FUN_80021B04`: four iterations round the
compass, three spawn blocks each, every child on one of two static move-VM stager
records in `0898`'s tail and carrying a per-child heading, a `+0x3E` value and a
`+0x98` value the burst computes from the trig LUTs plus bounded RNG jitter. The
two arms are the same loop written twice, differing in nine constants that
collapse to two exact relations. Byte-level decode, the two records, and the
18-byte trigger programs that fire each arm:
[`functions/battle.md`](../reference/functions/battle.md#801f30c4). Port:
`engine-vm::battle_burst`, wired through the engine's move-VM host (op `0x17`
queues the call) and the effect host (`FUN_801DFDF0`'s ids `4` / `0x13` seat
the trigger), both seated by `World::flush_battle_bursts`.

### The burning-body emitter at the tail of `FUN_8004998C`

The per-body anim decode `FUN_8004998C` ends in a second spawn loop
(`0x8004A5FC..0x8004A8D8`). The frame driver keeps an accumulator
`ctx[+0x328]` - low nibble kept, `DAT_1F800393 << 3` added every battle frame
(`FUN_80046A20`, `0x8004713C..0x80047160`) - and every body whose `+0x21F`
impact selector is non-zero spends it `0x10` at a time. Each pass picks a
random object of the body's current pose, turns its translation by the facing
`+0x46`, jitters each axis by `(r >> 4) - rand % (r >> 3)` with `r` the node's
`+0x58` size, and, when the point is at or above the floor (Y `<= 0`), hands
it to `FUN_801DFDF0`: effect `0x0B` (fire) for selector `1` once the node
colour's red lane reaches `0xB0`, effect `0x10` for selector `2` (which also
sets screen-shake globals the port does not model). This is the fire
`gimard_burning_attack` shows at the creature's mouth - two effect-`0x0B`
masters live at `(127, -317, -1732)` / `(133, -404, -1730)` beside the red
creature at `(143, -1606)` - and the fire a Tail-Fire-struck body sheds. Port:
`World::emit_battle_burn_sprites` over `engine-vm::battle_impact_fx`'s
`burn_effect` / `burn_emit_point`.

This path is disjoint from the `FUN_801D8DE8` / `FUN_801DBF9C` family above -
those spawn 2D billboard quads out of the effect pool, this seats full move-VM
actors - and from the summon-overlay dispatch below, which pages in code rather
than running bytecode.

### Seru-magic summon-overlay dispatch

The 3D visual of a player Seru-magic cast (the summoned Seru and its attack mesh - e.g. Gimard's *Burning Attack* flame) is **not** spawned by an opcode and does **not** live in `befect_data`. It is a **per-summon code overlay** paged in on demand. In outer state **case `0x29`**, when the queued action's spell id `actor[+0x1df]` is in the player Seru-magic block `0x81..0x8b`:

```c
_DAT_8007bd24[7] = 0x32;                                   // advance to the cast band
_DAT_8007ba2c = (&PTR_s_re_check_801f6734)[id - 0x81];     // the module's move-VM entry VA (a code pointer, called by op 0x20)
FUN_8003ec70(id - 0x79, 0);                                // overlay loader B: PROT (id - 0x79 + 0x381)
```

`FUN_8003EC70(param)` (overlay loader B) loads `FUN_8003E8A8(param + 0x381)` into
`*DAT_80010390` (= `0x801F69D8`, above the resident battle overlay) - which in **extraction
index space is PROT entry `param + 0x37F`** (the resolver indexes the raw in-RAM `PROT.DAT`
head, 2 entries above extraction indexing; see [formats/prot.md § In-RAM
TOC](../formats/prot.md#in-ram-toc)). So the summons map to extraction **PROT 903..913** (Gimard
*Burning Attack* `0x81` → param `8` → **PROT 903**; the earlier "905..915 / Gimard → 905" reading was
this off-by-2 - the per-spell attribution below it was arithmetic-derived, never
content-pinned). **The Gimard leg is capture-pinned**: the loader-B current-id global
(`gp+0x934` = `0x8007BC4C`) reads `8` → extraction **PROT 903** in all three catalogued
player-Gimard cast states (`gimard_summon_start` / `_visible` / `_burning_attack` - the
value sits in the save-state RAM, no live probe needed), and stays `8` through the whole
cast; the **enemy** Gimard "Fire Tail" frames instead hold `5` → extraction **PROT 0900** (the
move-FX module). **Enemy boss specials ride their own stagers through the same loader**:
the catalogued final-boss corpus (six Cort mid-cast states) lands every leg on the same
linear arithmetic, byte-resident at slot B `0x801F69D8` - Mystic Circle `0x2B` → **938**,
Mystic Shield `0x2D` → **940**, Guilty Cross `0x31` → **944**, evolved-form Final Crisis
`0x42` → **961** and Ultra Charge `0x43` → **962**, and Cort's Evil Seru Magic `0x47` →
**966** - the last **distinct from the player-side Juggernaut stager 0927** (loader id
`0x20`): the player and enemy arms of the same spell ship separate stagers. So the
enemy-special id band `0x2B..0x47` maps to extraction **938..966**, while small ids
(`5`/`6` → 0900/0901) are the move-FX / widget modules streaming through the same slot. The
capture-class (`'c'`) spell branch loads from a different base:
`FUN_8003EC70(spell_record[+1] + 0x28)`. **The whole block is capture-pinned**: every spell
id `0x81..=0x8B` was observed mid-cast loading its arithmetic slot (`903..=913`), with zero
exceptions. PROT 0907 on the spell-`0x85` slot is **Nighto's stager** - its head title
"Hell's Music" is the attack's display name (the SCUS spell table carries the same string),
not a Disco King dance song (that reading is refuted: the dance overlay, 0980, contains no
slot-B loader callsite - its music is sequenced BGM). See
[`static-overlay-pipeline.md`](../tooling/static-overlay-pipeline.md).

#### Inside a summon overlay (extraction PROT 905, decoded)

> The deep-dive below analyzes the **extraction-905 file** - under the corrected loader arithmetic that is the spell-`0x83` slot, *not* Gimard's (`0x81` → 903, which parses identically as a stager under the same link base, and is now capture-pinned as the Gimard load via the loader-B current-id in the catalogued cast states). The file-level findings stand for the 905 file itself; the live-capture findings (flame mesh `DAT_8007C018[26]`, part-actor motion) are capture-derived and independent.
> The per-spell file attributions for the whole block (`0x81..=0x8B` → `903..=913`) are capture-pinned from per-spell mid-cast states. **Parse counts quoted for any stager must come from the entry trimmed to its TOC-gap footprint** (see [the trim subsection below](#enemy-boss-stagers--the-record-table-trim)); untrimmed extraction files over-read into the neighbouring stagers and inflate the spawn-site/record census.

The summon overlay carries **no embedded TMD geometry** (no `0x80000002` magic). The summon's meshes are the separately-loaded `DAT_8007C018` model library: **PROT entry 871** (`etmd.dat`), a 30-entry `asset::pack` of Legaia TMDs that the battle scene loader `FUN_800520F0` pulls at battle init (raw TOC index `0x369`, dev path `h:\prot\battle\etmd.dat`) and registers via `FUN_80026B4C`, populating `DAT_8007C018[3..32]` (`[0..2]` are the party battle meshes). Despite its CDNAME label `sound_data`, PROT 871 is the effect-model library; its texture sibling PROT 870 (a 256×256 flame-frame atlas, also `sound_data`) is loaded by a separate path. The overlay spawns and animates part-actors over those meshes. **Decompiled** (PROT 905 imported raw at base `0x801F0000`,
`ghidra/scripts/dump_summon_overlay.py`):

- The overlay spawns part-actors via the SCUS part-stager **`FUN_80021B04(world_pos, render_slots, record_ptr, 0x1000)`** (`param_1` = world position written to `actor[+0x14..0x18]`, `param_3` = a part record, allocated from the effect pool `DAT_8007062c`) - either directly, or through the thin pool wrapper **`FUN_80050ED4`** (stores the spawned actor pointer in the first free slot of the 0x60-pointer pool at `DAT_801C90F0`, then forwards the same arguments; the dominant call form in the high-summon and enemy boss stagers, `see ghidra/scripts/funcs/80050ed4.txt`). The
  same stagers flush what they seated through **`FUN_80050E74`** - see
  [`functions/game-modes.md`](../reference/functions/game-modes.md#move--animation-subsystem) for the per-slot writes and why it is not
  the same walk as the battle-teardown loop in `FUN_800480D8`.
  `record[+0]` (`model_sel`) drives the spawn-time render seat: `≥ 0` → library mesh `DAT_8007C018[model_sel + gp[0x754]]` (`actor[+0x5A] = 1`), any negative value (`-1` canonical) = no-mesh transform/pivot node (`actor[+0x56] = 0`, `actor[+0x5A] = 0`, draw-flag bit 2), `0x4000`/`0x4001` = special render-mode nodes (`actor[+0x5A] = 3` / `5`).
- Three staging functions drive the spawn: **`FUN_801F16A0`** (phase 0 = a `do { FUN_80021B04(...) } while(< 8)` loop spawning **8** flame parts, each with `rand()`-seeded actor params - `actor[+0x84]`, `actor[+0xb4] = rng%15 + 16`, `actor[+0xb6] = rng%255 + 512`, `actor[+0x28]`; phase 1 = 1 more part), **`FUN_801F36A0`**, **`FUN_801F4DD0`**. The per-frame motion is the standard actor-tick consuming those RNG-seeded fields.
- **Part records ARE in-file and move-VM bytecode (corrected link base).** Under the correct link base `0x801F69D8` (not `0x801F0000`), each `FUN_80021B04` call's record pointer resolves to PROT 905 **file `0x180C..0x1E00`** - a contiguous table of `[i16 model_sel][u16 reserved][move-VM bytecode @+4]` records, recovered by `legaia_asset::summon_overlay` (disc-gated `summon_overlay_real`). This **supersedes** the two earlier wrong-link-base "FALSIFIED" readings - "the records are beyond the `0x5800` file / `0x180C` is only coincidentally record-shaped / parser reverted" and "there is no move VM here." The records *are* move-VM bytecode;
  the reason PROT 905 has zero `jal 0x80023070` *inside the overlay* is simply that the `jal` lives in the SCUS stager `FUN_80021B04` (which seats `actor[+0x70] = 2` PC → bytecode at `record+4`, then ticks `FUN_80023070`), not in the overlay image.
- **But the move-VM scene-graph is NOT how retail renders the player summon
  (live trace).** A PCSX-Redux trace of a player Gimard *Burning Attack* cast
  shows `FUN_801F7088` = **0×**, the move VM `FUN_80023070` = **2-3×** (noise),
  and the **battle per-actor draw `FUN_80048A08`** in exact lockstep with the
  per-object rigid-TRS keyframe decoder `FUN_8004998C` → cluster-A
  `FUN_80043390`. Re-measured on `gimard_burning_attack` (400 vsyncs,
  `scripts/pcsx-redux/autorun_enemy_move_render_path.lua`): 213 hits each, never
  more than one per live actor per rendered frame - the "35-64×/frame" magnitude
  an earlier revision quoted does not reproduce. So the **player** summon is
  drawn as an ordinary battle actor (per-object TRS keyframes), the faithful
  path being `engine-vm/anim_vm.rs` (`FUN_80048A08` / `FUN_8004998C`). The
  move-VM stager records still exist, but they aren't the player summon's
  per-frame render path: the engine seats every player summon's own body as a
  battle creature with its keyframe clips (`engine-core::summon::summon_spawn_asset`,
  on both hosts), and `summon::SummonScene` remains only for the move-FX and
  cast-module effect paths. SCOPE: the trace covers the **player** "Burning Attack"
  only;
  the **enemy** Gimard *Fire Tail* boss move is a distinct path - see the Fire-Tail note below.

The flame renders as Gouraud-textured (`POLY_GT3`/`POLY_GT4`) prims sampling the resident `etim` page (832,256) 4bpp; `cba`/`tsb` are applied at render.

- In a live Tail-Fire capture the summon library occupies `DAT_8007C018[3..32]`; ten of those (`[23..32]`) are fire-textured meshes (cba row 478 `0x778B` baked), and the **active Gimard flame is `DAT_8007C018[26]`** - the only rendered model baking etim, with both rendering actors carrying `actor[+0x64]=26` and `actor[+0x56]=5` (full-TMD mode → `FUN_8002735C`).
- Each individual flame mesh is **static geometry**; the visible fire motion is the **spawned part-actors** moving (the 8 RNG-seeded parts above), **not** CLUT cycling - the entire CLUT band is byte-identical across two animation-distinct `battle_gimard_tail_fire_a/_b` frames while the framebuffer differs ~21% (this falsifies the earlier "fire flicker = CLUT/palette animation" reading).
- The PROT 905 `LoadImage` (`FUN_800583C8`) CLUT uploads target VRAM row `481+` (the character/party-CLUT region), conditionally, not the flame's row 478.
- The part records are recovered (`legaia_asset::summon_overlay`). The **player** summon renders through the battle TRS-keyframe path (`FUN_80048A08` / `FUN_8004998C`), with each summon's own body and clips seated by `summon_spawn_asset` on both hosts.

##### Enemy "Fire Tail" - move-VM part, not the widget path

**The retail string is `Tail Fire`.** Spell id `0x27` in the static SCUS spell
table reads `Tail Fire` (`asset spell-names`), and the monster archive names the
same move that way in the enemy Gimard's spell list; "Fire Tail" is this
section's own long-standing nickname, kept here only because other pages link
its anchor. Do not confuse it with the *player* summon `0x81` (`Gimard`), whose
attack is `Burning Attack` - a different move on a different path.

The **enemy** Gimard *Fire Tail* boss move is the distinct path the player-summon
trace did not cover, and it is now characterized from the two catalogued
mid-cast frames (`battle_gimard_tail_fire_a/_b`; disc + library gated
`firetail_movefx_liveness`). Unlike the player summons and the Cort/Delilas/Zeto
boss specials - which page a per-spell *stager* into slot B - Fire Tail's slot-B
occupant is the move-FX module **PROT 0900** itself (loader-B id `5`, byte-exact
at the residency pin file `0x1628` ↔ `0x801F8000`).
But PROT 0900's **screen-widget family is dormant**: an effect-actor-list walk of
both frames finds **zero** live mask/sprite/panel/letterbox widgets - so Fire
Tail is not the cutscene widget path (that stays exclusive to the ten ending
scenes; see [`move-vm.md` § screen-effect widget family](move-vm.md#screen-effect-widget-family-prot-0900)).
The live effect is instead a single **move-VM part-actor** in the part pool
`DAT_801C90F0`, ticked per frame by the generic SCUS actor tick `FUN_80021DF4`
(→ `FUN_80023070`) - a live capture pinning that render-tail driver. Its
`[i16 model_sel][u16 reserved][bytecode]` record (`actor[+0x48]`) lives in the
**battle overlay (0898)** resident data at `0x801F5xxx` (below the 0900 slot-B
link base `0x801F69D8`), `model_sel` reading `-1` (transform node) / `5` (library
mesh `DAT_8007C018[5 + base]`) - the summon part-record format, sourced from the
battle overlay rather than a stager. So the move-VM scene-graph *is* Fire Tail's
render path (one live part), but its records are battle-overlay data and PROT
0900's role there is resident move-FX code, not the live driver.

#### Enemy boss stagers + the record-table trim

The six final-boss Cort special-attack stagers - extraction PROT **0938** (Mystic Circle), **0940** (Mystic Shield), **0944** (Guilty Cross), **0961** (Final Crisis), **0962** (Ultra Charge), **0966** (Evil Seru Magic; distinct from the player Juggernaut stager 0927) - parse as summon stagers under the same `0x801F69D8` link base and record format as the player block (`summon_overlay::ENEMY_BOSS_STAGER_PROT`; disc-gated `enemy_stager_real`). They spawn dominantly through the `FUN_80050ED4` pool wrapper rather than direct `FUN_80021B04` calls.

**The enemy-cast stager path is not Cort-specific.** Mid-cast captures of ordinary bosses pin the same mechanism on the universal `extraction = id + 895` arithmetic (loader-B current-id at `0x8007BC4C`, byte-resident at slot B; disc+library-gated `enemy_stager_binding`): the Delilas brothers - Gi / Blazing Slash `0x3F → 0958`, Che / Megaton Press `0x40 → 0959`, Lu / Plasma Strike `0x41 → 0960` - and Zeto, whose Call Wave and Big Wave are one logical attack over two turns and so share a single stager (`0x33 → 0946`). None of these four carries a `0x4000` render-mode record, and at the captured instants the part pool `DAT_801C90F0` is empty (no live part seated) - so the render-mode draw still has no live exerciser.

**Stager extraction entries are over-read windows.** The TOC-indexed footprint of every stager entry runs past the next entry's start LBA, so an extraction `.BIN` is `[this stager][the following stagers' bytes...]`; only the first `(next_start_lba - start_lba) * 0x800` bytes are the entry's own content (`summon_overlay::unique_content_len`).
The Cort mid-cast saves pin the boundary byte-exactly: each state's slot-B resident image matches its stager file up to precisely the TOC gap (0938 → `0x1800`, 0940/0944/0961 → `0x2000`, 0962 → `0x2800`, 0966 → `0x4000`) and diverges after it (stale bytes of the slot's previous occupant). Spawn sites in the over-read tail belong to *neighbouring* stagers, and their `lui/addiu` record pointers - valid only for the neighbour's own load at the shared base - dereference unrelated bytes in the wrong file window.

**That trim resolves the record-first-word "sentinel" question.** Across every trimmed stager (player 0903..=0913, the evolved-Seru block 0914..=0923, high 0927..=0934, the six Cort entries) the first word is only ever `-1` (transform node, dominant), a small library-mesh index, or **`0x4000`** - matching `FUN_80021B04`'s own dispatch exactly (negative → transform path, `0x4000`/`0x4001` → render-mode nodes, else library index). The previously-reported `0x1000`/`0x8000`-class sentinel population was the over-read artifact.

**Render-mode-node census (`0x4000`).** A static sweep of the trimmed stager
corpus (disc-gated `summon_overlay_block`) finds `0x4000` records in **five**
stagers: the three Sim-Seru high casts Palma (0928, 4 records), Mule (0929),
Jedo (0931), **plus two evolved-Seru player casts** - spell `0x8E` → 0916
(4 records) and `0x93` → 0921 (6). The evolved-Seru block (`spell_id
0x8C..=0x95` → extraction 0914..=0923, `summon_overlay::EVOLVED_SUMMON_STAGER_PROT`)
is the contiguous continuation of the player block under the same linear loader
arithmetic (`extraction = (id - 0x81) + 903`); every entry trims to a clean
move-VM stager, so the evolved casts ride the stager mechanism. **Eight of the
ten legs are capture-pinned** (`0x8C..=0x8F` → 914..917, `0x92..=0x95` →
920..923; one mid-cast state each, loader-B id read mid-cast + the stager 100%
byte-resident at slot B - disc+library-gated `evolved_summon_binding`); only
`0x90 → 918` / `0x91 → 919` stay arithmetic-predicted. **Both render-mode
carriers are pinned as player casts** - `0x8E → 916` (Aluru) and `0x93 → 921`
(Iota) - so neither unblocks the live-exerciser question below (a player cast
renders the namesake creature, never seats the stager parts). The two flanking
blocks carry the same byte-pin oracle: the base block `0x82..=0x8B` → 904..913
and the high block `0x99..=0xA0` → 927..934 each byte-pin one mid-cast state
per leg (loader-B id + slot-B-resident stager; disc+library-gated
`summon_binding_base_high`), so `0x82..=0x95` (minus the two predicted evolved
legs) and `0x99..=0xA0` are all regression-covered against real RAM.
Live correlation from the Cort states: every live pooled part-actor (`DAT_801C90F0` slots) carries `actor[+0x48]` pointing into the trimmed record table at a `-1` record (RAM first word == file first word), with the spawn-time `+0x56`/`+0x5A` zeros rebound post-spawn by the move-VM ops (`+0x56 = 4` / `+0x5A = 2` dominate mid-cast) and `actor[+0x64] = 0` throughout. No `0x4000`/`0x4001` part-actor was live in these captures.

**The render-mode nodes have no live exerciser in the catalogued corpus.**
For the three player Sim-Seru casts in the mid-cast save corpus whose stagers
*carry* `0x4000` records - Palma (0928), Mule (0929), Jedo (0931) -
a pointer-scan of each state's full RAM finds **zero** words referencing
any of the stager's record starts (or their `record+4` bytecode entries), even
though the stager is 99.9–100% byte-resident at slot B. So in a player cast the
move-VM scene-graph is not live at the on-screen instant at all - the summon
renders as its namesake `battle_data` creature through the monster animation
pipeline (the player-summon correction), and the stager part-actors (including
any `0x4000` node) are already gone. The Cort *enemy* path does run live stager
parts but holds only `-1` nodes. Pinning the `0x4000`/`0x4001` draw behaviour
therefore needs a frame-stepped capture inside an *enemy* stager-spawn window
whose stager carries a `0x4000` record - not reachable from the catalogued
states (`crates/mednafen/tests/summon_render_mode_node.rs`).

### `FUN_801D5854` - per-actor pose driver

The single most-cited helper inside `FUN_801E295C` (~30 call sites). Signature `FUN_801D5854(actor_id, pose_id)`. Pose IDs surfaced:
- `6` = idle / breathing
- `7` = ready / pre-action
- `8` = action-end / hit-recovery
- `9` = defeat / down

It is a **camera/presentation program driver**, not the animation system: its body dispatches `pose_id` 0..9 through a jump table at `0x801CEA00` computing three i16[3] tween-target vectors handed to `0x801D7130` (with a secondary dispatch on `actor[+0x1DB]` values `0x11..0x18` - per-art camera variants for the dynamically-installed art anims). It never writes `+0x1D9/+0x1DA`; the same-numbered **anim** ids 7/8/9 are staged separately (by the SM's own `+0x1DA` stores and the `FUN_8004AD80` end-of-clip chains), and the anim system's idle id is `0` - pose 6 has no anim counterpart (record[0] entry 6 is empty in every player file). The two id spaces are designed to align numerically at 7/8/9, which is what made the conflated reading stick.

`FUN_801D5854` opens with an **out-of-range guard** (`0x801D58C8..0x801D58E8`, `param_1` = `a0` = actor slot in `s5`, `param_2` = `a1` = pose id in `s4`): when `param_2 >= 6` *and* `param_1 >= 8` - i.e. a real pose requested for a slot outside the 8-entry pool - it forces `param_2 = 9` and calls `FUN_801DB9C4`, which scrubs the `+0x8` flag word across the pool. It is a defensive path, not the run-side animation lookup an earlier reading called it, and the operands are `>= 6` / `>= 8`, not `== 9` / `== 7`.

#### Case `0` - the submenu close-up framing

Pose `0` is the per-character command-menu close-up, called as `FUN_801D5854(actor_slot, 0)` from `FUN_801D388C`. Every component is a constant or a function of the acting actor; there is no per-seat table and no `base + seat * delta` angle law:

| Slot | Value | Kind |
|---|---|---|
| pitch | `0x20` | constant |
| yaw | `0x8F0 - actor[+0x46]` | facing-relative |
| TR.x | `-0x200` | constant |
| TR.y | `[0x801F4D2C + (char_id - 1) * 2]` | per-character height |
| TR.z | `0x600` (prescaled) | constant |
| focus | `-actor[+0x34/+0x36/+0x38]` | negated world position |
| duration | `0xC` = 12 frames | 6 camera steps x 2 vsyncs |

The battle actor pointer table is `0x801C9370`, indexed by slot (sibling of the `0x801C9360` arts-gauge table). `char_id = DAT_8007BD10[slot]` is the 1-based party-record selector, so TR.y keys on **character identity** (Vahn / Noa / Gala / Terra), not on seat - a per-model height offset. The table holds one entry per playable character; it is static overlay data, parsed off the disc by `legaia_asset::battle_camera_table` rather than transcribed, and installed on the world at scene entry. Vahn's entry is `0x480` = 1152, the value the solo-Vahn camera trace observes - which is what anchors the table's base and stride to the measurement.

A yaw of `2288` measured on a solo-Vahn fight is therefore not a seat constant: it is `0x8F0` with Vahn's battle facing of `0` subtracted, and `FUN_801E7824` resetting `actor[+0x46] = 0` is what makes that facing `0`. The framing is a fixed over-the-shoulder offset that generalizes to any seat once facing is tracked. The **per-seat variation lives entirely in the focus trio** (`0x80089118/1C/20`): the camera orbits about whichever actor is acting. With one party member that is indistinguishable from a constant, which is why a solo trace reads as a single fixed pose.

`TR.z` is the one prescaled slot - see [`FUN_801D829C`](#fun_801d829c---camera-angle-tween-prescale) below. Case `3` is the same shape with `0x900 - actor[+0x46]`, a second close-up `0x10` units round from this one.

#### Case `9` - the far Begin/Run framing

Pose `9` is the wide menu framing. Its depth and focus are **computed from the live formation**, so - like case `0`'s yaw - none of it is a magic number:

| Slot | Value | Kind |
|---|---|---|
| pitch | `0x20` | constant |
| yaw | `_DAT_8007B792` | passed through - the idle orbit owns it |
| TR.x / TR.y | `0`, `0x500` | constants |
| TR.z | `max(span * 3, 0x800)` (prescaled) | formation-sized depth |
| focus | `-(bbox centre)` | formation centre |
| duration | `0xE` = 14 frames | 7 camera steps x 2 vsyncs |

The builder walks a slot range selected by the framing argument (`0` = the whole field, `1` = enemies only, `2` = party only), skipping actors whose presence halfword `actor[+0x14c]` is zero, and accumulates `min`/`max` of `actor[+0x34]` (X) and `actor[+0x38]` (Z). `span` is the **larger** of the two extents, so a wide-but-shallow line frames on its width. The walk's slot mapping folds the party and enemy blocks together: on reaching the party count it jumps to slot 3, the first enemy slot.

The far framing's traced `TR.z` of `7680` is `prescale(0x12C0)`, i.e. `span = 1600`. That is a measurement of one formation, not a constant - and it is reproduced independently by the retail seat tables: the traced fight is a solo Vahn (party row 1, seat `z = -800`) against one monster (monster row 1, seat `z = +800`), a Z span of exactly `1600`. A three-member party frames wider.

#### `FUN_801D829C` - camera angle-tween prescale

The angle-tween builder takes three caller buffers of 3 x `i16` (rotation trio `0x8007B790/92/94`, translation trio `0x800840B8/BC/C0`, focus trio `0x80089118/1C/20`) plus a frame count. It rewrites **slot 5 only** - `TR.z` - as `(z << 8) / 0xA0`, converting a world-space camera distance into GTE projection units (`0xA0` = 160 = PSX screen half-width, `<< 8` = GTE `H = 256`).

The divide truncates, which is the fingerprint to look for: traced `TR.z` values are floors of a round raw, not exact divides. `0x400 -> 1638`, `0x600 -> 2457`, `0x800 -> 3276`.

The fourth argument is a **frame count**, not a speed - the stored word is the per-frame increment and the tween lasts that many vsyncs. The submenu call passes `0xC` (12 frames = the 6 measured camera steps at 2 vsyncs each); the action-camera sites pass `1` (instant cut) and `0x30`. Under a speed reading the submenu tween would take 436 steps.

The engine port of the framing rules lives at `crates/engine-shell/src/window/battle_cam.rs` (`BattleCamActor::submenu_pose` for case `0`, `menu_framing` for case `9`); the fixed-point tween kernel is `legaia_engine_vm::battle_camera`. The port tweens the focus trio on the same clock as the rotation and translation trios, and the window camera consumes it as the look-at target, so a non-Vahn seat frames on the acting member rather than on the formation centre.

### `FUN_801EED1C` / `FUN_801E7320` - party / monster setup hooks

Called from state `0x0C`:
- Party (`actor_id < 3`): `FUN_801EED1C()` - initialises per-character action data.
- Monster with AI flag (`+0x16E & 0x380 != 0`): `FUN_801E7320()` - initialises monster-AI action.
- Otherwise: neither - actor inherits from previous frame.

### `FUN_801EFE44` - battle camera bounds

Called from state `0x0C` for non-flee actions. Walks the 8-slot actor table computing min/max X and Z to set the battle camera's frustum. Read-only with respect to the action state machine; pure rendering helper.

### The escape roll (`FUN_801E791C`)

Called by state `0x64` to decide a retail flee. It is the writer of `_DAT_8007726C` - the
battle-message source pointer states `0x64`/`0x65` test: `ctx + 0x159` ("escaped" text) on
success, `ctx + 0x189` ("couldn't escape") on failure. From the dump
(`ghidra/scripts/funcs/overlay_battle_action_801e791c.txt`):

```
party_score = Σ_party  (SPD*3)>>1 + (maxHP - curHP)>>4    ; actor +0x164 / +0x14E / +0x14C
enemy_score = Σ_enemy   SPD      + (maxHP - curHP)>>5
roll_p = rand() % party_score ;  roll_e = rand() % enemy_score
if Escape Boost (ability bit 52):                 roll_p += roll_p >> 1
if Great Escape (bit 55) or ctx[+0x291] == 2
   or (_DAT_8007BAC0 & 0x100):                    roll_p = roll_e
FAIL iff  !(_DAT_8007BAC0 & 0x100)
          && (roll_p < roll_e  ||  ctx[+0x287] != 0)
```

Both sides run faster the more hurt they are (missing HP raises the score) and the party's
SPD is weighted 1.5x against the enemies' 1x; every slot contributes, downed members
included. The two ability bits are read from the *living* party members' second
accessory-passive word (character record `+0xF8`): bit 52 = passive `0x34` **Escape Boost**
(Chicken Heart, roll x1.5), bit 55 = passive `0x37` **Great Escape** (Chicken King) - the
assured bit forces the party roll equal to the enemy roll so the compare cannot fail, but
the scripted no-escape flag `ctx[+0x287]` is tested *after* that (`0x801E7AF0` sets the tie,
`0x801E7B14` reads `+0x287`) and still blocks it - "assured" describes only the compare, never
the outcome, which is why Chicken King is "assured escape (non-boss)" (see the
[accessory-passive table](../formats/accessory-passive-table.md)). The battle flag
`_DAT_8007BAC0 & 0x100` forces the flee outright - it bypasses even `ctx[+0x287]` and skips
the "No. of Escapes" Records counter (`_DAT_800846A8`) the normal success path increments.
The bit is folded inside the party loop, once per living member (`0x801E7978`, the `s1 = 2`
store at `0x801E7A14`); `World::roll_battle_escape` folds it the same way, and the engine keeps
no escape counter.

**Both ctx inputs are written at battle setup, not by the roll.** `ctx[+0x287]` (the
[scripted-fight flag](#ctx0x287-is-the-scripted-fight-flag-and-0x288-is-the-lone-monster-defeat-latch),
also read by the state-`0x20` reaction hold's bypass) is latched by the SCUS
battle-setup routine `FUN_800513F0` in its first instructions: `ctx[+0x287] = (DAT_8007BD60 >> 5)
& 4` - it carries bit `0x80` of the battle-flags byte `DAT_8007BD60` (the same byte state `0x5A`
masks with `&= 0x7F`), so a scripted "can't run" fight sets it to `4` at load (`0x801E5058` reads
it; `see ghidra/scripts/funcs/800513f0.txt`). `ctx[+0x291]` is not written directly - it is a
**latch** of `ctx[+0x290]`: the SM's state-`0x00` action-begin does `ctx[+0x291] = ctx[+0x290]`
then clears `+0x290` (`0x801E2B38`). `ctx[+0x290]` itself is written by the formation-setup
routine `FUN_80051D84` - `1` under a monster-id-range test, or `2` on a `func_0x80056798()`
(BIOS-rand) roll - so `ctx[+0x291] == 2` is a per-formation flag set at battle setup (`see
ghidra/scripts/funcs/80051d84.txt`) that reaches the *same* forced-tie store as the Great Escape
bit, and carries the same caveat: it makes the compare unfailable, not the escape certain, since
`ctx[+0x287]` is read afterwards. Note also that `1` (back attack) is never compared here - the
roll only ever tests against `2`, so a back attack costs the party its round-one initiative keys
and nothing else. Because the roll reads only the *latched* copy, a state-`0x00` that clears
`+0x290` without copying it - or an engine that stores the latch and never reads it back -
silently disables pre-emptive-strike escapes for the whole battle. The latch
also runs **every round**, so round two's pass copies the `+0x290` round one
cleared: a pre-emptive strike's unfailable escape compare lasts one round.

On success the routine also stages the flee scene (`0x801E7B98..0x801E8030`):

- every party actor stages the looping walk (`+0x1DA = 1`, `+0x1DC = 1`), turns its back on
  the fight (facing `+0x46 = 0x800`) and takes target `+0x1DD = 9` - a group code the range
  law `FUN_8004E2F0` reads as out of range, so the walk's root motion carries the member
  away until the battle ends;
- live `x` is halved and `z` quartered, then the group is re-centred on `(0, 0x400)` and
  every pair closer than 200 units in `x` is pushed apart by half the shortfall each;
- live HP/MP are written back to the character records with downed members **floored at
  1 HP** (the record-side half of the state-`0x64` floor);
- the live camera is snapped to yaw `0xF00`, TR `(0, 0x600, 0x2000)`, focus at the origin,
  and `FUN_801D829C` tweens it over `0x30` frames to the reverse angle: yaw `0x800`, TR
  `(0, 0x600, 0)`, focus on the regrouped party. Nothing re-frames it before the battle
  ends.

Ported: `engine-vm::battle_formulas::escape_roll` (+ `escape_party_score` /
`escape_enemy_score` / `EscapeFlags`), rolled live by `engine-core::World::roll_battle_escape`
when the command menu resolves Run; the staging is `World::stage_party_flee` and the shot
`BattleCamera::arm_escape_shot`. The port stages only the members still standing - a downed
one stays where it fell.

### The state-`0x20` reaction hold

Once the attacker's own last clip has ended, `0x20` also waits out the
**target's** reaction (`0x801E54FC..0x801E5580`), so a flinch, a knockdown and
its get-up, or a death and its fade all play before the Done band's countdown
starts. The target is `s8`, the actor at the attacker's `+0x1DD` (loaded at
`0x801E29CC`, and left unloaded for a group target code `>= 8`). The band holds
while all three read true:

| test | instructions | releases when |
|---|---|---|
| target committed anim `+0x1D9 != 0` | `lbu v0,0x1d9(s8)` / `beq v0,zero` at `0x801E5520..0x801E5528` | the target is back on idle |
| not a party target on entry `8` | `sltiu v0,t2,0x3`, `beq v1,v0` with `v0 = 8` at `0x801E5504..0x801E5518` | a downed party member reaches its downed loop (`4 -> 7 -> 8`) |
| render node still drawn | `lw v0,0x74(*(s8+0x22C))`, `& 0xFFFFFF` at `0x801E5530..0x801E5544` | a dead monster's defeat fade has walked it to black |

A dead monster holds its knockdown frame through the fade, so the third test is
what ends a killing blow's hold. One bypass lets the band out with the target
still reacting: `ctx[+0x287] != 0 && 0x8007BD0D == 0 && ctx[+0x288] != 0`
(`0x801E554C..0x801E557C`) - a scripted lone monster whose defeat fade has
raised the [latch](#ctx0x287-is-the-scripted-fight-flag-and-0x288-is-the-lone-monster-defeat-latch).
Both exits, and the target-idle one, take the same `0x50` store at
`0x801E5588`, which falls into the monster's KO taunt (tag `0x22`). The
`player_steal_skeleton_banner` capture is this hold seen from outside: `ctx[7]
== 0x20` with the attacker's clip already `0` and the killed skeleton on its
knockdown.

Port: `battle_action::attack`'s `target_reaction_holds`, reading the target
through `BattleActionHost::reaction_hold_view` (the engine plays reactions on a
side channel, so its host merges that channel into the committed id); the latch
is raised by `World::tick_battle_defeat_sink`. One engine choice sits beside
it: a target whose animation rate `+0x21D` reads `0` - frozen by a starter
commit that no art commit thawed - does not hold the band, since its clip
cannot advance before the Done band restores the rates.

### `ctx[+0x287]` is the scripted-fight flag, and `+0x288` is the lone-monster defeat latch

The two bytes are adjacent and they are read together at the state-`0x20`
reaction hold, which is how they came to be described as one counter-attack
thing. Neither is one:

- **`ctx[+0x287]`** is a per-**battle** property, derived once at battle init
  and never written again during the fight - `(DAT_8007BD60 >> 5) & 4`, i.e.
  bit `0x80` of the formation's per-battle flags byte. Everything it gates is
  "is this a scripted fight": the escape roll above, the two magic-capture
  audio-duck arms (states `0x6F` / `0x70`), the defeat fade's floor sink and
  the reaction hold's bypass.
- **`ctx[+0x288]`** has one writer, the tint SM's defeat-fade arm
  `FUN_80050120` (`sb s4,0x288(v1)` with `s4 = 1` at `0x800504E8`): a monster
  seat fading out on render flag `2`, still drawn (`node[+0x74] & 0xFFFFFF`),
  not captured (`+0x225`), no Seru absorb staged (`ctx[+0x269]`), in a scripted
  fight whose formation has no second monster (`gp+0x9F5` = `0x8007BD0D`
  zero). That same predicate skips the arm's floor sink
  (`0x80050444..0x8005045C`), so the latch reads "the lone scripted monster is
  dying in place". Its readers are the reaction hold (`0x801E5574`) and the
  battle camera's case 8 (`0x801D6AC8`); the Done band's menu arm clears it
  (`0x801E6114`).

The distinction is load-bearing for the port rather than cosmetic. An engine
that seeds `+0x287` per *action* leaves the two duck arms and the attack-return
arm permanently unreachable, because an action-scoped byte is zero at the
moments a battle-scoped one is set; seeding it from the formation row's own
flags is what makes those arms run. The port derives it at battle entry from
`FormationDef::per_battle_flags()`
([battle-formulas.md](battle-formulas.md#seru-magic-side-effects---the-element-debuffs-fun_801f3d3c--the-finisher-switch)).

### The `_DAT_8007B910` ramps are an audio duck

States `0x35` (summon sustain), `0x51` (done) and `0x6F` / `0x70` (magic
capture) ramp `_DAT_8007B910` against the reference `_DAT_8008457C`. That cell
is the **live audio level**, not screen brightness, so these arms duck the mix
under a summon and restore it afterwards.

Every one of its readers is a volume setter. `FUN_800267A8` narrows it
(`<< 15`, then arithmetic `>> 16`, i.e. halve) into `FUN_80062004`, which is
`SsSeqSetVol(slot, channel 0, vol, …)` via `FUN_80061EDC`; `FUN_80026478` hands
the same halved value to `FUN_8002657C`, which writes it as **both** channels of
`FUN_80064890(slot, vol_l, vol_r)` - symmetric, so not a pan either; four sites
build an `SpuCommonAttr` on the stack with the cell in the CD-volume pair and
call `SpuSetCommonAttr` (`FUN_8006BCB4`); and the cold reset `FUN_8001FFA4`
seeds it `0xD7` beside its persistent reference and immediately calls the
audio-context volume re-apply `FUN_8002614C(0)`. Across the whole dumped corpus
the cell has 26 read sites and **none** reaches a draw primitive.

The screen fade is a different scalar: `_DAT_8007B440`, ramped by the function
at VA `0x801ED308` **in the menu/cutscene overlay family** (`see
ghidra/scripts/funcs/801ed308.txt` - own prologue, `jal 0x8003479c` at
`0x801ED3F0`/`0x801ED4BC`/`0x801ED510`) and drawn each frame by the wipe/curtain
emitter `FUN_8003479C` (clamped `0xF2`). Name the overlay when citing it: in the
**battle** image that VA is interior to `FUN_801EC3E4` (the underdog-rewrite
arm's power-scalar read), which touches `_DAT_8007B440` nowhere - the aliasing
class the dump-aliasing caution below covers. The two were conflated because they ramp together - the summon
dims the screen *and* ducks the music. Port: `BattleActionHost::duck_audio_level`
→ `BattleEvent::DuckAudioLevel` (`75` from the summon / capture arms, `100` once
on the `0x50 -> 0x51` transition); the native window's `AudioBgmDirector`
mirrors the cell (`duck_level`, seeded `0xD7`), ramps it one unit per frame
toward the target (`tick_duck`) and re-applies it through
`AudioOut::set_sequencer_master_vol`. The browser play page consumes the same
event through `play_battle_audio::drain_battle_audio_cues`, which is the one
typed battle event a host's audio reads (the live loop owns the gameplay fold).

### Battle voice cues - the XA30 grunt vs the XA2/XA4/XA6 arts shout

Legaia's battle voices are **XA stream cues, not SPU samples**. There are two distinct
per-character voice cues, each fired through the SCUS clip player
`FUN_8003D53C(clip_slot, channel, dur)` (the runtime clip table at `0x801C6ED8` follows
`slot i` = `XA<i+1>`, see [cutscene.md](cutscene.md); the sequencer `FUN_8003D764` runs
`CdlSetloc` + `CdlSetfilter{file 1, chan}` + `CdlReadS`, and `dur` converts to an absolute CD
stop position `end = start + (dur * 0x96 + 0x95) / 0x3c`, a physical span `~dur * 2.5` sectors;
`see ghidra/scripts/funcs/8003d53c.txt`).

**1. Normal-move grunt (`XA30.XA`).** The battle-action overlay's input handler around
`0x801EEB44` (`see ghidra/scripts/funcs/overlay_battle_action_801ec3e4.txt`) reads the acting
slot's 1-based character id from `DAT_8007BD10[slot]` and fires `FUN_8003D53C(0x1D, chan, dur)`
(clip slot `0x1D` = `XA/XA30.XA`) with a per-character channel: Vahn chan 0 (`dur 0x26`), Noa
chan 4 (`0x2E`), Gala chan 6 (`0x1A`). Each XA30 hero channel is one clean ~0.4-0.7 s
vocalization. It is **not** what every swing plays: the cue is gated on the defender
committing the `+0x1F3` reaction pose, and a swing that commits `+0x1EF` / `+0x1F0` /
`+0x1F1` is silent - see [what a melee swing sounds like](battle-action.md#the-sound-a-melee-swing-makes-and-which-half-of-it-the-port-has) below.

**2. Tactical-Arts shout (`XA2` / `XA4` / `XA6`).** When the staged-anim materialiser
`FUN_8004AD80` runs a party art action, it calls the arts-voice cue selector
`FUN_8004C140(char_id, action_constant, flag)` (`see ghidra/scripts/funcs/8004c140.txt`),
which fires `FUN_8003D53C(clip_slot = (char_id-1)*2+1, channel, dur)`:

| character | clip slot | arts-voice file |
|---|---|---|
| Vahn | 1 | `XA2.XA` |
| Noa  | 3 | `XA4.XA` |
| Gala | 5 | `XA6.XA` |

all 16-channel short-mono shout banks. This is **traced and capture-verified**, not by-ear: a
live PCSX-Redux trace of Vahn's Tri-Somersault fires `FUN_8003D53C(0x01=XA2, chan 0/6, ...)`
and Noa's Miracle fires `(0x03=XA4, ...)`, both from `FUN_8004C140` (`ra = 0x8004C464`;
scenarios `battle_vahn_tri_somersault_super` / `battle_noa_miracle_art_combo`, probe
`scripts/pcsx-redux/autorun_arts_voice_cue.lua`). The `channel` is chosen **at random**
(avoiding an immediate repeat, via `gp+0xa4a`) from a per-art **candidate-channel pool** keyed
by the art's action constant. The pools live in SCUS tables: a range table at `0x800781A4`
(`[lo, hi, second_lo]` per character), a first-half table (`base + (hi - ac)*0x0F`) for
`lo <= ac <= hi`, and a second-half table (`base + (ac - second_lo)*0x10`) for `ac >= second_lo`
(bases `0x80077B64/0x80077D5C/0x80077F54` and `0x800780A4/0x80078104/0x80078154`). Three
first-half table variants exist, keyed on the context byte `ctx+0x243` (`ctx` = ptr at
`0x8007BD24`) and the `flag` argument; **a live battle art goes through the `(0, 0)` variant** -
recomp-runtime cue captures observe in-battle fires selecting channels 14/15, members only that
variant's pools carry (`scripts/recomp/xa_cue_capture.py`, frame-tagged reads of the
`FUN_8003D53C` cue globals `0x8007BBF0`/`0x8007BC6C`/`0x8007BC30`). The three second-half
tables are un-varianted and packed back-to-back (Vahn 6 records `0x2B..=0x30`, Noa 5
`0x2E..=0x32`, Gala 5 `0x2B..=0x2F`) - each character's art constants end exactly at its span,
so a walk past it reads the *next character's* rows. Each record is a channel list - byte `+0`
is always a member (channel 0 is legal) and, when `+1 != 0`, runs to the next `0`.
`dur = (dur_table[channel + char*0x10] * 0x3C + 99) / 100` from `0x80077A8C` (verified: Vahn
`ch0` -> `0x2D`, `ch6` -> `0x3D`, matching the trace; every recomp-captured cue's `dur`
reproduces this arithmetic for its observed channel). Parser: `legaia_art::arts_voice`; the
capture-witnessed per-art picks are `arts_voice::CAPTURED_ART_CHANNELS`.

Note the arts shout is **not** in the art record ([art-data](../formats/art-data.md)); its Hit
Effect Cue `0x1A` low half is an SPU SFX-descriptor id ([sfx-table](../formats/sfx-table.md)),
a separate subsystem.

An art whose action constant sits **below the range table's `lo`** (the `1A`-class Hyper
constants) has no pool row and plays **no** XA2/XA4/XA6 shout at all. Its cue is the
per-character stereo **fanfare** bank - the *even* clip slots `XA1`/`XA3`/`XA5` - fired from
the staged-animation materialiser `FUN_8004AD80`'s anim-id-`0x1A` block through the
`FUN_8004FCC8` jingle queue (Confirmed - disassembly `ghidra/scripts/funcs/8004ad80.txt` +
frame-tagged recomp cue captures of every art below). The selector, per queued Hyper constant
(`actor[0x1DF + cursor]`, cursor = `ctx+0x15`), is `jingle_id = rand() % 2 * 3 + base` - a
coin flip between a fixed pair of channels `{base_ch, base_ch+3}` (no avoid-repeat memory,
unlike the shout pool; a Frost Breath double-fire landed the same member twice). Base ids are
immediates in three per-character switch blocks (`0x8004B8D4` / `0x8004B9A0` / `0x8004BA6C`,
fire at `0x8004BB34`); the jingle decode is `n = id - 0x100`, clip `n>>3`, channel `n&7`,
`dur = (u16[0x800788B8 + n*2]*0x3C + 99)/100` (every captured cue's `dur` reproduces this).
The pairs, all capture-witnessed (witnessed members in parentheses):

| character | bank | art (constant) | channel pair |
|---|---|---|---|
| Vahn | `XA1.XA` | Burning Flare `0x1C` | 4 / 7 (4) |
| Vahn | `XA1.XA` | Fire Blow `0x1D` | 3 / 6 (6) |
| Vahn | `XA1.XA` | Tornado Flame `0x1E` | 2 / 5 (both) |
| Noa | `XA3.XA` | Hurricane Kick `0x1D` (stages `1A 1D 1E`) | 4 / 7 (4) |
| Noa | `XA3.XA` | Vulture Blade `0x1F` | 3 / 6 (3) |
| Noa | `XA3.XA` | Frost Breath `0x20` | 2 / 5 (2, twice) |
| Gala | `XA5.XA` | Explosive Fist `0x1C` | 4 / 7 (both) |
| Gala | `XA5.XA` | Lightning Storm `0x1D` | 3 / 6 (3) |
| Gala | `XA5.XA` | Thunder Punch `0x1E` | 2 / 5 (5) |

A **Super or Miracle** expansion takes the generic branch of the same block instead: when the
queue-builder's per-seat Super mark (`ctx[0x28D + seat]`, set by `FUN_801EED1C`) or its
16-word scratch `0x801F6990[cursor-1]` is non-zero, the id is the fixed per-character
`0x101`/`0x111`/`0x121` = **channel 1** of the same bank (sites `0x8004B7D0` and
`0x8004B840..68`; one-shot latch `ctx+0x28B`). Capture-witnessed on all three characters
(Vahn Tri-Somersault, Noa Super Tempest, Gala Miracle). A Miracle's **finisher** additionally
fires its animation cue track (`FUN_800508DC`, ids `0xC8..=0xFF` re-based `+0x38`) - witnessed:
Gala's Biron Rage ended on id `0x12D` = `XA29.XA` channel 5. Table + decode mirrored at
`legaia_art::hyper_fanfare`. Sibling cue in the battle overlay: SM state `0x6E` of
`FUN_801E295C` plays a whole-file XA stream via `FUN_8003EAE4(0, slot)` with the slot from the
SCUS byte table at `0x800787AF` (heroes → slot `0x08` = `XA9.XA`, no channel filter).

The site's arts page reproduces both cues faithfully: `crates/web-viewer/src/arts_view.rs`
parses `legaia_art::arts_voice` off the visitor's `SCUS_942.54`, demuxes the character's
`XA2`/`XA4`/`XA6` channels, and maps each art (by its record `anim_id` = action constant) to
the capture-witnessed channel where one is pinned, else to a stable member of its real
candidate pool (`ArtsVoiceTable::pick_channel`); Hyper/Super/Miracle records resolve their
fanfare channel through `legaia_art::hyper_fanfare` and demux `XA1`/`XA3`/`XA5` the same way;
`site/js/arts-viewer.js` plays the resolved clip as the art starts.

### Battle helper functions

Four helper addresses `FUN_801E295C` calls in the `0x801Fxxxx` battle-overlay region
(`0x801F0348` / `0x801F1ED4` / `0x801F3990` / `0x801F45A4`). **Dump-aliasing caution:** the
`overlay_0897_801f*` dumps these were first decoded from are double-shifted - PROT 0897's
extraction over-reads into PROT 0898 and that Ghidra program maps the file at base `0x801C0000`
instead of the true `0x801CE818`, so every function it surfaces at a `0x801Fxxxx` VA is really a
different battle-overlay function (and the "mid-body label inside an earlier entry" pairing is
an artifact of the same shift). `0x801F3990` is re-pinned below from battle-resident bytes; the
other three descriptions are retained but **need re-verification** against a battle-resident
dump (`overlay_battle_action_*`) before being relied on.

**Over-read `0x801Fxxxx` / `0x8020xxxx` alias resolutions.** A cluster of addresses that
surface as self-entry bodies in the `overlay_0897*` / `overlay_0897_xxx_dat*` dumps are the
same double-shifted images - each is byte-identical modulo relocation to a battle function
already pinned under its true entry, and nothing attests the printed VA. Resolve them to the
real entry (arbiter `classify-worklist.py --explain`; the first two independently confirmed
from the disassembly against the descriptions cited):

- `0x80205504` -> `FUN_801EED1C`, the retail queue-builder (below): zeros the 16-word scratch
  at the shifted `0x801F6990`, writes the action queue `actor[+0x1DF..+0x1E2]`, calls the
  Super applier `FUN_801EF9E4`.
- `0x8020A178` -> `FUN_801F3990`, the cast audio-cue dispatcher (below): mode gate on
  `DAT_8007BD10[ctx+0x13]`, two 9-entry `jr` jump tables keyed on `actor[+0x1E8]`, cues via
  `FUN_8004FCC8`; `actor[+0x1DF] == 0xFE` takes an effect-spawn path.
- `0x802028C4` -> `FUN_801EC0DC`.
- `0x801FD150` -> `FUN_801E6968`, the Lost Grail Final Heal auto-revive (state `0x50`).
- `0x801F8580` -> `FUN_801E1D98`.
- `0x801F8AB0` -> `FUN_801E22C8`.

Read the true entry's battle-resident dump, never the shifted alias. The remaining worklist
addresses at these VAs are non-standalone (interior citations, shared tails, `$zero`-absolute
data decoded as code, or 0-instruction stubs) and carry no body to document.

**`0x801F0348` - target-size camera framing.** Pinned from battle-resident bytes
(`overlay_battle_action_801f0348.txt`). It writes the camera height/distance at ctx `+0x6D0`
(i16) from a monster's **size class**, the byte at monster record `+0x1F`:

```text
ctx+0x6D0 = clamp(size_class << 7, 0x0C00, 0x1400)
```

The default `0x0C00` is also the floor, so only monsters with a size class above `0x18` pull the
camera back at all and everything from `0x28` up saturates. The slot it reads the size from is
resolved twice: first from the acting actor's target slot (`+0x1DD`, when `>= 3`), then - when the
acting actor is *itself* a monster (`ctx+0x13 >= 3`) - overwritten with the acting actor's own
size. The second store really does clobber the first, so a monster's attack frames on the
attacker's bulk rather than the target's. Record pointers come from the monster table at
`0x801C9348 + (slot-3)*4`.

Both lookups sit behind an **outer gate** on the target byte, `sltiu v0,v1,0x8` at `0x801F037C`,
and its branch target is the *clamp* rather than the attacker arm. A target slot of `8` or above
therefore suppresses the attacker-side store as well, leaving `ctx+0x6D0` at the `0x0C00` seed -
the one path on which a monster attacker's own size is ignored. Live slot bytes are only ever
`0..=6`, so the gate is a guard against a stale or uninitialised `+0x1DD`.

Ported as `battle_formulas::camera_height_for_frame` (whole routine, gate included) over
`camera_height_from_size_class` (the `<< 7` + clamp arithmetic), and wired: the port runs at
`ActionSeed` - the same edge as retail's call at `801e2d2c`, ahead of the gated
`FUN_801EFE44` bounds walk - feeding `BattleActionHost::camera_frame_height` and landing on
`World::battle.camera_frame_height`. The size input comes from the monster record's `+0x1F`
([`monster-animation.md`](../formats/monster-animation.md), `MonsterRecord::size_class` ->
`MonsterDef::size_class`) through the `BattleActionHost::monster_size_class` hook.

Retail's monster-band base is the literal `3` at `0x801F0384` / `0x801F03CC`, because retail
reserves three party slots whatever the party size. The port takes that base as a parameter
(`RETAIL_MONSTER_SLOT_BASE` for the retail reading) because `engine-core` compacts its seating
and seats the first monster at `party_count`; the two agree for any three-member party. This is
the same seating split `apply_side_lockout` documents from the other side.

> The earlier reading of this address - a 40-slot widget-pool teardown walking ctx `+0x11B4` -
> came from the aliased `overlay_0897_801f0348.txt` dump and is **falsified**: the
> battle-resident body contains no widget table, no free call and no `0x801C8FA0` clear.

**`0x801F1ED4` - player-summon effect-script dispatcher.** Re-pinned from a clean
self-entry dump: the classifier confirms the muscle-dome capture's bytes at this
VA are byte-identical to the PROT 898 battle-action image (`--explain 801f1ed4`
=> `REAL`; entry `801f1ed4`, 163 insns, `jr ra`; the `overlay_0897` dump is
interior, entry `801f1cc8`). The body is a `jr`-jump-table dispatcher on
`actor[+0x1DF] - 0x81` (the summon / Enhanced-Seru-Magic id block `0x81..=0xA0`;
`sltiu` bound `0x20`, table at `0x801D54EC`) into per-summon effect routines
(`FUN_801F69D8`, `FUN_801F6A84`, ...); after the routine, if `ctx[+0x27A] != 0`
it calls `FUN_801F2410`. It is the summon spell's visual driver, not a centroid
re-centre. The earlier "summon actor/camera re-frame - bounding-box recentre onto
the cast centroid" reading came from the aliased `overlay_0897_801f1ed4.txt` dump
and is **falsified**: that centroid-recentre body is really the formation
span-normalise leaf `FUN_801DB318` (documented above), surfaced at a shifted VA.
The summon **creature spawn** is a separate mechanism (the `summon.dat` applier
`FUN_801F12D0` / `FUN_801F19EC`, see [summon-readef](../formats/summon-readef.md)).
See `ghidra/scripts/funcs/overlay_muscle_dome_801f1ed4.txt`.

**`0x801F2160` - magic effect-class dispatcher.** Sibling of `0x801F1ED4`, called
from state `0x70` (magic-capture phase 2). Re-pinned from a clean self-entry dump
(`--explain 801f2160` => `REAL`; muscle-dome bytes = PROT 898; entry `801f2160`,
172 insns, `jr ra`). A `jr`-jump-table dispatcher on the spell's **effect-class
byte** - `*(DAT_800754C8 + actor[+0x1DF]*0xC + 1)`, i.e. `+1` of the SCUS
spell-table record ([spell-table.md](../formats/spell-table.md)); `sltiu` bound
`0x20`, table at `0x801D5A6C` - into per-effect-class routines
(`FUN_801F69D8`..`FUN_801F9BA8`); after the routine, if `ctx[+0x27A] != 0` calls
`FUN_801F2410`. **Ported** - `engine-vm::battle_cast_dispatch` carries the dispatcher itself
(the `+1` effect-class read, the `0x20` bound and the arm table). "Documented
not ported" described only the *targets*: the per-effect-class routines
`FUN_801F69D8..FUN_801F9BA8` are the slot-B cast modules, which each drive that
class's visual sequence and carry their own scope rows in the
`slot_b_cast_module` section of `scripts/ci/port-catalog-ignore.toml`. See
`ghidra/scripts/funcs/overlay_muscle_dome_801f2160.txt` and
[`cast-module.md`](cast-module.md).

**`FUN_801F3990` - cast audio-cue dispatcher.** Pinned from battle-resident bytes (the aliased
`overlay_0897_801f3990.txt` dump surfaces a different function - really `FUN_801DD0AC` - at this
VA). Argument-less: reads the active-actor index `ctx[+0x13]` and the per-slot char-kind table
`DAT_8007BD10`, dispatches on `actor[+0x1E8]` through two 9-entry jump tables, and plays the
per-class cast sound cues via `FUN_8004FCC8`. The earlier "per-move damage roll `FUN_801F3894` -
move-power table + RNG → damage, with a `FUN_801EC964` decimal-digit formatter" description came
from that double-shifted dump and is falsified. The spirit damage the state-`0x3D` reading
attributed here is state `0x3E`'s inline formula, ported as `battle_formulas::spirit_damage`.

#### The one caller is state `0x3D`, and it is an **Item / Spirit** state

`FUN_801F3990` has a single reference disc-wide: the `jal` at `0x801E3E04`.
That instruction sits in the arm at `0x801E3DD8`, and the arm's owner is
readable straight off the dispatcher's jump table: the table base is
`0x801CED44`, the word holding `0x801E3DD8` is at `0x801CEE38`, and
`(0x801CEE38 - 0x801CED44) / 4` = `0x3D`. So the cue band belongs to the
Spirit / Item band's wait state, not to the Magic band - and the band is
entered from exactly one place, state `0x3C`'s unconditional `ctx[7] = 0x3D`
store: `addiu v0, zero, 0x3d` at `0x801E3B5C` and the `sb v0, 7(v1)` at
`0x801E3B60` it feeds. The arm's one branch - `sltiu v0, v0, 3` at
`0x801E3B28` on the byte at `s5[+2]`, the same byte the state passes to
`FUN_801D5854` - rejoins at `0x801E3B40`, above both, so no path through
`0x3C` skips the store.

Which actions reach `0x3C` is fixed by the two category arms the
[category jump table](battle-action.md#inner-dispatch---actor-action-category) at `0x801CF144`
selects:

* **category 1 (Item)**, arm `0x801E2E30`: stores `ctx[7] = 0x3C` *first* and
  overrides to `0x28` only for the two summon-item ids. Every ordinary item
  use therefore walks `0x3C -> 0x3D` and reaches the cue band.
* **category 2 (Magic)**, arm `0x801E2EB0`: stores `0x28` first and overrides
  to `0x3C` only when the spell's class byte is `< 0x14` **and** the spell id
  is `< 0x65` (`sltiu v0, a0, 0x65` at `0x801E2EF4`). The player Seru block
  `0x81..0x8B` fails the id test outright, so no player Seru cast can route
  into the band whatever its class byte says. Twenty-four of the ids below
  `0x65` satisfy both tests.

That bound is the whole reason a cast-driven sweep finds the arm cold: the
band's door is an item, not a spell. Driving it as an item use reaches it
immediately (`capture`): the seed state `0x0C` was hijacked to category 1 on
two battle scenarios, and every driven item action entered `0x3C`, entered
`0x3D`, passed the `+0x1DA == +0x1D9` guard on the state's first frame, called
`FUN_801F3990` with `ra = 0x801E3E0C`, fired the band's `jal 0x8004FCC8` at
`0x801F3C18`, and started a CD-XA clip through `FUN_8003D53C`. The resolved
cue ids came out on the documented party leg `char_kind * 0x10 + 0xF8..0xFC` -
`0x0108` / `0x010B` for a `char_kind` of `1`, `0x0128` for a `char_kind` of
`3`. Probe:
[`autorun_spirit_item_cue_band.lua`](../../scripts/pcsx-redux/autorun_spirit_item_cue_band.lua).

The guard passing on the state's first frame is what the two halves of the
band's timing look like from `0x3C`: that state stages `actor[+0x1DA] =
actor[+0x1E7]`, so when the queued clip byte and the live clip byte already
agree the wait is zero-length and the cue fires the frame after the pre-arm.

The party leg's ids are all `>= 0x108` (the char-kind byte is the 1-based
roster id), so the cue is never an SPU descriptor: it is the character's
CD-XA voice - Vahn's Healing Leaf is `0x0108`, clip slot `0x1A`, channel `0`.
The port resolves it at the world through `admit_voice_cue` onto the
`(clip, channel, dur)` channel both hosts play.

#### The commit clip `actor[+0x1E7]`

The byte `0x3C` stages is written by the command ring at the commit, one
value per arm (`FUN_801D0748`):

| arm | `+0x1DE` | `+0x1E7` | store |
|---|---|---|---|
| Item | `1` | `9` | `li v0,0x9` / `sb v0,0x1e7(v1)` at `0x801D13E4..0x801D13E8` |
| Magic | `2` | `9` | `0x801D14BC..0x801D14C0` |
| Spirit | `4` | `0x10` | `0x801D16A8..0x801D16B0` |

Nothing between a Spirit turn and the next commit clears it (the only
clearing store is `FUN_801D388C`'s all-party reset at `0x801D392C..0x801D3934`),
so an arm that skipped its own write would inherit the Spirit clip - and the
Spirit clip's cue track - as its pose, and an item used after a Spirit turn
would sound like Spirit.

**`0x801F45A4` - end-of-action damage / HP-bar settle.** *Decoded from the aliased
`overlay_0897_801f45a4.txt` dump (disasm only; the Ghidra decompile times out) - identity, entry
address, and body need re-verification against battle-resident bytes.* Per
action category (`actor[+0x1DE]` `1..6`) it tests the actor's ability bits in the character record's
`+0xF4`/`+0xF8` bitfield (base `0x80084140 + (char_id-1)*0x414`, fields `+0x6BC`/`+0x6C0` = record
`+0xF4`/`+0xF8`) and, when set, ramps a value pair `*s0` toward `*s2` by half per pass (`*s0 += (*s2
- *s0) >> 1`) - the HP-bar / damage-number settle. It applies the AP-boost bits (`+0x200`/`+0x100`)
to `actor[+0x170]` and clamps it at 100 (`0x64`) - the same adjust-and-clamp the `0x50` Done arm
performs - clears status-word `actor[+0x16E]` bits, resets brightness/screen globals, and ends in a
per-actor jump-table dispatch keyed on `actor[+0x1D]`. Called at battle-complete (`0xFF`); it is the
final damage/HP settle + ability-effect application, not a bare teardown.

### The `+0x16E` status word - one bit map, two port representations

Every scan above reads one halfword, so the bit map is worth stating once. The
bits are pinned individually by the HUD icon selector `FUN_8002C2E4`, whose
priority ladder tests them in order, and by the appliers that set them.

| bit(s) | condition | where it is set |
|---|---|---|
| `0x0001` | Venom | weak-DoT applier, 1/8 |
| `0x0002` | Toxic | strong-DoT applier, 1/8 |
| `0x0004` | Stone | petrify applier (`ori $v0,$v0,4` at `0x80041CF4`, over the `lhu` at `0x80041CEC`) |
| `0x0008` / `0x0010` / `0x0020` | Rot, one rolled limb | `1 << (rand % 3 + 3)` |
| `0x0040` | inside the Rot group, never set | - |
| `0x0080` / `0x0100` / `0x0200` | AI delegation, always as the group `0x0380` | `FUN_80047430`, from accessory passive bit 45 |
| `0x0400` | Numb | status applier |
| `0x0800` | Sleep | status applier |
| `0x1000` | Curse | curse applier, 1/4 |
| `0x2000` / `0x4000` / `0x8000` | unused - no applier in the dumped corpus | - |

Three masks are read as units: `0xF84` gates a slot out of the selectable
scans (Stone, the delegation group, Numb, Sleep), `0x0F80` is what taking
damage clears, and `0x0404` is the whole-actor inert test. There is **no KO
bit** - a dead actor is one whose `+0x14C` is zero, which is why every consumer
pairs the word with the liveness halfword.

The port holds the same conditions twice: `BattleActor::field_flags` is the raw
word (the cast band ORs its debuff bits straight in), and the status tracker
holds a typed instance list the turn loop reads. `World::raw_status_word`
composes them - the typed list packs through
`status_effects::pack_display_flags`, whose bit map is this table, and the raw
word ORs in unchanged - so a consumer that wants retail's word gets every bit
either half carries, including the delegation group the typed list has no kind
for.

### Actor-pool leaf helpers

Small self-contained routines the SM and its round driver call over the
8-slot battle-actor pool (`&DAT_801C9370`) and the ctx target queue. Each is
ported as a pure function in `engine-vm::battle_action` (`pool_ops`); all are
transcribed from the disassembly (`overlay_battle_action_801db9c4.txt` /
`_801db318.txt` / `_801d8a88.txt` / `_801d8d00.txt` / `_801db124.txt` /
`_801db8b4.txt` / `_801dba04.txt` / `_801db81c.txt`, plus `80019b28.txt`).

- **`FUN_801DB9C4` - pool `+0x8` flag-word scrub.** AND-masks the `+0x8` flag
  word of pool slots 0..=6 with `0x7CFFFFFF` (clears bit 31 and bits 25/24).
  Its only static caller in the battle overlay is the pose setter
  `FUN_801D5854`'s out-of-range guard (`jal` at `0x801D58E8`). It is **not** the
  state-`0x5A` per-actor anim-flag clear: state `0x5A` runs its own inline mask
  (`lui a1,0x7cff` at `0x801E6478`, paired with `+0x21F = 0`) and
  `FUN_801E295C` contains no call to `FUN_801DB9C4`. Port:
  `clear_pool_flag_words`.
- **`FUN_801DB318` - formation span-normalise + recentre.** Over the included
  slots (0..2 always, 3.. gated on `+0x14C`), takes the X/Z extents; if an axis
  spans more than `0x800` it rescales every included coordinate by
  `(coord << 11) / span` (`span` is the extent narrowed to i16) and divides the
  matching camera-focus accumulator (`_DAT_80089118` X / `_DAT_80089120` Z)
  likewise; then it recomputes the extents and subtracts the centroid
  `((max + min) as u32) >> 1` from every included slot, shifting the focus
  accumulators back by the same centroid. Port: `normalize_formation_span`,
  run at every round start and on the ring's cancel back to the round prompt
  by `World::normalize_battle_formation`
  ([battle.md](battle.md#stage-seats-fun_800513f0-placement-tables)).
- **`FUN_801D8A88` - attack target-queue builder.** Builds the ring the cycle
  accessor steps through. Counts live monsters (slots 3..=6) into `ctx[+0x244]`,
  takes the acting actor's `+0x1DD` current target as the wrap slot `+0x245`,
  then computes each monster's bearing offset from the current-target direction
  (via `FUN_80019B28`, each result `+0x800 & 0xFFF`, expressed as a positive
  angle in `[0, 0x1000)`) and appends the three nearest *alive, non-target*
  monster slots to `+0x246..` in ascending order, consuming each pick. Port:
  `build_attack_target_queue` / `AttackTargetQueue` (the bearing is a closure so
  the ordering ports without the retail arctan LUT).
- **`FUN_801D8D00` - attack target-cycle accessor.** Locates the active actor's
  current target inside the multi-target ring built by `FUN_801D8A88`
  (`ctx[+0x244]` count, `+0x245` wrap slot, `+0x246..` ordered slots) and steps
  to the next (`param 0`) or previous (`param 1`) entry, wrapping at the ends.
  Port: `cycle_attack_target` / `TargetCycle`.

The seed state also raises the per-action **target banner**. Retail's seed body
ends every category arm at the same `jal 0x801e6d84` (`0x801E3028`), and the
port mirrors that placement: `battle_action::dispatch::raise_target_banner`
runs `plan_target_banner` (`FUN_801E6D84`, in `engine-vm::battle_cue_group`)
and raises each HUD element id it lists through `BattleActionHost::ui_element`.
Category `5` - Run / Defend - is the one arm that returns before raising
anything; categories `0` and `4` raise the caster banner and skip the target
arm. Two retail inputs are abstracted, neither of which reaches the id list:
`ctx[+0x24B]` (the `target == 9` override slot, passed as `0`) and the
`FUN_80035F04` descriptor width the banner-width term subtracts.

The two target-cursor leaves are the **engine's** enemy target cursor, not just a
transcription: `engine-core`'s `TargetPickerSession` builds the ring from its
own monster rows (which carry each slot's battle-world seat) and steps it, so a
Left/Right press moves to the angularly nearest live monster the way retail
does rather than to the next slot index. The bearing comes from
`FUN_80019B28`'s ported quadrant algebra over a computed arctan table
(`approx_arctan_lut`), because the retail table at `0x8006F4C8` is not
extracted by any boot path. A host that never seated its actors leaves the
seats at the origin, and the cursor falls back to a plain slot-order scan.
- **`FUN_801DB8B4` - first live monster slot.** Scans pool slots 3,4,5,6 and
  returns the first with a non-zero `+0x14C` liveness halfword; falls through to
  `7` when none is alive. Port: `first_live_monster_slot`.
- **`FUN_801DBA04` / `FUN_801DB81C` - selectable-participant scans.** Both walk
  the pool over `0..ctx[0]` applying the same three predicates - the slot's
  **roster character id** `(&DAT_8007BD10)[i] != 4` (`0x801DBA44`), alive
  (`+0x14C`), and no can't-select ailment (`+0x16E & 0xF84`).
  `FUN_801DBA04` starts at slot 0 (first selectable target); `FUN_801DB81C`
  starts at `ctx[+0x13] + 1` (next participant after the current actor). Each
  returns `ctx[0]` when nothing qualifies. Ports: `first_selectable_target` /
  `next_selectable_actor`, both called by `engine-core`'s
  `World::next_member_owing_command`.

  The `!= 4` term used to read here as an *action-state* byte with `4` meaning
  "removed / done". It is neither: `DAT_8007BD10` is the three-byte per-slot
  roster id (`01 02 03` = Vahn / Noa / Gala) that the arts preseed indexes to
  reach a character record (`byte - 1` is the record slot, `0x801DA37C`), and
  `4` is the AI companion seat - the same `== 4` test the auto-fight block in
  `FUN_801EED1C` uses. The scans exclude a seat the player does not command,
  not one that has finished commanding.
- **`FUN_80019B28` - 12-bit bearing (atan2).** Folds the displacement
  `(p2 - p1)` into a quadrant by sign, divides the shorter leg into the longer
  (`(min << 11) / max`), indexes the retail arctan LUT at `0x8006F4C8`, and adds
  the per-octant `0x000/0x400/0x800/0xC00` base to reassemble a clockwise 12-bit
  heading (`0x000` = `-Z`, `0x400` = `+X`). Port: `bearing_12bit` (LUT is
  caller-supplied Sony data; no table bytes embedded). The motion VM keeps a
  separate `f32` approximation for its face-target ramp.
- **`FUN_801DB124` - dead-target redirect roll.** When a queued action's chosen
  target (`actor[+0x1DD]`) is dead, and the category qualifies (Attack always;
  Magic when the spell class byte `>= 0xA` or the target is an enemy slot; Item
  only for ids `0xFE`/`0x98`), it re-rolls a **living** slot on the same side
  (`rand % party_count`, or `rand % monster_count + 3`), retrying until alive.
  Port: `redirect_dead_target` / `RedirectQuery`. Its turn-picker call site
  (`FUN_801DABA4`'s party arm, `0x801DAF14`, gated on `ctx[+0x06] == 0xFF`)
  is what stops a member whose target died earlier in the round from walking
  at the corpse: the short step `0x19` has no timeout, and the range law never
  brings a dead target into reach. The picker's monster arm calls it too,
  unconditionally, straight after the AI picker (`jal 0x801E9FD4` then
  `jal 0x801DB124` at `0x801DAF48..0x801DAF50`), which is what stops a monster
  whose picked party member has since fallen from walking at the corpse. That
  arm has no category gate, so it reaches a monster's single-target **cast**
  too (category `2`, spell id in `+0x1DF`). The Magic arm's "class byte" is
  the spell record's cast class `+0` (`0x800754C8[id * 12]`), and every real
  class (`0x14` / `0x32` / `0x63`) clears `0xA`, so a cast at a fallen hero
  re-rolls onto a standing one; only the class-`0` records (the internal
  `0x00..=0x24` tiers and the monster attacks `0x2E` / `0x2F`) keep a dead
  party target. An all-side target code (`8` / `9`) fails the `< 8` test and
  is left alone. The engine runs it at each party dispatch and on a
  monster's strike and single-target cast (`World::redirect_dead_battle_target`,
  called from `take_monster_turn`).

### Per-frame action-effect update helpers

Battle-overlay functions that run the *visual* side of a chosen action -
projectile flight, action-HUD chrome, side-band asset streaming. All are heavy
GTE / GPU-primitive / CD-IO work reached through the effect + anim path, so
they are documented here rather than lifted whole into `engine-vm`.

- **`FUN_801DA6B4` - target-select cursor tint.** Over the fixed monster-slot
  window `3..=6`, brightens the acting actor's current target (`+0x1DD`) and
  dims the rest: `+0x21C` render flag (`5` / `200` / `0`), `+0x4` colour word
  (`0x20080200` / `0x00401004`), `+0xC` tint-blend word (`0x1000` / `0`). The
  blend word is the q12 intensity `FUN_8004A908` copies into the render
  packet's `+0x78` whenever it is non-zero - it weights how hard the `+0x4`
  colour modulates the mesh, and `FUN_80050120` arm 0 drains it by `0x20` per
  frame back to `0` once the colour word has eased neutral (the item/spirit
  cue-group expander `FUN_801E22C8` flashes it to `0x2000`). It is not a mesh
  scale. `param_1
  == 0` stamps the highlight; non-zero clears it. Only the alive slots
  (`+0x14C != 0`) are touched. Self-contained; ported as
  `battle_action::target_cursor_highlight`. See
  `overlay_battle_action_801da6b4.txt`.
- **`FUN_801DBDDC` - the Rot stamp over an arts-entry chip.** Gated on
  `ctx[+0x6CE] == 0`. Emits one `POLY_FT4` (tag `0x09000000`, colour
  `0x2C808080`) sampling the `etim` Rot stamp (`(0x50, 0x60)` 32x24, CLUT
  `0x770B`) over `(x, y, cost)`, widened by `(cost - 0x1E) >> 1` each side,
  and links it via `FUN_8003D2C4`. Called only by the round driver's
  arts-entry arm, once per rotted limb; see
  [arts-command-gauge.md](arts-command-gauge.md#status-limb-gating). Ported
  as `engine-vm::battle_party_panel::rot_stamp_on_arts_chip`.
- **`FUN_801DEA50` - action effect-script stepper.** For the acting actor
  (`param_1 == ctx[+0x13]`) walks 8-byte effect-script records at `param_2`
  under the `actor[+0x1F5]` cursor (`< 8`), GTE-rotating each record's offset by
  the actor's `+0x46` facing through the sin/cos LUTs (`_DAT_8007B81C` /
  `_DAT_8007B7F8`) and spawning effects via `FUN_80050ED4` / `FUN_801DFDF0`. On
  a terminator it installs the **move-power record** (`0x801F4F5C +
  map[actor+0x1DF]*0x1A`, map at `0x801F4E64`, `0x1A`-byte stride) at
  `ctx[+0x1014]` and seeds per-target homing state (`+0x1144` position, `+0x252`
  target, `+0x1166` bearing). Kernels ported as
  `engine-core::action_effect_script`; the spawns come back as requests. See
  `overlay_battle_action_801dea50.txt` and
  [move-power.md](../formats/move-power.md).

  **`FUN_801E295C` does not call it.** A five-form reference scan
  (`scripts/ghidra-analysis/find-address-word-refs.py 801dea50`) finds the
  address referenced exactly twice in the whole corpus, both `jal`s in
  `SCUS_942.54` at `0x800478B8` and `0x80047C08` - inside `FUN_80047430`, the
  per-frame anim-node tick. There is **no** reference of any form to it inside
  the battle-action overlay image. So the effect-script walk is driven from the
  anim path, not from the action SM, and a port that waits for the action SM to
  reach it waits forever.
- **`FUN_801E09F8` - cast-effect census + projectile flight/impact.** Runs two
  jobs each frame. **(1) Census**: it recomputes, from scratch, the outstanding
  effect-count fields the magic/summon exit states poll - `ctx[+0x249]` (actors
  still mid-animation: `+1` per live actor with `+0x1D9 != 0`, less party actors
  whose `+0x1D9 == 8`) and `ctx[+0x24D]` (active spell-children over `ctx[+0x252..]`),
  plus the sole-survivor target indices `ctx[+0x24A]` (party) / `ctx[+0x24B]`
  (monster). `ctx[+0x24D]`'s count is **gated**: it counts non-zero entries of
  `ctx[+0x252..=+0x255]` only if at least one entry of `ctx[+0x24E..=+0x251]` (the
  per-slot kind array) is non-zero, and retail returns from the whole tick before
  reaching the count otherwise (`0x801E0BA8`) - so an empty kind array reads as
  "nothing outstanding" whatever the child array holds. These are **live counts,
  not latched flags**, which is why state
  `0x2E` (magic exit, gated on `ctx[+0x249] == 0`) and state `0x35` (summon
  sustain) wait on them - a stalled effect child that never dies pins the count
  above zero and holds the band. **(2) Flight/impact**: it steps the in-flight
  effect slots (`ctx[+0x24E]` phase, `+0x252` target, `+0x1144` position, `+0x6C6`
  per-slot timer), homing each with the LUT trig and spawning per-effect visuals
  via `FUN_801DFDF0`; on arrival it calls the damage kernel `FUN_801DD0AC`
  (indexed through the `0x801F4E64` map) and applies the roll to the target's HP
  (`+0x14C`), death anim (`+0x1DA`), and the accumulated-damage queue
  (`ctx[+0x83C]`). **The two halves have different verdicts.** The census head
  (`0x801E0A44..0x801E0BF0`), including the `+0x24D` early-out ordering, is
  ported as `engine-vm::battle_cast_census::cast_census`, and the tail's
  **hit arm** (`0x801E1844..0x801E1A6C`) as
  `engine-vm::battle_cast_census::effect_child_hit`, which is what gives
  `battle_hp_bar::clamp_damage_against_live_hp` a caller - `engine-core`'s
  `World::apply_effect_child_hit` drives it from the cast fold. What sits
  between the two does **not** share one verdict. The **GTE homing** is
  render-track: the engine transforms effect positions through its own wgpu
  path and the primitives retail calls are scope rows under `[libgte]`. The
  **per-effect spawn** already has both its sinks - `World::try_spawn_effect`
  for the direct form and `World::spawn_action_table_effect` for the table
  form, drained by `World::drain_battle_effect_spawns` - so what is missing
  there is not a spawner. It is the third item, and it is the only genuinely
  simulation-shaped one: the **per-slot staging arms** that decide *when* each
  slot fires - the `ctx[+0x24E]` phase byte, the `+0x252` target, the
  `+0x1144` position quad and the `+0x6C6` per-slot timer. Read off the 0898
  image at base `0x801CE818`; see `overlay_battle_action_801e09f8.txt`.

  Those arms are ported as `engine-core::action_effect_script::HomingSlots`,
  seeded by the terminator beside the streak (`World::seed_homing_slots`) and
  stepped each battle frame (`World::tick_homing_slots`). Phase `1` waits out
  the streak word; then every slot takes `2` when the record's `+0x12` list is
  non-empty, else `3` (`0x801E0CF8..0x801E0D34`). Phase `2` turns onto the
  child's live seat, steps `record[+0x08]` units along that heading, spawns
  the `+0x12` list each time its counter is out and re-arms it to
  `0x40 - record[+0x08]`, and lands inside `|dx| + |dz| <= 0x100`: phase `3`,
  counter `record[+0x06]`, the slot on the seat raised by `record[+0x02]`, the
  `+0x16` list spawned there (`0x801E0FB8..0x801E1430`). Phase `3` drains and
  frees the slot. The transition falls straight into the per-slot pass in the
  same call, so a zero landing counter frees the slot on the frame it lands.
  The spawns are the move's own list bytes: for a cast, whose fold would
  otherwise stage both lists at the target at once
  (`World::request_move_fx_spawn`), the flight takes them over when the
  caster's script still has its terminator ahead - and also when the
  terminator runs first. Retail has no fold to order against: a monster's
  Tail Fire (move `0x27`, record `map[0x27] = 0x12`, both lists `[0x1B]`)
  seeds its flight from Gimard's script before the engine's fold, and
  `battle_gimard_tail_fire_b` holds two `0x1B` prototype nodes (record
  `0x801F5A3C`) and nine live children of the `0x17` burst they run (record
  `0x801F5DA4`, the wide arm's stager). So a flight seeded
  ahead of a pending Magic-category fold emits, and the fold then stages
  nothing (`CastFxState::homing_holds_lists`). The hit stays with the cast
  fold, and the census is not fed from the slots.

  Two details of that arm a decompiled reading loses. The `+0x1DC` writes are
  bit **ORs** (`|= 4` on the reaction leg at `0x801E19D8`, `|= 1` on the face
  leg at `0x801E1A18`), not the `+= 1` bump the slot-B cast modules use. And
  the face store has no `+ 0x800` term (`0x801E1A54` writes the raw
  `FUN_80019B28` result), so the victim turns to **face** its attacker here,
  where every cast module's equivalent store faces it away. The reaction pick
  has three legs, not two: a dead victim takes `+0x1F1` regardless of the
  `+0x1F2` gate, and a zero `+0x1EF` falls on to `+0x1F0`.
- **`FUN_801E0080` - the effect-VM per-frame walker.** Gated on
  `DAT_8007BD58 != 0 && DAT_8007BD71 == 0xFF` (battle live, no end signal).
  The 32-slot `0x1C`-stride pool at `_DAT_8007BD30 + 0x1010` is the effect
  **master** slots and the 128-slot `0x20`-stride pool at `_DAT_8007BD30 + 0x10`
  their **children**; their scripts are the `efect.dat` 2-pack (PROT 0873) the
  init `FUN_801DE914` fixes up, and the zeroed pools themselves are a slice of
  the battle heap block `FUN_800513F0` allocates. The third pass builds one
  textured-sprite primitive per live child (`0x09000000` tag, brightness
  envelope, random UV mirror). This is the routine
  [`effect-vm.md`](effect-vm.md) documents from its prologue word `0x801E0088`;
  its entry is `0x801E0080`, where the pool-ready byte is loaded, and the one
  `jal` to it is the draw tick `FUN_800480D8`'s per-frame pass. Ported as
  `engine-vm::effect_vm` (`Pool::tick_retail` / `Pool::child_billboards`), live
  through `World::tick_effects`; a second port that read it as a separate
  "arena scatter" with unparsed data pools duplicated it and was removed.
- **`FUN_801DF6B8` - damage-number popup renderer.** Draws a scaling decimal
  number sprite for one actor's accumulated damage `ctx[+0x83C]`: extracts each
  base-10 digit (`* 0x66666667` / `>>0x22` = divide-by-10), indexes the digit
  glyph atlas at `0x801F6..` (`-0x7FE09BA4`), and builds one `0x09`-code sprite
  quad per digit into the OT, ramp-scaling the rect by the per-frame timer
  `ctx[+0x85C]`. The anchor is the struck actor's display trio
  `+0x3C/+0x3E/+0x40` with Y replaced by `+0x3E / 2 - timer * 3 / 2`; the
  timer steps `0x10` a frame and `FUN_800195A8` projects a view-space square
  of half-extent `timer / 2` about it, so the number grows as it rises. The
  rect is widened to at least `clamp(timer >> 5, 1, 12)` and cut to 24 px,
  clamped to `y >= 32`, `x <= 280` and `x >= 8 + 32 * extra digits`, and the
  value is zeroed once the timer passes `0x240` (37 frames). Port:
  `engine-vm::battle_value_readout::popup_cells` (layout) and
  `engine-ui::battle_numerals::popup_value_cells` (anchor + projection), which
  both hosts seat their numerals through. See
  `overlay_battle_action_801df6b8.txt`.
- **`FUN_8005112C` - per-character signature effect trigger.** SCUS-resident
  (`8005112c.txt`), gated on `actor[+0x68] != 0 && actor[+0x5A] < 3` (a party
  slot). Reads the roster char id `DAT_8007BD10[actor[+0x5A]]` (`1`/`2`/`3` =
  Vahn/Noa/Gala) and, when the actor's current anim id (`*(actor[+0x4C]) + 0x77`)
  hits that character's hard-coded frame value (`0x29`/`0x1E`/`0x2A`/`0x64`),
  fires `FUN_80048310(actor, effect_id, 3, rgb)` with a per-character effect id +
  RGB tint - a hand-authored visual accent on a specific animation frame.
  **Ported**, and the "effect spawn, not a formula; not ported" verdict here is
  stale: the accent is the weapon trail, and `engine-vm::battle_trail` carries
  the trigger's per-character identity-byte table together with
  `FUN_80048310`'s sweep schedule and band colour ladder. Only the projected
  quad emission is render-track (`engine-ui::battle_trail`).
- **`FUN_801F17F8` - summon / readef side-band streamer.** A three-phase
  (`ctx[+0x26C]`) CD loader gated on `ctx[+0x26B]`: opens `data\battle\summon`
  (arg `0x37F`) or `data\battle\readef` (arg `0x380`) via `FUN_800558FC`, reads
  a `0x10800`-byte page into `ctx[+0x314]`, and waits on `FUN_8003DE7C`. Pure
  CD-IO; the engine streams these through `SceneAssets`. See
  `overlay_battle_action_801f17f8.txt` and
  [summon-readef.md](../formats/summon-readef.md).

The worklist addresses `0x801F1ED4` / `0x801F2160` have their only clean
self-entry dumps in the **muscle_dome** overlay, but the classifier confirms
those bytes are **byte-identical to the PROT 898 battle-action image**
(`--explain` tags both `REAL`, capture `battle_action(898)`) - the muscle-dome
overlay carries the same battle-action code region, so the dumps *are* the
battle-resident bodies and are decoded [above](#battle-helper-functions) (the
summon and magic effect-class dispatchers). The battle overlay's *own* dumps
show these VAs as interior because the `overlay_0897` extraction is
double-shifted (the aliasing the caution above describes); that shift is also
what produced the earlier - now falsified - "summon actor/camera re-frame"
reading of `0x801F1ED4`. `0x801F69D8` / `0x801F7088` remain per-summon effect
leaves reached through those dispatchers.

## Overlay-local PRNG `FUN_801D0290`

The battle-action overlay carries a second random-number generator, distinct
from the SCUS PsyQ-shape `rand()` at `FUN_80056798` that
[battle-formulas.md](battle-formulas.md#rng-primitive) documents. It is
twelve instructions with no frame, and its whole state is the word at
`0x801F6950` (the overlay's own data tail):

```
s = *0x801F6950
v = s * 12 + 2              ; (s << 2) + (s << 3) + 2
s = (v << 16) + (v >> 16)   ; 32-bit rotate by 16
*0x801F6950 = s
return s                    ; the store is the jr-ra delay slot
```

The multiply-add is done with shifts. The final step **is** a rotate, and an
earlier note here saying it is not can be discarded: the `addu` sums `v << 16`,
whose low sixteen bits are all zero, with `v >> 16`, whose high sixteen bits are
all zero because the shift is `srl` and not `sra`. The two operands occupy
disjoint bit ranges, so no carry can arise and the `addu` is bit-for-bit an
`or`.

### The one caller - and it is not `FUN_801CFB94`

Five call sites (`0x801CFCE4` / `0x801CFDE8` / `0x801CFED4` / `0x801CFF1C` /
`0x801CFF5C`), none in SCUS, and all five inside a single routine:
**`FUN_801CFA48`**, the overlay-resident **effect-ribbon geometry emitter**,
whose body runs `0x801CFA48..0x801D028C` - the function immediately before the
generator in the image.

`0x801CFB94` is not a function entry. It is a branch target inside that
routine's plane-select switch: the words there are `j 0x801CFBE4` +
`addiu t8, t8, 4`, one of four arms, and the enclosing prologue is
`addiu sp, sp, -0x70` at `0x801CFA48` with a pointer table in the words before
it. Naming it as the caller is the intra-function-label-promoted-to-fake-`FUN_`
artifact [`ghidra.md`](../tooling/ghidra.md#decompiler-artifacts-that-have-produced-false-claims)
catalogues, and it also collides across the slot-A family - `0x801CFB94` **is** a
real `jal` target inside the cutscene overlay (PROT 0970), which is a different
routine at the same VA.

### What the draws feed

`FUN_801CFA48` is the `0x2000` arm of the multi-target case of the per-actor
render dispatcher [`FUN_8001ADA4`](world-map.md#per-actor-render-dispatcher---fun_8001ada4).
At `0x8001B0F0..0x8001B124` SCUS tests `actor[+0x9E] & 0x2000` and calls it as
`(scratch, actor[+0x9E], (s16)actor[+0x9C] + (((s16)actor[+0xC8] >> 3) << 8), actor + 0x9C)`,
which is the same call shape its two SCUS siblings `FUN_8002A5A4` (`& 0x4000`)
and `FUN_80028158` (neither bit) take. `scratch` is `*_DAT_8007B85C + 0x5DC00`, the
synthetic-TMD block the [cutscene tile shatter](cutscene.md) builds into as well.
Which shape each arm draws is settled on the disc rather than by inference: the
dev harness in PROT 0973 selects the three emitters from one switch and prints
its own label first - `CICLE1` for `FUN_80028158`, `SPRITE1` for `FUN_8002A5A4`
and `THERNDER1` for this one (`0x801CED30..0x801CEE10`). It is the **lightning**
emitter.

What it builds is a synthetic Legaia TMD object - object descriptor at
`out + 0xC`, vertices from `out + 0x28`, primitives after them, group header
`count = 6 * segments`, `flags = 0x26`, `ilen = 9`, `mode = 0x3C` - whose shape
is a jagged random walk. Each segment emits **six** 8-byte vertices at lateral
offsets `±r`, `±2r` and `±8R` about the walk position (the lateral direction is
the heading plus a quarter turn, the `+ 0x400` at `0x801CFD00`), and **six**
9-word Gouraud-textured quads: the core drawn twice, then a mid band and an
outer band on each side, the outer pair fading to a black vertex colour. The
five draws are exactly the five things about that walk that are random:

| Site | Draw | What it sets |
|---|---|---|
| `0x801CFCE4` | `s0/2 + rng() % s0` | the segment's **inner** half-width `r` - the `±r` and `±2r` vertex pairs |
| `0x801CFDE8` | `s0 + rng() % s0` | the segment's **outer** half-width `R` - the `±8R` pair |
| `0x801CFED4` | `rng() & 7` | a 1-in-8 **kink**: on zero the heading accumulator is quartered and negated (`0x801CFEEC..0x801CFF18`) |
| `0x801CFF1C` | `rng() % m - m/2` | the ordinary per-segment **turn** added to that accumulator, `m = param[+0x0C]` |
| `0x801CFF5C` | `L + rng() % L` | the segment's **advance length**, `L = (s16)param[+0x1A] >> 1` |

`s0` is the tapered half-width: `(s16)param[+0x18] >> 1` over the first half of
the run, scaled linearly down to `1` over the second, and `1` at segment 0. So
the answer to "which battle quantities does it feed" is **none**. Every draw
lands in vertex geometry; no damage number, target pick, formation slot, camera
angle or timer is on the far side of any of them.

The state is also **re-seeded on every call**, at `0x801CFC18`:
`*0x801F6950 = (s16)param[+0x1C] >> 2`, where `param` is `actor + 0x9C`. From a
caller's point of view the generator is therefore not a stream at all but a
shape **hash** - the same seed halfword redraws the identical bolt frame after
frame, which is what lets a growing bolt be rebuilt from scratch each frame with
one fewer suppressed segment (`FUN_801CFA48` forces the leading
`total - count` segments to zero width). It is also why the generator can be
overlay-local: nothing about it has to survive a call. Either way the earlier
observation holds - draws from it do not perturb the `FUN_80056798` stream the
determinism oracles follow.

### Nothing outside PROT 0898 touches `0x801F6950`

A byte sweep over `SCUS_942.54`, every extracted overlay image and every PROT
entry finds exactly **three** machine references to the word, all in PROT 0898:
the seed store at `0x801CFC18` and the generator's own load and store
(`0x801D0294` / `0x801D02BC`). The field (0897) and menu (0899) images carry
none, and the address is not even inside the field overlay's own content, which
ends at `0x801F3818`.

The four `overlay_0897_*` dumps that look like they reference it do not. Three
of them (`801F747C`, `801F7628`, `801F5748`) match only because `801f6950`
occurs as an *instruction address* inside a mis-based print of `FUN_801D0748` -
a text grep over a dump is not a reference scan. The fourth,
`overlay_0897_801E63E0`, is the real store re-keyed:
[`overlay-va-aliases.md`](../reference/overlay-va-aliases.md) already resolves
`0x801E63E0 - 0x167E8 = 0x801CFBF8`, i.e. **inside `FUN_801CFA48`**, and the
twelve-word signature at that VA occurs in exactly one image on the disc (PROT
0898, file `+0x13E0`). There is no cross-overlay read, and no second word living
at the same VA under a different overlay.

**Read the 0898 image for this one.** There is no `overlay_battle_action_801d0290`
dump; the only dump at that VA is an `overlay_0897` slice holding a *different*
five-instruction body that advances a VM PC in `s8` - a field-VM opcode-handler
fragment, and one that does not even match the field extraction's own bytes at
`0x801D0290`, so its program is mis-based as well. Disassemble the routine
instead:

```bash
scripts/ghidra-analysis/disasm-overlay-fn.py \
    extracted/overlays/overlay_battle_action_0898.bin \
    --base 0x801CE818 --addr 0x801d0290
```

Ported as `engine-vm::battle_action::OverlayRng`; the emitter that draws from it
is ported as `engine-core::effect_ribbon`.
