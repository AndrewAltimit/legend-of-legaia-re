# Battle action exit gates and softlock classes

## The `0x51` exit gate and the HP-bar settle invariant

State `0x51` (done / fade-down) leaves the action band only when
`ctx[+0x6D8] < 0 && ctx[+0x276] == 0`, and state `0x50` seeds that countdown
with `0x3C`. The countdown is **not** decremented unconditionally - the arm
gates it on a call:

```
801e6044  jal  0x801e7250          ; HP-bar settle check
801e604c  bne  v0,zero,0x801e60b8  ; "not settled" -> branch PAST the decrement
801e6054  lh   v0,0x2(s7)          ; s7+2 is ctx+0x6D8
801e6068  lbu  v0,0x393(v0)        ; DAT_1F800393, the per-frame delta
801e6070  subu a0,v1,v0
801e6074  sh   a0,0x2(s7)          ; ctx+0x6D8 -= delta
```

The branch target `0x801E60B8` rejoins after the store, so a "not settled"
answer skips the store and nothing else. A park at `0x51` with `ctx[+0x6D8]`
holding exactly the `0x3C` that `0x50` seeded, `ctx[+0x276] == 0` and a healthy
`DAT_1F800393` is therefore neither a stalled effect child, nor a zero frame
delta, nor a pinned census. The state machine is still being entered on its
normal cadence (once per game frame, which `DAT_1F800393 = 3` makes roughly one
vsync in three) and the `0x51` arm still reaches the `jal` - `FUN_801E7250` is
simply answering "not settled" every time.

### What `FUN_801E7250` measures

52 instructions; see `ghidra/scripts/funcs/overlay_battle_action_801e7250.txt`.
It reads the acting actor's active-target slot `actor[+0x1DD]` and branches on
the target class:

| `actor[+0x1DD]` | Result |
|---|---|
| `0`–`2` | 1 ("not settled") when that party actor's live HP `+0x14C` differs from its displayed HP `+0x172`; else 0. |
| `3`–`7` | 0 immediately - a monster target can never hold the exit. |
| `8` (all) | 1 when **any** slot below `ctx[+0x00]` has `+0x14C != +0x172`; else 0. |
| `> 8` | 0. |

Only party-side slots are ever inspected, in either arm. An action aimed at a
monster clears the gate on the frame it is asked, whatever the enemy's mirror is
doing; an action aimed at the party - an enemy attack, a heal, any all-target
cast - is the only kind that can be held. `ctx[+0x00]` is the party member
count, so the all-target arm is a party-side scan too.

### Why only the party side has anything to wait for

Retail draws **no HP readout for monsters**. The party HUD counts its HP down
over several frames after a hit; an enemy's HP is never shown at all. That is
what the whole asymmetry is for, and it explains three things that otherwise
look arbitrary:

- `+0x172` is maintained for monster slots but never drawn. `FUN_80047430`'s
  non-party arm therefore does not animate: it applies the entire delta in one
  frame and clears the accumulator (`0x80047578`). There is no readout to ramp.
- The settle gate returns 0 for monster targets because there is no animation to
  wait on - the wait exists purely to let the party's readout finish counting
  before the action ends.
- The same arm's `lhu` read of the signed accumulator, and the non-party MP
  path's use of the HP fields (see below), are unobservable in retail precisely
  because nothing renders the values they corrupt.

So "HP bar" is shorthand throughout this page for the **displayed-HP mirror**
`+0x172`, which is a drawn readout only on the party side. Readers of `+0x172`
in the corpus are UI-side: `FUN_80046A20` (`0x80046AA8`) and the
`FUN_801D8DE8` UI-element family (`0x801D9758`).

### The invariant the check assumes

Live HP `+0x14C` and displayed HP `+0x172` converge through a third field,
`actor[+0x10]`, a signed pending-delta accumulator. The per-actor tick
`FUN_80047430` (SCUS band, `see ghidra/scripts/funcs/80047430.txt`) drains it
into the bar. A **party** slot gets a quarter per game frame - `0x172 -= step`
and `acc -= step`, with `step` a divide-by-four biased so it is never zero for a
non-zero accumulator (`(acc+3)>>2` positive, `acc>>2` negative) - so the total
bar movement equals the seeded accumulator exactly and the sequence terminates
at zero for either sign. A **monster** slot instead takes the whole delta in one
frame and clears the accumulator (`0x80047578`), which is a second reason a
monster target never holds the `0x51` exit.

The whole ramp sits behind one guard at `0x800474E8`
(`lw a0,0x10(s2); beq a0,zero,<skip>`): **with a zero accumulator the bar is not
touched at all.**

Two retail quirks live in the non-party arms and are unobservable because
nothing draws a monster's readout. The HP arm reads the signed accumulator with
`lhu` (`0x8004757C`), so a negative accumulator on a monster - a heal - wraps
through the low halfword. And the non-party **MP** arm at `0x80047624` operates
on `+0x172` / `+0x10`, the **HP** fields, rather than `+0x174` / `+0x178`: a
copy-paste of the HP arm. The consequence is that a monster's MP accumulator
`+0x178` is never cleared, so that branch re-runs every frame for the rest of
the battle, subtracting an already-zeroed HP accumulator. Both are faithful
behaviour for a port to reproduce, not defects to correct.

That makes `+0x14C != +0x172` with `+0x10 == 0` on a party slot an **absorbing
state** for as long as the actor takes ordinary hits: the drain is the only
thing that moves the bar, and a subsequent damage or heal adds its own delta to
both sides so the constant offset rides along. One path does re-derive the bar
from live HP - the per-round status ticker
[`FUN_801E752C`](battle-action-helpers.md#fun_801e752c---per-round-status-dot-ticker) force-assigns
`+0x172 = +0x14C` right after its own HP write (`0x801E7600` and `0x801E7698`,
one per status bit) - so a poison or regen tick on the affected actor clears the
mismatch. That is the only re-sync in the dumped battle corpus, and it explains
why the softlock is survivable rather than terminal for a statused party.
Absent it, every action that targets the
party side reaches `0x51`, is told "not settled", and never decrements its
countdown. The battle camera's idle azimuth sweep (`FUN_801D0748` stepping
`_DAT_8007B792`) runs unconditionally and never consults the state machine, so
the visible result is a battle that keeps orbiting the acting actor forever -
an endless-camera-orbit softlock with no other symptom.

The park is directly reproducible: offsetting one party slot's `+0x172` by a
single point while clearing its `+0x10` is enough, and the next party-targeted
action hangs while monster-targeted actions in between still complete normally.
Probe: `scripts/pcsx-redux/autorun_gaza2_hpbar_settle.lua`.

### Phased crediting: the invariant breaks mid-action by design

A multi-strike action does not apply its damage once. The unsafe kernel
credits the bar accumulator **per strike** - each strike adds the same delta
`a0 = s0 - s1` to the per-action damage total `actor[+0x00]` (`0x801EDB40`)
and to the accumulator `+0x10` (`0x801EDB58`), paired stores off one register -
while live HP is committed **once, at the end of the resolution**, from the
accumulated total (`0x801EEA10`). Between the first credit and the commit the
readout is draining toward damage that live HP does not yet show, so a
watchpoint sees `+0x14C != +0x172` with `+0x10 == 0` - the absorbing shape -
**transiently, inside the action, as normal behaviour**. At the commit the
total that live HP absorbs equals the sum the bar was credited, and the pair
reconciles; the state `0x51` settle wait then holds the action open until the
tail of the drain lands. Measured end to end on the Gaza 2 save
(`autorun_gaza2_acc_discard.lua`, `invariant.csv` / `acc_writes.csv`): a
three-strike physical resolved as bar credits of 338 + 344 + 304 drained
per-strike, one live-HP commit of 986 at the end, and a bar that landed
exactly on live HP.

