# Lane 3 - battle depth: a pad-driven command ladder, and four disclosed divergences settled

Two pieces of work that turned out to share a root: **nothing in this repo
pressed a button at a battle command other than Arts**, and the one command
that *did* have a pad-driven test carried a disclosure comment describing
retail behaviour that retail does not have.

## Part 1 - the ladder

`crates/engine-shell/tests/battle_depth_replay.rs` (new). A disc-gated,
pad-driven ladder that boots `town01` off `extracted/`, enters the real Rim
Elm sparring battle (MAN formation 4, the 999-HP opponent - long-lived enough
to carry eight rungs in one fight), and issues each command class through
`World::set_pad` + `BootSession::tick`.

### Why it did not exist

The critical-path replay fights one random encounter with `set_pad(0)` every
frame, on purpose - pressing into it would make the walk's score a function of
the battle UI. That works only because it never sets `battle_player_driven`,
so the SM auto-resolves each party turn as a physical Attack. Everything else
was covered at SM level, at formula level, or by
`crates/engine-core/src/world/tests/` calling `pub(in crate::world)` methods
directly, which bypasses `World::tick` and is unreachable from an integration
test. A submenu could be wired to nothing and stay green everywhere.

### Rungs - measured **8 / 10**

| # | rung | result |
|---|---|---|
| 1 | battle entered, command menu open | pass |
| 2 | Attack | pass |
| 3 | Arts entry (`Attack` -> `Command`, debits AP) | pass |
| 4 | Arts executed (`Begin` runs the typed sequence) | pass |
| 5 | Magic (casts, charges MP) | pass |
| 6 | Summon (`0x81..=0x8B` raises a summon spawn) | pass |
| 7 | Item (menu consumes a carried item) | pass |
| 8 | Spirit (charges AP, raises the guard stance) | pass |
| 9 | Run refused under `battle_no_escape` | **blocked** |
| 10 | Run escapes | **blocked** |

`BASELINE = 8`, ratcheted in-file (a reviewed edit, same contract as the
critical-path baseline). Skip-pass proven by contrast: with the disc the run
clears 8 rungs and prints the stall; with `LEGAIA_DISC_BIN` unset it prints
`[skip]` and finishes in 0.01 s.

### The two engine findings the ladder produced

**1. Run is unreachable from the pad.** `CommandPhase::RoundPrompt` exists and
`step_round_prompt` implements both chips (Circle -> `RunAway`, and the
`no_escape` refusal), but **no battle ever opens it**. Every battle's opening
command session observed - the first fight and every later one - is
`Menu(Some(Item))`, the four-arm ring. Since Run lives only on the round
prompt, a player currently cannot flee a battle at all. Rungs 9 and 10 are
blocked on this, which is why they are last.

**2. Three commands park the fight.** After a **Summon**, an **Item** use or a
**Spirit** guard, no further party command session opens - measured to 9000
ticks (2.5 minutes of game time) with the mode still `Battle` and every
surface closed. A parked battle has no in-battle exit: not even winning it,
because the turn pump that would notice the opponent at 0 HP is the thing that
stopped (measured with the opponent at 1 HP and the mode still `Battle`).
Attack, Arts and Magic do **not** park it.

That is why rungs 7, 8 and 10 each take a fresh battle through a host-level
scene restart (`reset_to_field`). Without it only one parking command could
ever be measured - whichever ran last - and the other two would be
indistinguishable from broken.

### Coverage, with its N stated

`cargo llvm-cov -p legaia-engine-shell --test battle_depth_replay` joined
through `scripts/ci/replay-port-coverage.py`:

| | |
|---|---|
| ported anchors | 834 |
| statically live / **entered by this ladder** | 688 / **143** |
| statically not-live / entered anyway | 146 / **0** |
| `NOT WIRED`-disclosed anchors executed | **0** |
| live, never entered | 461 |
| not observable in this binary | 108 |

**This is the ladder's own number, not a delta.** The wave brief's figure of
132 belongs to `critical_path_replay`, a *different* test; computing a real
"previously-unentered" count needs the union of the two coverage sets, and I
did not run the critical path under instrumentation. Reporting `143 - 132 = 11`
would be a subtraction across two different denominators. What is measured
here: this ladder alone enters 143 live anchors, executes no inert anchor, and
executes nothing carrying a `NOT WIRED` disclosure.

