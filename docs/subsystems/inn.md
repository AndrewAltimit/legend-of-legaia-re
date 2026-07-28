# Inn Subsystem

Covers the HP / MP restore flow used at in-game inns. Retail has **no inn
overlay, no inn opcode and no inn cost table**: each inn is an ordinary
field-VM interaction record in its scene's MAN, the price is a script literal
(see *Retail cost source* below), and the whole stay - offer, choice, gate,
debit, fade, restore - runs as one pass of that record through the field VM.
The port executes it the same way, so the reachable in-game inn is the retail
script, not an engine routine (see *The trigger* below).

`engine-core::inn` additionally carries an engine-side `InnSession` prompt
(`MenuRuntime::open_inn` / `open_scene_inn` over the scanned
`SceneHost::scene_inn_cost`). That is a presentation the retail path does not
use and no host reaches - see *Open items*.

## Retail cost source (field-VM script literals)

An inn stay is a scripted **gold-gate + debit pair** inline in the scene MAN
(asset type `0x03`), the same place the town gold-shop stock lives
(see [Shop](shop.md#gold-shop-stock-source)):

```text
0x4E <pp> 0x30 <cost u16> <skip u16>   ; if gold < cost, jump +skip
...                                    ;   (the "can't afford" reply)
0x3A <sext24(-cost)>                   ; ADD_MONEY: gold -= cost
```

Op `0x4E` **sub-op 3** (operand byte 1, high nibble) loads the party gold
`_DAT_8008459C` and compares it against the u16 literal at operand `+2` (low
nibble `0` = jump when gold < literal - the can't-afford branch); sub-op 10 is
the 32-bit sibling (literal lo16 at `+2` / hi16 at `+6`, 9 bytes) used where a
price can exceed 65535 (the casino gold-to-coin counter). Provenance: the
op-`0x4E` inner jump table at field-overlay VA `0x801CEE30` (12 entries) -
the sub-3 arm at `0x801E0AEC` loads `_DAT_8008459C`, sub-2 at `0x801E0AC0`
loads a per-character level byte (`+0x130`), sub-9 at `0x801E0B34` loads the
casino coin bank `_DAT_800845A4` (see
`ghidra/scripts/funcs/overlay_0897_801de840.txt`; the decompiled-C case labels
collapse these arms - the disassembly + jump-table words are ground truth).
Op `0x3A` (`ADD_MONEY`, `docs/subsystems/script-vm.md`) applies the signed
24-bit delta.

After the debit the same script continues in-line: the innkeeper's
thank-you text, per-party-slot `0x4C` records on slots 0/1/2 (the restore),
and a `0x3F` transition whose destination name is `DREAM@@` - the inn dream
sequences. So cost prompt, gate, debit, restore, and dream hand-off are all
one field-VM dialogue; no menu-overlay sub-screen is involved in retail.

## The trigger: the picker's own jump table

There is no "this dialogue is an inn" test anywhere - no opcode, no flag, no
carrier table. The hand-off from the innkeeper's offer to the restore is the
**option picker's per-option relative jump**, and the innkeeper's record is
laid out so that the Yes entry points at the gate.

The shape, read off `retock`'s innkeeper record (partition-1, the record
containing the scene's one scanned charge):

```text
1F <greeting> 00  1F <price line> 00  1F <offer question> 00
2A <rel16 yes> <rel16 no> 24        ; 2-option picker + post-page continue
1F <"yes" label> 00  1F <"no" label> 00
  ; yes-jump target:
A2 F8 <clip>  AC F8 08  AD F8 08    ; play a player clip, clear its end
                                    ;   latch, spin until it re-latches
... 26 <rel16>                      ; -> the gate
4E <pp> 30 <cost u16> <skip u16>    ; if gold < cost, skip to the refusal line
3A <sext24(-cost)>                  ; take the money
1F <thank-you> 00 1F <good night> 00
35 / 34 / 4A / 36                   ; BGM release, fade, waits, cue
4C 82 00  4C 82 01  4C 82 02        ; the restore, one op per party slot
```

The **no**-jump target runs its own clip beat and lands on the decline line;
the gate's skip target lands on the can't-afford line. Both are inside the
same record, so a stay never leaves the field VM.

Two details of that layout are easy to get wrong, and each one alone is enough
to make the inn unreachable in a port:

- **The open byte is `0x2A`, not `0x27`.** `FUN_80038050` - the inline-script
  control handler that applies the chosen option's jump - treats `0x27`,
  `0x28`, `0x29` and `0x2A` identically (one `switch` arm, `new_pc = (O + 1 +
  index*2) + i16_LE(entry[index])`). On the pager side `0x2A` selects state
  `0x11` where `0x27` selects `0x13`, and the shared cursor handler at
  `0x801D941C` reads the option count off the *active* state: `0x18` = 4,
  `0x16` = 3, anything else = 2. So `0x2A` is a **2-option** menu that
  animates the box geometry first, and its cursor does not wrap (the
  `state == 0x12` carve-out at `0x801D9474` clamps at both ends instead). See
  [`formats/mes.md`](../formats/mes.md#post-page-dispatch-state-0x19).
- **`AD F8 08` is a spin, not an end.** `0x2D` tests a bit of the clip-control
  word `actor+0x62`, and bit 8 (`0x0100`) is the "end" flag the actor tick
  `FUN_800204F8` latches when a clip cursor reaches an end - so the
  `A2 F8 <clip>` / `AC F8 08` / `AD F8 08` triple is "play it, clear the
  latch, wait for it". Retail's dialog SM `FUN_80039B7C` returns and re-enters
  per frame until the latch lands. A runner that treats the halt as the end of
  the conversation stops one instruction into the Yes branch, before the gate.

Engine port of the whole path: `World::trigger_field_interact` →
`World::drive_inline_dialogue` → `World::step_inline_dialogue`
([`crate::inline_dialogue`](../../crates/engine-core/src/inline_dialogue.rs)) →
`legaia_engine_vm::field::step`. The runner parks on the clip-end spin and
latches the tested bit once the cued player clip finishes (the port's stand-in
for the anim tick's write, since the record's context is not bound to the poked
actor); the gate reads the live purse through `FieldHost::party_bank_value`,
the debit is the record's own `ADD_MONEY`, and the restore is its own
`4C 82 <slot>` ops (`FieldHost::op4c_n8_sub2_restore_party_slot`). Disc-gated
oracle: `crates/engine-core/tests/inn_stay_field_vm_disc.rs`, which drives the
real record from the interact call and asserts the gold delta and the pools on
the Yes, No and can't-afford branches.

The shared scanner is [`legaia_asset::inn_costs`]: a byte scan (robust to the
dialogue-picker jump tables that desync a linear walk) for a gold compare
whose literal reappears as the magnitude of a negative `ADD_MONEY` within a
few ops of the gate (retail sites sit 7..~16 bytes apart). Swept disc-wide
by `crates/asset/tests/inn_costs_disc.rs`: the pair resolves in the inn /
paid-lodging scenes (e.g. the 200 G innkeeper sites in the `ropeway` and
`balden` blocks, `rayman2`'s 200 G stay, `retock`'s 240 G stay), the paid
tours and the 3,000 G `station3` train ticket (sub-3, u16 costs), and the
casino gold-to-coin counters (`koin*`; `koin4` carries the only sub-10 u32
sites, 8,500..90,000 G). Free rests (Rim Elm's bed, Biron) simply have no
gate + debit pair in their scripts.

## Flow overview (the engine-side `InnSession` prompt)

This section describes the port's **alternative** presentation - a menu
session with its own cost window - which nothing on the reachable path opens
(see *Open items*). The in-game stay is the field-VM record above.

| Phase | Sub-screen | Description |
|---|---|---|
| Cost prompt | `InnConfirm` | Shows the cost for one night and a Yes / No cursor. |
| Rest fade | `InnSleep` | Transient screen that plays the rest fade after a Yes. |
| Commit | - | Deducts gold, restores all active party members' HP/MP. |
| Exit | - | Returns to field without resting if No or gold insufficient. |

The menu state machine (`engine-vm::menu`) routes the prompt: `InnConfirm` Yes
(slot 0) commits the rest and routes to the transient `InnSleep` fade, which
auto-advances to the menu's `Closing` state after `transient_hold_frames`;
`InnConfirm` No (slot 1) and Triangle route straight to `Closing`. Either way the
inn session is cleared (`MenuRuntimeHost::commit` / `cancel`).

On confirmation the engine calls `InnSession::can_afford(world_money)` before
committing. The commit path (`commit_inn_confirm`):
1. Deducts `InnSession::cost` from `World::money`.
2. For each of the first `World::party_count` actor slots whose `active` flag
   is set: restores `battle.hp` to `battle.max_hp` and `battle.mp` to the
   roster record's `mp_max`. Inactive slots and the reserve bench are
   untouched.
3. Calls `save_party()` to sync the roster records.

## Key data structure

### `InnSession` (`engine-core::inn`)

| Field | Type | Meaning |
|---|---|---|
| `cost` | `u32` | Gold required for one stay |

Key method:
- `can_afford(world_money: i32) -> bool` - `world_money >= cost`

Installed on `MenuRuntime` by `open_scene_inn(&SceneHost)` (resolves the
loaded scene's scanned cost and enters `InnConfirm`; returns `None` and
installs nothing for free-rest scenes) or directly by `open_inn(cost)`.

## Open items

- **Per-scene costs - RESOLVED, wired.** The old "menu overlay DATA segment"
  reading is falsified: no cost table exists anywhere. Each cost is a field-VM
  script literal in the scene MAN (gate `0x4E` sub-3 + debit `0x3A`), parsed
  by `legaia_asset::inn_costs` and swept disc-wide by
  `crates/asset/tests/inn_costs_disc.rs` (see *Retail cost source* above).
  Production wiring: `SceneHost::load_scene` scans the cached MAN into
  `scene_gold_charges` (`scene_inn_cost()` = the first sub-3 charge), and
  `MenuRuntime::open_scene_inn(&SceneHost)` opens `InnConfirm` with that
  scanned cost - `open_inn(cost)` stays as the direct test / tooling entry.
  Disc-gated oracle: `crates/engine-core/tests/inn_cost_scene_disc.rs`
  (`retock`'s 240 G stay resolves; free-rest `town01` opens nothing).
- **Trigger - RESOLVED, wired.** The stay runs as the innkeeper's own field-VM
  record, reached by walking up and talking (*The trigger* above), and the
  charge and restore are the record's own ops. The port had the ops and the
  cost scan but not the path: the `0x2A` menu open byte decoded as nothing, the
  gold gate read an empty purse because `FieldHost::party_bank_value` had no
  engine implementation, and the clip-end spin ended the conversation one
  instruction into the Yes branch. All three are closed and pinned by
  `inn_stay_field_vm_disc`.
- **`InnSession` has no production caller - disclosed, not wired.**
  `MenuRuntime::open_inn` / `open_scene_inn` and the `InnConfirm` / `InnSleep`
  sub-screens are an engine-side prompt with its own cost window and its own
  commit kernel. Retail has no such screen and the reachable path does not pass
  through one, so wiring a host into it would *replace* faithful innkeeper
  dialogue with an invented panel rather than fill a gap. It stays as the
  direct entry for tests and tooling; the native window's `InnConfirm` /
  `InnSleep` draws are reachable only from there, and the browser play page
  deliberately mirrors that by not drawing them either. Deleting it is a
  legitimate future call; inventing a caller for it is not.
- **The `DREAM` hand-off is not mirrored.** Some inns append a story-flag-gated
  tail that warps to a `DREAM` scene after the restore. The restore runs first
  and unconditionally, so a stay is complete without it, but the dream scenes
  themselves are not yet reached.

## Relationship to `legaia_save`

Gold is stored at `_DAT_8008459C` in retail RAM and in `World::money` in the
engine. Per-character HP/MP live pools are the `(max, cur)` u16 pairs at
`+0x104 / +0x106` (HP) and `+0x108 / +0x10A` (MP) within the 0x414-byte
character record (see [`save-record.md`](../formats/save-record.md)).

## See also

**Reference** -
[Shop UI](shop.md) ·
[Save screen](save-screen.md) ·
[Level-up](level-up.md)