Two prior observations are re-attributed by this. The "party slot holding
live HP 266 with the bar drawing 0" capture that used to sit under the clamp
asymmetry below is a phased mid-action state - the window closed with a death
commit ~90 vsyncs later. And a mid-action watchpoint that samples between
strike and commit will always find "absorbing" shapes on healthy fights;
only a mismatch that **survives the action's own commit and settle wait** is
a real desync. No such survivor has been captured from retail-only play -
see [the instrumentation section](#consequences-for-instrumentation) for the
measured campaign.

### Where the desync comes from: two seeding conventions

Every writer of `+0x10` in the dumped battle corpus follows one of two
conventions, and they disagree about what to do when a new delta arrives while
the bar is still moving.

**The battle damage/heal kernel `FUN_801EC3E4` accumulates at every one of its
sites** (`see ghidra/scripts/funcs/overlay_battle_action_801ec3e4.txt`):

| store | shape | branch |
|---|---|---|
| `0x801EDAF0` | `acc -= (max - hp)` | overheal - live HP saturates at `+0x14E` first, so only the amount actually applied is credited |
| `0x801EDB14` | `acc -= (s0 - s1)` | ordinary net delta, paired with the live-HP write at `0x801EDAFC` |
| `0x801EDB58` | `acc += (s0 - s1)` | the second actor the same hit credits |
| `0x801EDB7C` | `acc = bar` | anti-overkill clamp, guarded `if (bar < acc)` at `0x801EDB70` - caps the drain at the whole visible bar |

Each is a read-modify-write, so overlapping hits compose and the invariant
`(+0x172 - +0x14C) == +0x10` survives every path through the damage kernel.

**The item / restore applier `FUN_800402F4` assigns.** Its head builds a pointer
table over `&actor[+0x14C]`, `+0x14E`, `+0x150`, `+0x152` for slots `0..6`
(battle mode `0x15`; a different source table otherwise), applies the restore
with `hp = hp + amount` at `0x800408AC`, and then seeds the bar with a bare
store:

```
800408f0  lw   v1,0x0(v1)      ; v1 = the actor
800408f4  subu v0,zero,v0      ; v0 = -amount
800408f8  jal  0x801e22c8
800408fc  _sw  v0,0x10(v1)     ; actor[+0x10] = -amount   <- the old value is never read
```

All three of its seeds - `0x800408FC`, `0x80040D28`, `0x800410BC` - are that
same shape.

**`amount` here is a signed stat change, not a damage magnitude** - which is what
makes the negation land on the *same* convention `FUN_801EC3E4` uses rather than
the opposite one. The `v0` being negated is the value folded into the stat
halfword sixty instructions earlier (`0x800408A8`: `lhu v0,0x0(v1)` / `addu
v0,v0,s4` / `sh v0,0x0(v1)`), so damage arrives as a **negative** `s4` and a heal
as a positive one, and `-s4` is positive-means-the-readout-falls either way.
Reading the seed in isolation is the reliable way to get the sign backwards.

Because none of the three seeds reads the old accumulator, **a restore that
lands while a damage drain is still in flight discards the remainder.** The rest
is forced arithmetic: with live HP `L`, bar `D` and remainder `A = D - L`, a
restore of `H` leaves live HP at `L + H` and ramps the bar from `D` to `D + H`,
so the bar settles exactly `A` above live HP with the accumulator back at zero.
That is the absorbing state above, reached through nothing but ordinary game
actions.

So the retail trigger is **healing a party member whose HP readout has not
finished counting down from a recent hit** - and the residual desync is exactly
the amount of readout movement the heal cancelled. Every later action whose
acting actor targets that slot (`+0x1DD` in `0..2`) or the whole party
(`+0x1DD == 8`, which is what a party-wide spell uses) then parks at `0x51`.

#### The auto-revive reaches the assigning seed by itself, one state early

The chain does not need a player to time an item badly, because state `0x50`
does it unprompted. `FUN_801E6968` - the Lost Grail **Final Heal** auto-revive
that `0x50` runs - calls `FUN_800402F4` twice, at `0x801E6A24` and `0x801E6BD0`,
both with `a0 = 4, a1 = 1`: **effect class 4** (revive) at tier 1 (full).
Class 4 dispatches through the applier's jump table at `0x80014FA0` into the
revive arm at `0x80040F14`, and that arm's accumulator seed - `0x800410BC` - is
one of the three bare assigns. Each call is guarded by
`lhu v0,0x14c(<actor>); bne v0,zero,<skip>`, so it fires only on a member whose
live HP has just reached zero.

On paper that is the worst possible moment: if the readout is still mid-drop
when the revive lands, the assign discards whatever is left of it, the bar
settles above live HP, and `0x50`'s only successor is `0x51` - the state that
asks whether the readout has settled. The park would land on the very action
that triggered the revive.

Measured, the race is **starved on the Gaza 2 fight**. The probe
`scripts/pcsx-redux/autorun_gaza2_acc_discard.lua` arms an Exec breakpoint on
every `+0x10` writer in the corpus plus the two Final Heal call sites, and a
capture campaign on the Gaza 2 save (Lost Grail armed on the party, no
harness write touching any HP / readout / accumulator field) drove **twelve**
auto-revives through `FUN_801E6968` across single-target and party-wide,
cast-path and kernel-path kills. **Every assign landed on an accumulator
already drained to zero**, margins 143-280 vsyncs. The starvation is
structural on this move set: [phased crediting](#phased-crediting-the-invariant-breaks-mid-action-by-design)
lands the bar credits per strike, *early* in the resolution, while `0x50`
arrives only after the remaining targets resolve and the effects tear down.

The margin is quantifiable from the same captures, and it is thin. Grouping
every party-side credit into actions and measuring last-credit to first
`0x51` settle check: minimum gap `90` vsyncs (~27 rendered frames at the
light-load 3-4 vsync cadence), median ~110-220. Against that, the biased
quarter-step (`acc -= (acc+3)>>2`) drains a full readout in a
size-insensitive ~20-30 frames - `600 -> 20`, `1289 -> 23`, `3000 -> 26`,
`9999 -> 30` (exact iteration of the retail step). A LV23 party (readouts
1289-1382, 23 frames) misses the fastest observed Gaza 2 tail by ~4 frames -
which is why twelve revives all came up clean - but the drain grows about one
frame per doubling while the action tail does not, and a `9999`-HP readout
(30 frames) crosses the fastest tail. **The prediction that falls out: the
discard-and-park fires for high-max-HP (late-game) parties killed by
fast-tailed moves, and cannot fire at low HP pools** - matching the
community's clustering of orbit reports on late-game bosses. Two caveats
bound the claim. The frame-vs-vsync clocking of the specific tail states
(timed states compensate by `DAT_1F800393`, animation waits do not) shifts
the line by a few frames either way. And a community capture of the live
softlock (Japanese version; the community reports the same park on both
regions) shows the parked fight's target panel drawing a **mid-game** pool -
`1476/1476`, displayed exactly at max - so a high readout is not *necessary*:
the parked fight's configuration crosses at ~1476.

Three follow-on measurements interpret that exhibit:

- **The HUD draws `+0x172` raw.** An injected overshoot (readout = live +
  150) renders on screen as `1439/1289` - over max, human-verified - and
  the drain (`FUN_80047430`) has no max clamp (no `+0x14E` read anywhere in
  it), so an overshot readout rides and *shows*. An over-max readout is
  therefore a distinctive on-screen witness of the discarded-restore
  direction worth searching community footage for.
- Because nothing clamps the drawn value, the exhibit's clean `1476/1476`
  panel is a **healthy displayed member**: the desynced slot is off-panel,
  and the parked action is party-targeted (`+0x1DD == 8`, the all-slot
  scan) - which is what a boss's party-wide cast uses, matching the
  community's "magic softlock" phrasing.
- **Party-wide cast damage flows through module-local appliers - paired.**
  In the capture campaign, the double-kill party-wide casts credited live
  HP and the accumulator through **none** of the census sites (kernel, item
  applier, enemy-cast safe applier, drain): the writers are inside the
  streamed **capture-class per-spell modules** (PROT 944..966; Gaza 2's
  Neo Star Slash is module 960 - see
  [battle-formulas.md](battle-formulas.md)), invisible to breakpoints
  armed on the resident-overlay copies. A static store audit of the whole
  family (`scripts/asset-investigation/audit_module_hp_stores.py`) finds
  every actor-accumulator store to be the **paired** accumulate + live-HP
  shape - module-local copies of the safe applier (e.g. 0944
  `+0x808`/`+0x824`) - so the module family follows the safe convention
  and cannot seed the desync by itself. Any future writer census must
  include the streamed module addresses, not just the resident overlays.

The same arm is what a **Phoenix** (class 4) reaches from the battle item
menu, and the class 0 / class 1 heal arms reach the sibling assign at
`0x800408FC`. But note the shape of the gate itself: a party-targeted action
holds its own `0x51` open until every party readout settles, so **the drain a
menu restore could interrupt has always finished before the menu can act** -
the inter-action race is closed by the very wait this page documents. The
intra-action Final Heal is the one crack, and it is measured tight above.

##### Port: the seed is per call site, and dropping it is unconditional

Retail cannot reach the absorbing state without the timing race above, because
the applier that writes live HP is the same routine that seeds the accumulator.
A port that separates them loses that guarantee: a heal that writes live HP
alone leaves `hp != hp_display` with `+0x10 == 0` on *every* use, not on a
raced one, and the next party-targeted action parks at `0x51` with no exit -
not even winning the fight, because the turn pump that resolves a wipe is the
thing waiting.

`engine-core`'s `World::use_item` is exactly that split: it is shared with the
field menu, where there is no readout to move. The battle call sites therefore
carry the seed themselves, through
[`BattleActor::assign_hp_bar`](../../crates/engine-battle-vm/src/battle_action/types.rs)
(the `-delta` assign, `battle_hp_bar::assign_pending`):

| Port site | Retail counterpart |
|---|---|
| `World::apply_battle_item` (the battle item menu's applier) | `FUN_800402F4` class 0 / 1 heal arms |
| `fold_spell_outcome`'s `Revive` arm | `FUN_800402F4` class 4 revive arm |
| `apply_final_heal_revives` | `FUN_801E6968`'s two `FUN_800402F4(4, 1, slot)` calls |

Damage and heal spells already route through `World::apply_battle_hp_delta`,
which is the accumulating convention (`FUN_801EC3E4`). Regression:
`engine-core` `a_battle_item_heal_keeps_the_readout_and_the_turn_pump_alive`
(pad-driven, disc-free) and the `item` rung of `engine-shell`'s
`battle_depth_replay`, which runs in the same fight as the rungs after it so a
park costs rungs instead of being restarted around.

`apply_final_heal_revives` runs earlier than retail's state `0x50`: the
port also sweeps right after a tick's damage lands, on the killing hit's own
tick, when that hit's ramp has not moved at all. Seeding the revive there
discarded the whole drop and left the readout above max HP by the member's
pre-hit HP (soak: `jouina`, a member downed and Final-Healed by one enemy
hit). The sweep therefore re-syncs the readout to the live zero before it
seeds, which is where the measured 90-vsync-or-more tail leaves retail's
readout; the high-HP fast-tail crack above is not reproduced.

#### The `(class, tier)` seed at state `0x3C`

State `0x3C` is the only writer of `actor[+0x1E8]` / `+0x1E9`, and the branch it
takes is the category byte `+0x1DE` (`0x801E3B70..0x801E3CB0`):

| `+0x1DE` | `+0x1E8` | `+0x1E9` |
|---|---|---|
| `1` (Item) | `+0` of the item-effect descriptor at `0x800752C0 + subtype*4`, where `subtype` is the item record's `+1` byte (`0x80074368 + id*0xC + 1`) | `+1` of that descriptor |
| anything else (Magic / Spirit) | `+0` of the spell record `0x800754C8 + id*0xC` | `+1` of the same record |

Both legs land in one class space. `0..=8` are the applier's effect classes -
heal / cure / revive / shield / buff, the same numbering
[`item-effect-table.md`](../formats/item-effect-table.md) documents - and the
larger values (`0x14` plain cast, `0x32` summon, `0x63` capture) are the spell
band's routing bytes. That is why the spirit that raises the shield is disc
data: a Spirit's spell record simply carries a small class byte.

Three consumers read the pair, all downstream of this one write: the applier
call at `0x801E4134`, the cue-group site it selects, and the cast-audio cue
`FUN_801F3990` fired one state earlier at `0x801E3E04`.

#### `FUN_800402F4`'s cue-group sites

The applier does one more thing on its way out of most arms: it asks the
cue-group expander `FUN_801E22C8` to place the arm's visual effect. It reaches
`jal 0x801e22c8` from **eleven** branches of its 132-entry class jump table at
`0x80014FA0`, and the group id is chosen per branch rather than passed in.

Arguments at every site are `(a0 = tint, a1 = actor-state word, a2 = actor
slot, a3 = group id)`. `a2` is `param_3` everywhere except the class-1 loop,
which passes its own loop index. Sites marked *gated* run only in battle mode
(`*(s16 *)0x8007B83C == 0x15`).

| `jal` | Class arm | Reached when | `a3` (group) | Gated |
|---|---|---|---|---|
| `0x800408F8` | 0 - HP restore, single | class 0 | `param_2` | yes |
| `0x80040D70` | 1 - HP restore, loop | per live slot in range | `param_2 + 1` | yes |
| `0x80040E54` | 2 - MP restore | class 2 | `param_2 + 3` | yes |
| `0x80040F04` | 3 - status cure | class 3 | `5` | yes |
| `0x800410B8` | 4 - revive | class 4 | `6` | yes |
| `0x8004111C` | 5 - spirit shield | class 5 | `7` | no |
| `0x8004157C` | 7 - stat buff | `param_2 == 1` | `8` | no |
| `0x80041718` | 7 | `param_2 == 2` | `9` | no |
| `0x800417FC` | 7 | `param_2 == 3` | `0xA` | no |
| `0x80041BA0` | 7 | `param_2 == 4` | `0xB` | no |
| `0x80041C60` | 8 - status clear | class 8 | `0xC` | yes |

Classes `6`, `9`, `0xA`, `0xB`/`0xC`/`0xD`, `0xE` and `0x82` never reach the
expander; class 6's own 7-entry inner table at `0x800151B0` only bumps
counters. The class-1 loop is bounded by `param_3 == 9` (monster slots `3..7`)
versus anything else (party slots `0..3`) and carries a second per-slot gate on
the roster byte (`DAT_8007BD10[slot]` below 3, `DAT_8007BD09[slot]` above), so a
party-wide restore fires the expander once per **seated** member - occupancy,
not liveness, so a downed member still gets the cue.

`a0` and `a1` are literals too, one pair per site - the tint and the actor-state
word the expander writes to `actor[+0x04]`. The class-`0` / class-`1` restore
arms and the class-`7` tier-1 / tier-2 buff arms pass the neutral tint
`0x00808080`, so those recolour nothing; the rest do. **The revive arm
(`0x800410B8`) is the one site whose `a1` is `0x20080200`** - the exact word the
expander tests for - so revive is the only arm that leaves `actor[+0x0C]`
alone.

So the whole selection is a `(class, param_2)` table plus one loop, and the port
carries it as `engine-vm`'s `battle_cue_group::cue_group_for` rather than
porting the applier's 1976 instructions. The port's own call site is the
applier's single SM one (state `0x3F`, `0x801E4134`): the acting actor's
`+0x1E8` / `+0x1E9` pair selects the site, the expander runs, and each cue goes
to the effect pool / SFX scheduler through the host. The `+0x1E8` / `+0x1E9`
seed itself is state `0x3C`'s - see the state table.

#### `FUN_800402F4`'s Stone and Curse arms (classes 9 / 10)

Two of the classes that never reach the cue expander are status inflictors,
and their class bytes appear in **no** spell or item record - a scan of the
whole spell table (`0x800754C8`) and item-effect table (`0x800752C0`) finds no
class-9 or class-10 row. The callers are the streamed capture-class boss
modules (PROT 935..966), which pass the class as a code literal and reach the
applier through runtime dispatch, so no static reference scan recovers the
pairing either. Decoded off the disc's own jump table at `0x80014FA0`: entry
`9` = `0x80041C70`, entry `10` = `0x80041E64`.

Both arms share one roll, in a single-target form (`sltiu v0,s0,0x3` - party
seats only) and a `param_3 == 8` all-party loop (one `rand()` draw per
member):

```text
80041c90  jal 0x80056798            ; rand()
80041cc4  addu v1, atk+0x168, tgt+0x168
80041cd4  div v0,v1 ; mfhi v1       ; roll = rand % (atk_agl + tgt_agl)
80041ce0  slt a0, tgt_agl, v1       ; lands when tgt_agl < roll
80041cf4  ori v0,v0,0x4             ; class 9: Stone   (class 10: ori 0x1000, Curse)
```

The group arm additionally zeroes a landed target's queued action category
(`sb zero,0x1de`) and refunds a reserved battle item
(`jal 0x800421d4` with `a1 = 1` when `+0x1DE == 1` and the initiative key
`+0x16C` is live). No guard-accessory read appears in either arm: retail
writes the bit unconditionally and the per-frame guard sweep (`FUN_8004CE2C`'s
guard-clear half) removes it next frame for a protected wearer.

Port: roll kernel `legaia_engine_vm::status_effects::agl_status_inflict_roll`;
live wiring `World::apply_enemy_agl_status` on the monster cast fold, with the
guard gate applied at infliction (steady-state-equivalent to retail's
clear-next-frame) and the move-id list carried there as a disclosed inference
(Glare `0x3C` capture-pinned, Stone Circle `0xB9` / Curse `0x40` / Curse All
`0x53` from record names + published behaviour). A landed Stone is what arms
the status-CLUT recolour (`FUN_8004CE2C`'s fourth pass,
`engine-core::battle_status_clut`).

#### Menu-committed actions must be claimed the tick they park

Three live-loop arms conclude a player's turn by writing `EndOfAction`
directly (the spell cast, the Spirit guard, the Tactical-Arts fallback), and
the monster cast fold does the same. The SM's own `0x5A` handler is not a
parking spot: retail's non-wipe arm advances `EndOfAction ->
PreActionWait -> ActionSeed` on the assumption that the flow SM
(`FUN_801D0748`) has already staged the *next* action. An engine arm that
parks and returns therefore hands the next tick's seed the **stale** action
bytes of the actor that just acted - which in practice was the battle-entry
attack queue, so every Spirit guard and every menu cast granted its actor a
free physical strike. The engine's rule: every site that writes `EndOfAction`
outside the SM step calls `World::cycle_battle_turn` in the same tick, so the
next combatant (or the round boundary) claims the state before the SM can
re-seed it. Ladder coverage:
`crates/engine-core/tests/seru_cast_magic_xp_ladder.rs` (the test that caught
the double-turn), `battle_item_cast_band.rs`, `battle_flee_ladder.rs`.

### The clamp asymmetry: two overkill guards against different references

The corpus contains two ways to apply damage to a party actor, and they clamp
overkill against **different** values. This is the generator that needs no
restore and no timing race at all.

The **safe** shape - the enemy-cast damage applier at `0x801E1924`, reached from
the cast dispatch just above it (`jal 0x801DD0AC` at `0x801E188C`) - clamps the
**damage** first and then applies that one clamped value to both fields:

```
801e1924  lhu  a0,0x14c(v1)   ; live HP
801e192c  sltu v0,a0,a1       ; if (hp < damage)
801e1938  move a1,a0          ;     damage = hp          <- clamp the DAMAGE
801e1944  addu v0,v0,a1       ; acc += damage
801e1948  sw   v0,0x10(v1)
801e195c  subu v0,v0,a1       ; hp  -= damage
801e1960  sh   v0,0x14c(v1)
```

Both fields move by the same amount, so the invariant holds by construction.

The **unsafe** shape is `FUN_801EC3E4`, where the two fields are written at
different times against different references. The bar accumulator is credited
while the action resolves and is clamped against the **displayed bar**
(`if (bar < acc) acc = bar` at `0x801EDB70`), while live HP is committed only at
the end of the action, from the separate per-action damage total `actor[+0x00]`,
and is clamped against **live HP**:

```
801eea10  lhu  a0,0x14c(v1)   ; live HP
801eea14  lw   v0,0x0(v1)     ; the action's accumulated damage
801eea1c  sltu v0,v0,a0       ; if (damage < hp)
801eea2c  _sh  zero,0x14c(v1) ;     else hp = 0          <- clamp the HP
801eea38  subu v0,a0,v0       ;     hp -= damage
801eea3c  sh   v0,0x14c(v1)
801eea74  sw   zero,0x0(v1)   ; damage total cleared
```

The two clamps agree only while `+0x172 == +0x14C` **at the action's start** -
and the `0x51` settle wait of the previous party-targeted action guarantees
exactly that. From a synced start the arithmetic is forced to agree: the
bar-side clamp trips only when the credited sum exceeds the starting bar,
which from a synced start means it exceeds starting live HP too, so the HP
commit floors at `0` on the same action and both readings land at zero
together (a kill, consistent). The asymmetry is therefore an **amplifier of a
pre-existing offset, not a standalone generator**: it needs `+0x172` already
below `+0x14C` when the action begins, which is the very desync whose origin
is in question. The earlier reading of this section - "reachable from
ordinary damage, no restore and no timing race" - is withdrawn; its
supporting capture (live HP `266`, bar `0`, zero accumulator on the Gaza 2
save) is a [phased mid-action state](#phased-crediting-the-invariant-breaks-mid-action-by-design)
that closed with a death commit, not a settled desync.

Three further guards above the commit (`0x801EE988`, `0x801EE9AC`, `0x801EE9EC`)
branch to `0x801EEB5C` / `0x801EEB60` and skip the live-HP write entirely. A
skip that fires with the bar accumulator already credited would move the bar
without moving live HP - a real credit-without-commit generator if any retail
path reaches it. No capture has caught one firing that way yet; which action
classes route through those guards is the open question this thread reduces
to.

Probe: `scripts/pcsx-redux/autorun_gaza2_hpbar_writers.lua` puts Write
watchpoints on each party actor's `+0x00` / `+0x10` / `+0x14C` / `+0x172` rather
than guessing store addresses, and Exec breakpoints on the commit and its skip
exits, so the writers name themselves and the pairing is auditable per frame.

### Consequences for instrumentation

Any intervention that force-writes `+0x14C` without re-seeding `+0x10` -
a capture-harness HP clamp, a max-HP cheat code, an engine debug key -
manufactures this park by construction. The failure shape is worth naming
because it is easy to mistake for the retail bug: the clamp restores live HP on
the frame damage lands, the in-flight accumulator keeps draining the bar
downward, the ramp then stops at zero accumulator, and the bar is left short of
a live HP that the clamp holds pinned at maximum. A "reproduction" captured
under such a clamp is measuring the instrument. A clamp that also assigns
`+0x172` and zeroes `+0x10` keeps the invariant intact.

Two further instrumentation facts, measured on the Gaza 2 save:

- **The stat aggregator does not tick during battle.** Poking an equipment id
  into a character record mid-battle never reaches the ability bitfield -
  `FUN_80042558` runs again only on isolated menu-side paths (observed twice
  in ~48k captured vsyncs). To arm an equipment-derived battle behaviour (the
  Lost Grail Final Heal bit `+0xF8 & 0x80`) from a save already inside the
  battle, seed the bit once alongside the equipment id - that mirrors what
  pre-battle aggregation of the same equipment would have left. The
  aggregation-derived bit is also cleared when the revive consumes the grail
  and any aggregator pass rebuilds the field, so re-arming between deaths is
  part of the same mirroring.
- **The watchpoint shape that matters is action-scoped, not frame-scoped.**
  Because of [phased crediting](#phased-crediting-the-invariant-breaks-mid-action-by-design),
  per-frame sampling flags absorbing shapes on healthy fights. The probe
  `autorun_gaza2_acc_discard.lua` therefore keys its verdicts on the two
  events that survive phasing: an assigning store landing on a non-zero
  accumulator (`discards.csv`), and a `0x51` settle verdict streak with a
  frozen `ctx[+0x6D8]` (`settle.csv`). A campaign of three such captures
  (~84k vsyncs, twelve Final Heal revives, one menu heal, no harness write to
  any HP / readout / accumulator field) produced zero of either.

### The exit is a test on the value, not on the crossing

Three details of the `0x51` arm decide *when* the band leaves, and all three are
about the shape of the test rather than the countdown itself
(`0x801E60B8..0x801E6148`):

1. **The decrement stops at the sign change.** `bltz v0,0x801E60C4` at
   `0x801E605C` skips the store once the timer is already negative, so the value
   parks at `-1`-ish instead of running away.
2. **`ctx[+0x276]` pins a floor of `0xC`.** `slti v0,v0,0xc` / `li v0,0xc` /
   `sh v0,0x2(s7)` at `0x801E60C0..0x801E60E4` re-raise the countdown to twelve
   for as long as the menu flag is up - so the band's own tail-cue block (which
   runs *below* `0xC`) never starts early, and when the flag drops the last
   twelve frames still have to run.
3. **The exit re-tests the value every pass.** `bgez v0,0x801E6158` at
   `0x801E60F0` reads `ctx[+0x6D8]` fresh; `bne v0,zero,0x801E614C` at
   `0x801E610C` then holds the transition while `ctx[+0x276]` is up. Nothing in
   the arm records "the countdown crossed zero on this frame".

Point 3 is the one a port can get wrong invisibly, because both spellings agree
on every pass where nothing else holds the band. They diverge only when the menu
flag is up on the single frame the countdown would cross: an
exit conditioned on the *crossing* consumes it and the state then has no exit at
all, which is a park rather than retail's bounded tail. The engine's
`done_fade_down` is written against the value.

The `0x52` branch carries a fourth: taking it **re-seeds** the countdown with
`0xB4` (`li v0,0xb4` / `sh v0,0x2(s7)` at `0x801E6134` / `0x801E6138`) before
stamping the state. (This state was once read as a "multi-cast continuation"; its only entry condition is the absorbed Seru in `ctx[+0x269]`, and the port names it `DoneSeruAbsorb`.) A port that routes to the Seru-absorb banner state without the
re-seed hands it a timer that has already expired, and a state waiting on a
countdown that can no longer start is the same park by a different route.

The `0x50` seed itself is `0x3C` on all three of its paths (`0x801E5EE8` /
`0x801E5EFC` / `0x801E5F24` all converge on the store at `0x801E5F28`), with one
override: `lbu v0,0x15(s5)` at `0x801E5F2C` re-seeds `0x96` when `ctx[+0x26]`
is non-zero. See [the level-up banner tail](#ctx0x26---the-level-up-banner-tail)
for what that byte is and what else reads it; the port carries the override on
`BattleActionCtx::levelup_banner_element`.

### `ctx[+0x26]` - the level-up banner tail

`ctx[+0x26]` is a **UI element id** - `0` means no element, and the only value
retail ever assigns is `0x65`, the *"<spell>'s magic level increased"* banner.
The reading is pinned by its third reader, not by its writers: `0x801E61B4`
passes the byte as the first argument of the UI-element unload,
`FUN_801D8DE8(ctx[+0x26], 1)`, in a run of sibling unloads that pass literal
element ids (`0x4E`, `0xF`, `0x52`, `0x44`).

Three sites write it. A `sb`-at-`ctx+0x26` sweep over `SCUS_942.54` and every
statically-based overlay image (tracking the ctx pointer from its
`0x8007BD24` load and any `addiu`-derived base) finds no others:

| site | what it does |
|---|---|
| `0x801E723C` | the in-battle magic level-up arm of `FUN_801E70BC` stores `0x65`, right after raising that element with `FUN_801D8DE8(0x65, 0)` at `0x801E722C` |
| `0x801E6D3C` | the Final Heal tail (`FUN_801E6968`) **increments** it on the arm gated `first_monster_id == 0xB5`, alongside `ctx[7] = 0xFD` |
| `0x801E2CFC` | `ActionSeed` clears it (`sb zero,0x15(s5)`, `s5 = ctx + 0x11`) at the head of every action |

and three read it, all inside the Done band:

| site | what it does |
|---|---|
| `0x801E5F2C` | the `0x50` seed above: `0x96` frames instead of `0x3C` |
| `0x801E6078` | the `0x51` fade-down's **banner skip**: once the countdown has sunk below `0x5B`, any pad activity (`_DAT_8007B874 \| _DAT_8007B938`, tested only for non-zero) snaps the timer to `-1` |
| `0x801E61B4` | the `0x51` teardown unloads the element it names |

The seed and the skip together are what make the tail read as a banner rather
than a fudge factor: `0x96 - 0x5B` = 59 frames the player cannot skip, then
the press ends it.

What the `0x801E6D3C` **increment** counts is nothing: it is a make-non-zero
idiom on a byte its readers only test against zero. Three things fix that.
`ActionSeed` clears the byte at the head of every action (`0x801E2CFC`), so
the value the increment reads is `0` unless the magic level-up arm stamped
`0x65` first, which makes the result `1` or `0x66` - neither an element retail
ever raises. The arm it sits in is a one-shot: it is gated on the first
monster slot being dead **and** `first_monster_id == 0xB5`
(`0x801E6CFC..0x801E6D0C`), so it runs once per battle at most. And the two
stores beside it are the same shape - `_DAT_8007B64A = 3` and
`ctx[+0x07] = 0xFD`, a state byte outside the SM's own range. What the arm
wants from `ctx[+0x26]` is the `0x50` seed's long hold (`0x96` frames instead
of `0x3C`) plus the skip window, which "non-zero" is sufficient for; the
unload that follows is then `FUN_801D8DE8(1, 1)` on an element that was never
raised.

The earlier note here left this "not settled by any of the three readers".
What settles it is the *writer* side rather than the readers: the increment
cannot be intentional element selection, because the byte it increments is
known-zero at that point and `0x65` is the only id ever assigned.

**Port.** `legaia_engine_vm::battle_action::done`'s `DONE_LEVELUP_BANNER_FRAMES` /
`DONE_BANNER_SKIP_BELOW`, over `BattleActionHost::pad_word`. The writer is
`World::accrue_summon_spell_xp` (`engine-core`), the port of `FUN_801E70BC`.
The `0x801E61B4` unload is ported with the rest of its block as
`done::done_band_ui_teardown`, so the id round-trips: raised with the element,
carried through the `0x50` seed, unloaded here at the id it was staged with,
cleared by the next `ActionSeed`. See
[the `0x51` teardown block](#the-0x51-teardown-block) for the latch that makes
it once-per-action.

### The `0x51` teardown block

Everything the `0x51` arm does after its exit test is one straight line
(`0x801E614C..0x801E6214`) behind two gates: the countdown must have fallen
below `0xC`, and the latch `ctx[+0x17]` must be clear. The latch is bumped at
the end of the block and cleared by the `0x50` entry (`sb zero,0x6(s5)` at
`0x801E5F60`), so the block runs exactly once per action however many passes
the band takes - and it runs on **every** pass, including the ones that store
a new state, because the exit branch at `0x801E610C` jumps to the head of this
block rather than to the epilogue.

| step | site | condition |
|---|---|---|
| sprite-handle table reset `FUN_801D99BC` | `0x801E6170` | unconditional |
| unload `ctx[+0x18]`, the action's own element | `0x801E6188` | byte non-zero |
| unload `0x4E` and `0x4F` | `0x801E61A0` / `0x801E61AC` | `ctx[+0x18] == 6` |
| unload `ctx[+0x26]`, the level-up banner | `0x801E61C4` | byte non-zero |
| unload `0x0F` and `0x52` | `0x801E61DC` / `0x801E61E8` | `ctx[+0x19]` non-zero |
| unload `0x44` | `0x801E6200` | `actor[+0x1DE] != 5` (not Run) |

### The sweep the teardown falls into (`0x801E6218..0x801E6368`)

The tail past the latch increment, and it is **inside** the latch: nothing
branches to `0x801E6218` (no word, `jal`, `j`, PC-relative branch,
materialisation pair or `gp`-relative access, in any of the 84 images), so its
only entry is the fall-through from `sb v0,0x6(s5)` at `0x801E6214`. Both of
the block's gates (`beq v0,zero` at `0x801E6158`, `bne v0,zero` at
`0x801E6168`) are conditional, and **when taken** they land on `0x801E6814` -
past the whole tail; the second reads the very byte `0x801E6210`/`0x801E6214`
increments. So the tail runs on the same once-per-arming pass the teardown
does. An earlier reading here called it "the unlatched multi-cast sweep"; the
gate on the latch byte refutes that. The gates do not *jump* - a reading that
says they do makes the tail unreachable, which the fall-through disproves.

| step | site | condition |
|---|---|---|
| `FUN_801E92DC(ctx[+0x269])` then **raise** `0x59` (`a1 = 0`) | `0x801E6234` / `0x801E6240` | `ctx[+0x269]` non-zero |
| unload `(id, id - 4)` for each queued entry, row `_DAT_801F6834 + (count-1)*4` | `0x801E62CC` / `0x801E62F0` | any of the four `0x801F6980` value slots non-zero, and `_DAT_801F6974` non-zero |
| unload `0x51` | `0x801E6344` | all four value slots zero, `actor[+0x1DD] - 3 < 5`, `0 < actor[+0x1DE] < 4` |
| unload `0x50` | `0x801E6360` | all four value slots zero, `_DAT_8007BD14` non-zero |

Two corrections fall out of the table. `0x59` is **raised** here, not
unloaded - `a1 = 0` where every unload in the block passes `1`; the `0x52`
Seru-absorb band is what closes it. And the whole multi-cast half is
`FUN_801E805C`'s teardown inlined: same `(id, id - 4)` pair, same
`(count - 1) * 4 + i` row, same `_DAT_8007BD14` gate on `0x50`, all of which
`engine-vm::battle_value_readout` already ports.

`ctx[+0x269]`'s reading follows from its second reader. `FUN_801E92DC` takes a
**spell id**: it resolves the acting slot through `_DAT_8007BD10`, indexes the
live game-state window at `0x80084140 + char*0x414 + 0x704` and prepends there
(ported as `engine-core::magic_xp::learn_spell_prepend`). So the byte the Done
band's exit test routes on is the spell a capture granted, and element `0x59`
raised beside it is that grant's banner.

The capture arm's window is narrower than the table suggests. A non-zero
`ctx[+0x269]` on the band's *exit* pass re-seeds the countdown to `0xB4`
(`sh v0,0x2(s7)` at `0x801E6138`) before the teardown reloads it at
`0x801E614C`, so that pass fails the `< 0xC` gate; and the menu-flag floor
writes `0xC` exactly, which fails it too. The frames that see both a live
capture byte and a running teardown are therefore the last twelve of the
countdown, before it crosses zero - once, behind the latch.

**Port.** `done::done_band_capture_and_banner_sweep` carries the `0x59` raise
and the `0x51` close; the loop stays with `battle_value_readout`, whose state
(the value window and the queued count) is the readout's rather than the
action's.

`ctx[+0x18]` is a **context** byte, written by the seed's Attack arm
(`li v0,0x7` / `sb v0,0x7(s5)` at `0x801E2F48..0x801E2F50`) - the same arm's
`sb t2,0xf(s5)` at `0x801E2F44` writes the acting *slot* to `ctx[+0x20]`, not
an element id to the actor. `ctx[+0x19]` is the Spirit-action latch the seed's
Spirit arm bumps at `0x801E2FF0`; nothing clears it, so once a battle has seen
one Spirit action the `0x0F` / `0x52` pair is dropped at the end of every
later action too.

**Port.** `done::done_band_ui_teardown`, over `BattleActionCtx`'s
`done_ui_torn_down` / `action_ui_element` / `spirit_action_count` /
`levelup_banner_element`. `FUN_801D99BC` - the per-actor UI-element array
zeroing the teardown calls - carries a scope row in `render_pipeline` instead:
the engine's HUD is rebuilt from state each frame, so there is no element array
to clear. The block does not end at the latch increment: it falls through into
the tail at `0x801E6218`, which is **inside** the same latch and is ported -
[the sweep section](#the-sweep-the-teardown-falls-into-0x801e62180x801e6368)
has the branch targets that refute the earlier "unlatched, unported" reading.

## The `0x19` attack-approach park - a second, distinct softlock class

The endless-camera-orbit symptom has (at least) two parks behind it, and the
first one caught **from ordinary play with no interventions at all** is not
the `0x51` HP-settle gate above - it is state `0x19`, the attack-approach
range poll. Caught live on the Gaza 2 fight by a human playing at
recompiler speed under `scripts/pcsx-redux/autorun_gaza2_park_hunter.lua`
(a poll-only probe - no breakpoints, so it runs under dynarec while a human
plays and savestates); the frozen moment is the catalogued scenario
`battle_gaza2_park_0x19` (`scripts/scenarios.toml`, identified by
fingerprint).

The arm's wait shape, from the `FUN_801E295C` dump (`case 0x19`): recompute
facing from the target's current position, call the range check
`FUN_8004E2F0(acting_seat, actor[+0x1DD])`, advance to `0x1E` only when it
returns 0. The not-in-range path is the shared stall label `0x801E35D0`,
which adds the frame delta to the stall counter `ctx[+0x6D4]` and breaks -
**state `0x19` contains no movement code and no timeout**. The walking
happens in the `0x16` advance loop, an earlier state. An action that
reaches `0x19` still out of range therefore re-polls forever.

Measured anatomy of the caught park (interpreter replay of the fingerprinted
save, probe `scripts/pcsx-redux/autorun_gaza2_range_wedge.lua`):

- The acting actor is the **boss** (seat 3, category 3 physical attack,
  871/15000 HP) targeting Gala (seat 2) across a ~556-unit gap; the range
  metric returns 554-557 every poll and the actor's position never changes.
- The reach the metric is compared against: party attackers use the static
  table `DAT_80078870[acting_seat]` = `{256, 384, 1024}`; monster attackers
  use a size-scaled reach (~416 for Gaza's `0x1A`). 556 > 416 - the check
  is honest, the boss genuinely needed to walk.
- The state trace into the park runs `0x0C -> 0x14 -> 0x19` within ~3
  vsyncs: **the walk phase never engaged** (root cause below - Gaza has no
  walk animation, so `0x14`'s fallback path skips the walk chain).
- The whole round queue is wedged in approach states simultaneously: Vahn
  polls his own queued attack against Gaza (metric ~458 > his 256 reach),
  and Noa and Gala sit with `+0x1DD == 8` - which `FUN_8004E2F0` rejects
  **by construction** (its head returns 1 for any target `>= 8`, so an
  all-target action can never satisfy a range poll).
- Every party HP triple is synced (`+0x14C == +0x172`, `+0x10 == 0`): the
  `0x51` HP-readout invariant plays no part in this park.

The camera orbit is the same pure symptom as ever: `FUN_801D0748`'s idle
azimuth sweep never consults the state machine.

### Root cause: the walk-tag fallback in state `0x14`

Why `0x16` never walked is answered by the `0x14` arm's out-of-range monster
branch, read from the raw disassembly
(`ghidra/scripts/funcs/overlay_battle_action_801e295c.txt`, `0x801E31F4..0x801E32DC`):

```text
801e31f4  jal 0x8004e2f0           ; range check (a0 = acting seat, a1 = target)
801e3200  bne v0,zero,0x801e3230   ; out of range ->
801e3230  ...sltiu v0,v0,0x3       ; party attacker?
801e323c  bne v0,zero,0x801e32c0   ;   yes -> anim 1, state 0x19 (anim-driven approach)
801e325c  lw  a0,0x0(v0)           ; monster: a0 = DAT_801C9348[seat-3] (record)
801e3260  li  a1,0x20              ;   walk tag
801e3264  lbu a2,0x4a(a0)          ;   action count at record+0x4A
801e3268  jal 0x80050e2c           ;   tag scan over the table at record+0x4C
801e327c  bne v0,0xff,0x801e32b4   ;   found  -> state 0x15 (walk start)
801e329c  li  a1,0x1               ;   NOT found: fall back to tag 1 (the Move loop)
801e32a4  jal 0x80050e2c
801e32ac  j   0x801e32c4           ;   -> state 0x19: the park
801e32b0  _sb v0,0x1da(s3)         ;   (delay: stage that clip)
```

`FUN_80050E2C` is a linear scan of the record's action-pointer table
(`record+0x4C`, `record+0x4A` entries) for an action whose **first byte**
equals the tag; it returns the entry index, or `0xFF` when absent (see
[monster-animation.md](../formats/monster-animation.md) for the tag space -
`0x20`/`0x21` are the attack pre-approach / close-in pair). A monster whose
action table carries **no tag-`0x20` action** can never enter the walking
states `0x15..0x18`; out of reach, it is dropped into the `0x19` poll with
the tag-`1` "Move" clip staged.

**The fallback normally still closes the gap - the park needs a third
condition.** A position capture of the same fight from ordinary play
(poll-only probe `autorun_gaza2_summon_displacement.lua`) shows Gaza's
fallback melee attacks approaching **during state `0x19`** at a steady ~19
units/vsync (e.g. 977 -> 377 over 28 vsyncs, then in-reach -> `0x1E`),
across four separate attacks. The SM arm still contains no movement code -
the displacement comes from the staged Move clip's playback on the
animation/driver side. In the caught park, by contrast, Gaza's anim bytes sat
at idle (`+0x1DA = +0x1D9 = 0`) and his position never changed from the
first poll. So the full park condition is (a) out of reach, (b) no tag
`0x20` - which removes the SM's own `0x16` stepping guarantee - and (c) the
animation-side approach drive not running. Note (a) is the **norm**, not an
anomaly: every healthy Gaza melee on record started out of reach (nearest
target 438-1078 vs ~416 reach; all went `0x14 -> 0x19`), and the parked gaps
(556 / 786 units) are ordinary formation spacing - the boss's model is
simply so large that center-to-center distances beyond reach still look
adjacent.

**The trigger is reproduced: a summon immediately followed by the boss's
melee.** The first directed attempt at that sequence parked, with the onset
on record (`gaza2_summon_displacement` capture; scenario
`battle_gaza2_park_0x19_summon_melee`): the summon stages Gaza to
`(0, -2048)` and back (his damage-reaction clip plays during staging), his
melee starts directly next, the fallback Move clip **engages** (anim pair
`+0x1DA/+0x1D9 = 1/1`, ~19 units/vsync toward the target) - and **dies ~12
vsyncs later** (pair drops to `0/0`, position frozen ~236 units in, still
beyond reach). Healthy contrast in the same capture: when any other action
sits between the summon and the melee, the same clip runs 28-67 vsyncs and
arrives (e.g. 64 vsyncs / 1,260 units). So the drive engages and terminates
early, and nothing re-stages it. The field the staging round-trip leaves
stale is pinned below - it is not a frame cursor or clip-length latch but
the actor's anim event-flag byte.

The trigger is **CPU-core-independent in emulation**: the same recipe parks
under both PCSX-Redux cores (recompiler and interpreter, via `run_probe.sh
--timing`) with the identical onset shape - restore interleaved with the
`0x14` melee setup, Move clip engaged at ~19 units/vsync, dead ~12-16
vsyncs in, frozen thereafter. So the race is not an artifact of dynarec
cycle accounting. Emulators whose CD latency model differs substantially
(e.g. image preload / async readahead) may schedule the summon's streamed
staging restore differently relative to the melee and rarely or never land
in the window.

### The stale field: `+0x1DC` bit 2, the exit-to-idle anim event flag

The clip that engages and dies is played by the per-frame anim-node tick
`FUN_80047430` / commit `FUN_8004AD80` pair (the driver documented in
[monster-animation.md § Playback](../formats/monster-animation.md#playback)).
Three of its properties, read from the SCUS disassembly
(`ghidra/scripts/funcs/80047430.txt`, `8004ad80.txt`), assemble the park:

- **The approach drive is the tick's root-motion term**, not the SM: while a
  clip plays, `0x80047D20..0x80047E18` adds `facing sin/cos ×
  entry[+0xC] × frame_dt × actor[+0x21D] >> 0xF` to the actor's position,
  gated on `+0x1DC & 8` clear and the range poll still failing. Gaza's Move
  entry has `+0xC = +20` and his speed scale `+0x21D = 8` - the measured
  ~19-20 units/vsync. The idle entry's `+0xC` is `0`, which is why a
  clip death freezes him. The shift floors once a battle frame, so a
  heading's small axis keeps a frame's worth of it; the port ticks once a
  vsync and spreads each frame's total over its ticks
  (`motion::RootMotionCarry`, over the battle frames
  `World::battle_frame_id` names - two vsyncs unless a replay installs
  another step, [`battle.md`](battle-stage-camera.md#the-battle-frame-step-is-the-frames-own-cost) - the strike loop's
  swing drift `0x801E386C..0x801E3994` takes the same carry). A per-tick
  shift had floored any axis under a unit a vsync to nothing: Vahn's
  strikes at heading `142` drifted straight up the `z` axis.
- **A looping clip has no loop counter - it loops by re-committing at every
  natural end.** When the cursor `node+0x68` passes the stream's frame
  count, the tick calls the commit, which re-installs the still-queued
  `+0x1DA` and zeroes the cursor. Gaza's Move stream is 5 frames at rate 2
  with speed scale 8 → one cycle ≈ 12 vsyncs; the healthy 28-67-vsync
  approaches are that cycle repeating seamlessly (pair stays `1/1`) until
  arrival.
- **The two commit sites treat the event-flag byte `+0x1DC` differently.**
  The natural-end path first tests bit 2 (`& 0x4`): if set, it **stages
  idle over whatever is queued** (`sb zero,0x1da` at `0x80047B44`), then
  clears bits 0-2 (`andi 0xF8`, `0x80047B50`) and commits. The mid-clip
  event path (bit 0 = commit now, bit 1 = commit at event frame,
  `0x80047A38`) clears only bits 0-1 (`andi 0xFC`) - **it preserves bit 2**.

Bit 2 is the *exit-to-idle-at-clip-end* flag, and its writer is the damage
primitive `FUN_800402F4`: a surviving target's light flinch is staged as
`+0x1DA = +0x1EF`, `+0x1DC |= 4` and `|= 1` (`0x80042124..0x80042170`) -
exactly what the summon's hit on Gaza does, which is the damage-reaction
clip `2/2` observed during staging. Normally the flinch's own natural end
consumes the bit (stage idle, clear, commit - the actor tweens back to
idle). The park is the race that breaks that round-trip: Gaza's melee
begins while the flinch is still playing, and state `0x14`'s walk-less
fallback stages the Move clip (`sb v0,0x1da(s3)` at `0x801E32B0`) with
`+0x1DC |= 1` (`0x801E32D4`/`0x801E35C4`). Bit 1 routes the Move install
through the **event-path** commit - the one that preserves bit 2. The Move
clip therefore engages carrying the stale exit-to-idle flag, and its first
natural end (~12 vsyncs in) stages idle instead of re-looping: pair `0/0`,
drive dead, and state `0x19` re-polls forever. Any interposed action gives
the flinch time to end and consume the bit, which is why only the
directly-following melee parks.

Causally verified on the parked save (probe
`scripts/pcsx-redux/autorun_gaza2_stale_flag_repro.lua`, write-watchpoints
on `+0x1DA`/`+0x1D9`/`+0x1DC` logging writer PCs). Control: bounce the
state byte to `0x14` - the SM re-stages Move (`0x801E3270` stores the
`0xFF` tag-`0x20` miss, `0x801E32B0` the fallback index), the event-path
commit engages it the same vsync, the first natural end 12 vsyncs later
re-commits with the pair still `1/1` (`0x80047B58` + `0x8004BDE0`), and the
boss walks in and strikes. Experiment: the identical bounce with
`+0x1DC |= 4` re-armed first reproduces the park signature - the same
re-staged clip dies at its first natural end via the `0x80047B44` idle
write, position frozen beyond reach, state parked in `0x19`. The park save
itself reads `+0x1DC == 0` because the killing end-path commit consumed the
bit (`andi 0xF8`); the flag's staleness is only visible in flight.

The shipped fix below is complete against this mechanism: by the time the
guard sees the dead clip the stale bit has already been consumed by the
very commit that killed the clip, so the `0x14` re-stage it forces runs
with bit 2 clear and loops clean - which is what the fix-verify replay
shows.

Confirmed against the parked save itself (RAM read via `legaia-pcsxr`,
example `gaza2_walk_tag`): Gaza's seat-3 record holds 12 actions with tags
`[00 01 02 03 04 05 0B 0E 13 0C 23 23]` - **no `0x20`**. Bosses generally
lack the tag, which matches the community's clustering of orbit reports on
late-game bosses. Measured drift sources from the same capture: each summon
stages Gaza ~2,500 units off-arena (`(0, -2048)`) and restores him with a
**committed ~35-40-unit home drift per cast**; his own casts nudge his home
16-26 units without moving him; his melee both approaches and commits the
new position.

Two closing observations from the same replay:

- `ctx[+0x6D4]` (the stall counter the park increments) and `ctx[+0x6D2]`
  (the facing delta `0x14` computes) are **not a timeout**: their only reader
  is the arms/interference resolver `FUN_801EC3E4`, which adds them into an
  agility-style contested roll. Nothing bounds the park.
- The queued `+0x1DD == 8` values on the idle party actors are stale
  all-target sentinels on actors that never got their turn - the round is
  stuck on the boss's action alone, not on a corrupted queue.

**Fix (disc patch).** `legaia-patcher --approach-softlock-fix`
(`legaia_patcher::approach_fix`) rewrites the `0x19` arm's redundant
per-frame facing recompute (nine words at `0x801E3568` - the target never
moves during an approach, and states `0x14` and `0x1E` re-derive facing
themselves) into a guard: staged clip dead while the poll still fails ->
bounce the state byte to `0x14`, whose retail arm re-runs the whole staging
next frame, so the monster **resumes walking** - no invented behaviour, and
the same bounce rescues a party attacker whose run clip dies. The
no-Move-clip edge is vacuous: a roster sweep (`monster_move_tags`) finds all
186 monsters carry tag `1`. Runtime-verified hands-off on **both** live
park savestates (`autorun_gaza2_approach_fix_verify.lua`: poke the nine
words, touch nothing else - the guard bounces once, retail re-stages
`1/1`, the boss walks in at ~19 units/vsync, the strike lands, the round
completes); the byte-level edit is pinned by the `approach_fix_real` disc
oracle.

**Engine port note.** The engine walks: the live host's `range_check`
computes the retail law (`World::battle_range_metric`, the `FUN_8004E2F0`
port) and the approach movement runs as the root-motion drive
(`World::tick_battle_locomotion`, the `FUN_80047430` term - clip entry-speed
`+0xC` when a committed clip carries one, else the captured Move-drive
fallback), so a melee attacker physically closes on its target, strikes and
stays where it struck ([below](battle-action.md#where-an-action-leaves-its-combatants)). The
port still cannot reproduce this park, now for a
stronger reason: the locomotion drive runs in every approach state whether
or not a clip is playing - the engine-native form of the
`--approach-softlock-fix` guard - so an approach state always closes. The
monster routing is retail's: every `FUN_80050E2C` call site the action SM
carries - the `0x20` / `1` pair in `0x14`, the walk in `0x15`, the close-in
in `0x16`, the capture takedown's walk and the `0x22` knockout taunt below -
runs the tag search over the monster's installed action table
(`legaia_engine_vm::battle_action::monster_action_by_tag`, fed by
`World::battle_monster_action_tags`), so the 180 records with no
tag-`0x20` entry take `0x19` exactly as retail does and the six that carry
one play their own pre-approach and close-in clips.

The engine did park in `0x19` for a different reason, and the fix is worth
recording because the symptom is identical. It held every actor's
`+0x3C`/`+0x40` pair still for the length of an action as a "seat", where
retail re-derives it every frame
([below](battle-action.md#where-an-action-leaves-its-combatants)). The separation pass
measures overlap on that pair and nudges the live pairs, so two party
members whose held pairs overlapped were pushed apart every frame without
the overlap ever clearing, walked tens of thousands of units off the stage,
and a monster's approach - clamped at its target's live pair, measured
against the stale one - never came in range (Zora in `taiku`, Rogue in
`rugi`). The disc-gated `boss_approach_disc` test fights both, and
`monster_approach_sweep_disc` fights every one of the 186 archive records.

**The knockout taunt.** On the way into the Done band (`0x801E5594..
0x801E5658`), a monster whose attack has left its target at zero HP searches
its action table for tag `0x22` and, while at least one party member still
stands, stages that entry with the stage latch `+0x1DC |= 2`. The wiping blow
therefore plays no taunt, and most of the roster (166 of 186 records) carries
no `0x22` entry at all. Port: `attack::stage_ko_taunt`.
