# Lane D - the enemy status-effect applier gap

Verdict: **closed for the enemy special-attack channel**, with three named
residuals disclosed below. The gap was *not* where the brief's premise placed
it, and the reason it stayed open for so long is a mislabel in committed prose,
not a missing parser.

Everything in the "retail chain" section below rests on a **fresh read of the
disassembly** in `ghidra/scripts/funcs/`, not on committed docs. Where I lean on
a committed doc I say so explicitly.

---

## 1. The retail chain, from the instructions

There are **two** status-infliction legs, and they do not share a source.

### The special-attack leg - `FUN_801E09F8` (this is the monster -> party one)

1. **`FUN_801DEA50`** (action setup) reads the acting actor's queued move id at
   `actor[+0x1DF]`, maps it through the 128-byte id -> index table at
   `0x801F4E63`, and parks the resulting **move-power record** pointer in the
   battle context:

   ```text
   801df248  lw    v1,0x0(s5)          ; the acting actor
   801df24c  addiu v0,v0,0x4e64        ; map base is one BELOW this
   801df250  lbu   v1,0x1df(v1)        ; move id
   801df258  addu  v1,v1,v0
   801df25c  lbu   v1,-0x1(v1)         ; map[move_id]
   801df264  sll   v0,v1,0x1
   801df268  addu  v0,v0,v1            ; 3a
   801df26c  sll   v0,v0,0x2           ; 12a
   801df270  addu  v0,v0,v1            ; 13a
   801df274  sll   v0,v0,0x1           ; 26a   <- the 26-byte stride
   801df27c  addiu v1,v1,0x4f5c        ; table base 0x801F4F5C
   801df284  sw    v0,0x1014(a0)       ; ctx[+0x1014] = &move_power[idx]
   ```

2. **`FUN_801E09F8`** (per-frame action tick) waits for the strike arm's phase
   byte to reach the impact value `3`, then reads the record's `+0x0A`:

   ```text
   801e156c  lbu  a2,0x24e(v0)         ; ctx[+0x24E + arm] = phase
   801e1570  li   v0,0x3
   801e1574  bne  a2,v0,0x801e1a6c     ; not impact -> skip the block
   801e157c  lw   v0,0x1014(v1)
   801e1584  lbu  v1,0xa(v0)           ; <- THE STATUS BYTE
   801e158c  beq  v1,zero,0x801e1788   ; zero selector -> nothing
   801e15ac  sb   v1,0x21f(v0)         ; lingering-status visual latch on TARGET
   ```

   Note `a2` still holds `3` after the `bne` proves it - the routine reuses it
   as the comparison constant for the byte-3 arm at `801e1610`. That register
   economy is why a byte-level reader sees no `li ...,3` for the Venom arm.

3. The four-way ladder (`0x801E15F8..0x801E1788`):

   | selector | arm | PCs |
   |---|---|---|
   | `3` | `rand & 7 == 0` -> `+0x16E \|= 1` (**Venom**) | `801e1630..801e165c` |
   | `4` | `rand & 7 == 0` -> `+0x16E \|= 2` (**Toxic**) | `801e1660..801e168c` |
   | `5` | target slot `< 3` (`sltiu v0,a1,0x3`), then char `+0xF4` bits `0x0100_0000` (Rot Guard) / `0x1000_0000` (Master Guard) bail out, else `+0x16E \|= 1 << (rand%3 + 3)` (**Rot**) | `801e1690..801e1740` |
   | else | nothing | - |

   The guard-bitfield read (`lw v1,0x6bc(v0)` at `801e16cc`) **precedes** the
   `jal 0x80056798` at `801e16fc`, so a guarded or non-party target draws no
   RNG at all.

### The physical/arts leg - `FUN_801EC3E4` (party -> monster)

Same ladder shape, different source: the **art record**'s `+0x7A`, reached
through the `param_2` spill at `0x54(sp)`:

```text
801ee3cc  lw   t4,0x54(sp)
801ee3d4  lbu  v0,0x7a(t4)
801ee3e0  sltiu v0,v0,0x6         ; <6 gates the +0x21F latch + tint
801ee448  lbu  v1,0x7a(t4)        ; the ladder proper
```

### The asymmetry nobody had recorded

`FUN_801EC3E4`'s ladder has a **fifth arm**: `li v0,0x6` / `beq` at
`0x801EE478..0x801EE47C` -> `andi v0,v0,0x3` / `ori 0x1000` (Curse, 1-in-4) at
`0x801EE698..0x801EE6C8`. `FUN_801E09F8`'s ladder **stops at 5** -
`0x801E1620` compares against `5` and otherwise jumps straight to the join at
`0x801E178C`. **An enemy special attack cannot inflict Curse in retail.**
`docs/subsystems/battle.md` claimed the cast leg "routes to the same band" for
byte 6; that is false and is now corrected.

---

## 2. Which link was missing