### Rows the ladder reached the surface of and still never entered

The wiring signal. Render-side rows (`battle_intro_*`, `battle_camera`,
`battle_hud`) are excluded - this is a headless sim run, so those are expected,
not findings. What remains is simulation-side code whose surface the pad
*did* drive:

| anchor | site | the ladder did |
|---|---|---|
| `801d8f10`/`801d9110`/`801d9280`/`801d9594` | `engine-core/src/spell_menu.rs:38-41` | cast twice through the battle magic surface |
| `801d8a88` `build_attack_target_queue` | `engine-vm/src/battle_action/pool_ops.rs:397` | confirmed target pickers |
| `801d8d00` `cycle_attack_target` | `engine-vm/src/battle_action/pool_ops.rs:205` | confirmed target pickers |
| `801db124` `redirect_dead_target` | `engine-vm/src/battle_action/pool_ops.rs:295` | fought with dead monsters present |
| `80046a20` `battle_gauge` | `engine-vm/src/battle_gauge.rs:3` | charged AP via Spirit |
| `801cf650` `compute_battle_stats` | `engine-core/src/battle_stats.rs:223` | fought four real battles |
| `801db8b4` `first_living_monster` | `engine-core/src/battle_round.rs:197` | ran full rounds |
| `801dba90` `battle_cast_dispatch` | `engine-vm/src/battle_cast_dispatch.rs:6` | cast a spell and a summon |
| `801dceac` `target_group_range` | `engine-vm/src/battle_target_group.rs:181` | cast a group-capable summon |
| `801e22c8` `expand_cue_group` | `engine-vm/src/battle_cue_group.rs:134` | cast a spell and a summon |

The `spell_menu.rs` row is the sharpest: the battle magic surface is
`battle_magic::BattleSpellSession`, and `battle_magic` / `inventory_use` /
`arts_command_input` / `ap_gauge` do **not** appear in the unentered list at
all. So `spell_menu.rs` is a parallel module the battle path does not use -
orphan-module wiring debt of the shape wave 18 catalogued, not a dead
anchor.

### Two measurement traps this file walked into first

Both are worth knowing because both produced a *confident wrong answer*, not
an error.

**Ordering a cumulative ladder is a measurement decision.** The first pass put
Item at rung 5, scored 5, and stalled. The natural reading - "Item is the
first broken command" - was wrong twice over: Item works, and the four rungs
behind it (Magic, Summon, Spirit, Run) were not *failing*, they were
**unmeasured**. Unmeasured and broken print the same way in a ladder that
stops at the first stall.

**The two-AP-systems trap, live.** The Spirit rung first asserted on
`World::spirit_gauge()` and reported "Spirit never charged the gauge". Wrong
gauge: `Spirit` charges `World::ap_gauges[slot]` (`ApGauge::charge_spirit`,
+5) and raises `battle_guarding[slot]`, while `spirit_gauge()` reads
`actors[slot].battle.spirit_gauge` - the spirit-**art** meter. Worse, it read
plausibly in *both* directions: ordinary combat moves the spirit-art meter on
its own, so an earlier ordering of the ladder **passed** that rung for a
reason with nothing to do with the press. A rung that can pass without its own
cause is not measuring its command. The assertion now watches the AP gauge and
the guard stance.

### Things a driver has to know

These cost time to rediscover, so they are in the module docs too:

- **Arts is not a ring arm.** It is `Attack` -> `AttackMode::Command`. A driver
  that walks the four-arm ring looking for "Arts" never finds it.
- **Run is not a ring arm.** It is on the round prompt - which, per finding 1
  above, no battle currently opens.
- **There is no Summon arm.** A summon is a spell with id `0x81..=0x8B` cast
  from the Magic submenu, drained by the host via `take_pending_summon_spawn`.
- **Retail's player spell catalog is *only* `0x81..=0x8B`.** `enter_field_live`
  installs `retail_magic::retail_seru_magic_catalog`; all eleven ids are inside
  `summon::SERU_SUMMON_IDS`, so there is no non-summon player-castable spell to
  pick. Magic and Summon are split by what they assert, not by which spell.
  (`SpellCatalog::vanilla()`'s `0x10..` ids are a disc-free fixture, not what
  boot installs - reaching for them stocks spells the menu never lists.)