Not the parser, and not the tracker. Both ends already existed:

- `legaia_asset::move_power::MoveRecord::impact_effect()` has decoded `+0x0A`
  all along, and `MovePowerCatalog` is installed on `World::move_power` at
  scene entry (`crates/engine-core/src/scene/host/scene_entry.rs:115`) - a
  **shared** site, so all three hosts get it.
- `StatusKind::from_enemy_effect` already routed bytes `3/4/5/6` to
  Venom/Toxic/Rot/Curse (it was derived from these very appliers), and
  `StatusEffectTracker` already ticks, blocks, gates Magic and feeds
  `BattleHud::sync_status`.

**The missing link was the applier itself.** Nothing read `impact_effect()` for
gameplay: `MovePowerCatalog` was consulted for `+0x00` power
(`World::enemy_move_power`) and for presentation (`fx_for_move_id`), never for
the status proc. `crates/engine-core/src/world/battle/{monster_ai,loop_driver}.rs`
had zero `enemy_effect` references, exactly as the brief said.

The reason it read as "no source exists" is a **mislabel**: both
`crates/engine-vm/src/scus_battle_helpers.rs` ("the port has no monster-side
`enemy_effect` source at all") and `docs/subsystems/battle.md` ("the cast leg
reads the **spell descriptor** byte `+0x0A` off `ctx[+0x1014]`") described
`ctx[+0x1014]` as something other than the move-power record. Once it is named
correctly the source is one already-parsed byte away.

---

## 3. What I changed

All of it inside `engine-core::World`, i.e. **shared by all three hosts** - no
host-side edit was needed, and none was made.

| File | Change |
|---|---|
| `crates/engine-core/src/world/battle/monster_ai.rs` | New pure kernel `enemy_impact_status_proc` (`PORT: FUN_801E09F8`, the `+0x0A` ladder) + `World::apply_enemy_move_status`, called from `take_monster_turn`'s `Cast` arm after `cast_spell_on_slots` folds. `#[cfg(test)] mod impact_proc_tests`. |
| `crates/engine-core/src/world/tests/battle_special_ai.rs` | Four world-level wire tests + two fixtures. |
| `crates/engine-vm/src/scus_battle_helpers.rs` | Corrected the stale "no monster-side source" disclosure; replaced it with the narrower, still-true Stone limit. |
| `docs/subsystems/battle.md` | Named `ctx[+0x1014]` correctly; split the two ladders; added the byte-6 asymmetry; engine pointer. |
| `docs/subsystems/battle-formulas.md` | Same ladder split; corrected "the petrify applier is not in the dumped corpus" (it is - see §5). |

**Which targets.** Retail rolls once per strike arm that reaches the impact
phase. The port's cast folds one `SpellOutcome` per target and pushes exactly
one damage-coloured `BattleHitFx` per damaging one, so the popups appended
since a pre-cast index are this cast's impact list. Magnitude is deliberately
not consulted - retail's arm is upstream of the HP subtraction, so a fully
mitigated or Stone-absorbed hit still reaches impact. Heals/buffs are excluded
by `is_heal` / by producing no popup at all.

**Basic attacks need no guard.** The id -> index map is special-attack-only
(pinned in `docs/formats/move-power.md`), so a monster's basic attack resolves
to the all-zero record 0, whose `+0x0A` is `0`, and the applier's own
zero-selector early-out drops it.

---

## 4. Non-vacuity: the disable-and-fail

Replacing the call site in `take_monster_turn`

```rust
self.apply_enemy_move_status(slot, def.id, hit_fx_start);
```

with `let _ = hit_fx_start;` and running `cargo test -p legaia-engine-core --lib enemy_special`:

```text
test enemy_special_impact_selector_5_rots_the_party_target ... FAILED
  panicked: the enemy special's +0x0A selector 5 rotted the party member
test enemy_special_rot_is_blocked_by_rot_guard_and_master_guard ... FAILED
  panicked: an unrelated passive leaves the Rot arm alone
test enemy_special_dot_arms_are_a_one_in_eight_roll ... ok
```

Two fail, one still passes - and the one that passes is the one that drives
`apply_enemy_move_status` **directly** rather than through `take_monster_turn`,
which is exactly the split that proves the two failures are about the *call
site* and not about the kernel. Restored, all eight tests pass.

Non-vacuity inside the tests themselves:

- `enemy_special_impact_selector_5_rots_the_party_target` runs the **same
  battle twice**, once with `+0x0A = 0` and once with `= 5`, and asserts the
  first applies nothing while the second applies Rot. So it cannot pass by
  "the tracker is always populated".
- `enemy_special_dot_arms_are_a_one_in_eight_roll` asserts `0 < landed < 64`
  over 64 seeds, so it fails both if the arm never fires and if it fires
  unconditionally.
- `enemy_applied_toxic_ticks_at_the_round_boundary` carries the status through
  to a **gameplay consequence** (HP drops at `BattleRound::end`), not just a
  tracker row.
- `rot_arm_rolls_a_limb_and_guards_draw_nothing` counts RNG draws, so a guard
  arm that "works" by silently consuming the stream would fail.
- `ladder_covers_three_four_five_only` drives an all-zero RNG, i.e. every
  probability gate passes - a `None` there is the ladder's shape, not a failed
  roll.

Suites: `cargo test -p legaia-engine-core` green (2647 lib + all integration,
with `LEGAIA_DISC_BIN` set so disc-gated tests actually ran);
`cargo test -p legaia-engine-vm` green. `cargo fmt --all -- --check` clean.

No existing test was deleted or weakened. Nothing was found asserting the
defect.

---

## 5. What is still open (and why I did not close it)

### 5a. Party -> monster status is *also* dead, one layer higher (OUT OF SCOPE)

`crates/art/src/parse.rs:142` builds every `ArtRecord` with
`enemy_effect: EnemyEffect::None` **unconditionally** - the art-record parser
never decodes the `+0x7A` byte `FUN_801EC3E4` reads. Nothing else in
`crates/art` decodes it either (`arts_table::RawArtRecord` is the 20-byte SCUS
name/AP table, a different structure).

Consequence: the party -> monster chain that *does* exist end to end
(`stage_art_profile` -> `ArtStrikeInfo.enemy_effect` -> `fold_battle_event` ->
`apply_from_enemy_effect`) can only ever carry `None` from disc data. The one
non-`None` producer in the tree is a **demo** record hard-coded in
`crates/engine-shell/.../window/run.rs:590` (`enemy_effect: EnemyEffect::Toxic`).
Compounding it, `World::set_art_records` has no production caller at all - the
`run.rs` demo is the only writer of `World::art_records`.

So the brief's framing ("status effects flow party -> monster only") is itself
too generous: before this lane, status effects flowed **nowhere** from disc
data. Fix belongs in `crates/art/` (decode `+0x7A` in the art-record parser)
plus a disc-side loader for `World::art_records`; both outside Lane D's scope.

### 5b. Stone / petrify has a *different* applier, and it is unported

`+0x16E |= 4` is not in either ladder. Its applier is in **`FUN_800402F4`**, the
SCUS item/effect applier:

```text
80041cd4  div  v0,v1              ; rand % (attacker[+0x168] + target[+0x168])
80041ce0  slt  a0,a0,v1
80041ce4  beq  a0,zero,0x80041d54  ; roll failed -> skip
80041cec  lhu  v0,0x16e(a1)
80041cf4  ori  v0,v0,0x4           ; <- Stone
80041cf8  sh   v0,0x16e(a1)
80041d4c  sb   zero,0x1de(v0)      ; clears the target's queued action category
```

Two committed claims collide here and both are now provably about **one** arm:
`docs/subsystems/battle-formulas.md` said "the petrify applier is not in the
dumped corpus" (it is - `ghidra/scripts/funcs/800402f4.txt`), while
`crates/engine-core/src/world/battle/loop_driver.rs` describes the same PC as
"selector 9 = the action-interrupt roll ... a stun, not a miss". The
capture-pinned Glare evidence (`+0x16E: 0 -> 4` *with* `+0x1DE` cleared) matches
this arm exactly, so "petrify applier" and "action-interrupt roll" are the same
instruction sequence under two names. I corrected the flatly-false half in
`battle-formulas.md` and left the naming reconciliation alone - it spans
`loop_driver.rs`'s melee doc and `status_effects.rs`'s bit table and deserves
its own pass.

**Consequence for the CLUT thread**: `BattleHud::sync_status` arms
`status_clut` on `StatusKind::Stone` **only**, so the ported
`FUN_8004CE2C` petrify-grey pass (`scus_battle_helpers.rs:268`) still cannot
fire in play. My wire moves Venom/Toxic/Rot onto party slots, which is the
applier gap named in the brief, but rows `481..=483` need §5b, not §1.

### 5c. A monster's *basic* attack: `param_2` for `FUN_801EC3E4` is unpinned

`FUN_801EC3E4` is the melee kernel for both sides, and its status byte is
`param_2[+0x7A]`. For a party art `param_2` is the art record. What retail
passes for a **monster's plain physical** I did not pin - it needs the caller
sweep for `jal 0x801ec3e4`. Until that is done, whether a monster basic attack
can inflict anything is unknown; the port applies nothing there, which matches
the "record 0 has no status" shape but is not *proof*.

### 5d. A one-line doc pointer I did not make (scope)

`docs/formats/move-power.md` describes `+0x0a` as the "impact-effect selector"
whose "values 3/4/5 branch to extra status-proc rolls" - correct, but it does
not say *which* statuses or link the applier. A cross-link to
`battle-formulas.md`'s byte table would help the next reader. `docs/formats/`
was not in Lane D's scope.