- **Item ids: Healing Leaf is `0x77`.** `ItemCatalog::vanilla` carries the real
  retail ids and has a unit test guarding against "the old fabricated `0x01..`
  sequence". An id outside the catalog reads as a broken Item arm.
- **`no_escape` is copied into the session when it opens**, so setting the
  world flag under a session already on screen changes nothing.

Also: every surface reads `just_pressed`, so a held mask is one event - presses
are two-frame taps. The battle item menu maps only Up/Down/Cross/Circle, so
there is no horizontal navigation to drive. And `arts_input_active()` stays
true through Review / Begin|Reselect / Targeting, so an arts driver must loop
on the *phase*: keep pressing a direction while merely "active" and it walks
past the auto-end, toggles the cursor onto **Reselect**, and the next confirm
wipes the buffer back to a fresh entry - a perfect loop that never strikes.

Setup stocks the *player* (spells learned, items carried, MP, and a party and
opponent durable enough to outlast tens of thousands of frames) because a
player with none cannot press the button either. The commands themselves are
all pad-driven; nothing calls a battle internal.

## Part 2 - the four disclosed divergences

`crates/engine-core/src/arts_command_input.rs` disclosed four ways the port's
Arts entry differs from retail. Settled against the disassembly of
`FUN_801D0748` state `0x50` and `FUN_801D388C`, using the base-tagged
`overlay_0898` dumps. (All five dumps at each address are byte-identical -
11124 bytes / 2781 instructions for `801d0748` - so there is no aliasing
question here; only the `overlay_0898` pair carries the explicit base tag.)

### The trap that made two of them look real

`FUN_801D0748` tests the pad against `s2`, built at `801d0b20` as
`_DAT_8007B874 | _DAT_8007B938`. That is the **packed** pad word, whose byte
halves are swapped against the raw BIOS word
(`world_map_panel_host::packed_pad`: raw Cross `0x4000` -> packed `0x0040`, raw
Up `0x0010` -> packed `0x1000`).

Read raw, the entry's four direction tests at `801d1e60`..`801d1f38`
(`0x8000 / 0x1000 / 0x4000 / 0x2000`) look like Square / Triangle / Cross /
Circle - i.e. the face buttons - which makes the confirm and cancel masks look
unreachable, because Cross and Circle appear to be *directions*. Packed, they
are Left / Up / Down / Right, the d-pad entry the game actually has, and the
face buttons are free to be confirm and cancel. Both falsified rows below
follow from getting this the right way round.

### Verdicts

| # | disclosed as | verdict | evidence |
|---|---|---|---|
| 1 | pool seeds AGL, `DEFAULT_POOL=100` fallback | **correct**, keep | disassembly |
| 2 | "Cross confirms early; retail only auto-ends" | **falsified**, port was faithful | disassembly |
| 3 | "Circle backs out; retail cannot back out at all" | **falsified**, port was faithful | disassembly |
| 4 | art body not charged from Spirit | **correct**, real open gap | disassembly |

**1 - pool seed. Keep.** Retail seeds `ctx+0x6DC` from the acting actor's AGL:
`801d3a28 lhu v0,0x154(v0)` -> `801d3a30 sh v0,0x6(s6)`. The port does the
same; `DEFAULT_POOL` applies only with no roster loaded, which retail never is.
Reworded from "divergence" to "disc-free fallback" - it is not a behavioural
difference in disc-backed play.

**2 - early confirm. Falsified; the port was already faithful.** Retail ends
the entry on the configurable confirm mask `_DAT_800846D0` (the same mask every
menu in the game reads; measured `0x44` at the S3 anchor):

```text
801d207c  lbu  v0,0x8(s1)        ; committed count ctx+0x19
801d2084  beq  v0,zero,0x801d20e4 ; empty buffer skips to the cancel test
801d2098  lw   v0,0x46d0(v0)     ; _DAT_800846D0
801d20a0  and  v0,s2,v0
801d20ac  sb   v0,0x0(s3)        ; ctx+0x06 = 0x5A
```

The gate is the committed count - and that is *exactly* the port's
`ev.cross && !self.buffer.is_empty()`. The port matched retail; the comment
claiming otherwise was wrong. Disclosure removed, sites cited instead.

**3 - back-out. Falsified, and it concealed a real bug.** Retail backs out on
the sibling mask `_DAT_800846D4` (`801d20ec`), which forks on the same count at
`801d210c`:

- **buffer empty** -> leave the entry, to the attack-mode prompt `0x78`
  (`801d219c`/`801d21a0`), or the ring `0x28` (`801d218c`) when
  `_DAT_800846C4` is set. **The port already did this.**
- **buffer typed** (`801d2114 bne v0,zero,801d21a8`) -> `FUN_801D388C` case
  `0x26`, which wipes all sixteen queue bytes
  (`801d52d4 sb zero,0x1df(v0)` under `801d52d8 sltiu v0,s3,0x10`), re-seeds
  the pool from AGL (`801d535c lhu v0,0x154(v0)` -> `801d5364 sh v0,0x6(s6)`)
  and zeros the count (`801d536c sb zero,0x8(s4)`). Nothing writes `ctx+0x06`,
  so the flow **stays in `0x50`** - it is a restart, not an exit.

The port ignored Circle on a typed buffer entirely. **Fixed**: it now clears
the buffer, clears `spent` and restores the pool to `pool_max` - which is the
same thing the port's existing "Reselect" chip already did, so retail's model
was already implemented, just not reachable from this press. Unit test
`circle_on_a_typed_buffer_resets_the_entry_instead_of_leaving_it` pins the
fork against the pre-existing `circle_on_empty_buffer_aborts`.

This is the one behavioural change in the lane, and it is a **bug fix toward
retail**, not an enhancement.

**4 - art body from Spirit. Keep as a disclosed open gap.** Real, and already
decoded elsewhere in `arts-command-gauge.md`: retail pays the art body out of
`actor[+0x170]` through the accumulator `actor[+0x224]`, spent in the
battle-action cleanup arm. Wiring it needs the accumulator, which is battle
action state, not entry state - out of this lane's scope. The comment now says
"open gap" rather than sitting in a list next to two non-divergences.

### Doc changes

`docs/subsystems/arts-command-gauge.md`: the "What still diverges" table had
the two falsified rows (they were the source the module comment was written
from). Table corrected, and a new **Leaving state `0x50`** section carries the
three exits with their instructions, plus the packed-pad warning.

## Residue / what a follow-up should take

**The two engine defects above are the headline work items**, and neither is
in this lane's scope to fix (both live in `world/battle/loop_driver.rs` and the
action SM, which other lanes hold):

1. **No battle opens the round prompt** -> Run is dead from the pad. Start at
   `arm_round_open_prompt()` (`loop_driver.rs` ~:212) and ask what gates it;
   retail sets `0x1E` unconditionally out of state `0x14`, with a back attack
   (`ctx[+0x290] == 1`) the only documented skip.
2. **Summon / Item / Spirit park the fight** -> the turn pump stops. All three
   resolve *without* a strike, which is the obvious shared suspect: Item and
   Spirit both park at `ActionState::EndOfAction`, and the summon hands off to
   the host. Attack / Arts / Magic all resolve through a strike and do not
   park. A fix wants a test that asserts the *next* command session opens, not
   just that the command's own effect landed - which is precisely what rungs 7
   and 8 stopped being able to assert once they each needed a fresh battle.


- **The Spirit charge for the art body** (divergence 4) is the one real
  remaining gap in this area. It is a battle-action change, not an entry one.
- **Saved chains still do not preseed the entry.** `arts-command-gauge.md`
  already records this; retail's `FUN_801DA34C` copies a 16-byte string into
  `actor[+0x1DF..]` when the entry opens. The open question it names - whether
  preseeded presses arrive already paid for or re-debit the pool - is now
  sharper, because case `0x26` shows the pool being re-seeded from AGL whenever
  the queue is wiped.
- **`_DAT_8007B938`** (the second word OR-ed into `s2`) is not identified. Not
  load-bearing for anything here, but it is the difference between "edge" and
  "edge or repeat" if someone needs auto-repeat semantics.
