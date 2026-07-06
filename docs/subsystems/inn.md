# Inn Subsystem

Covers the HP / MP restore flow used at in-game inns. Retail has **no inn
overlay and no inn cost table**: each inn is an ordinary field-VM dialogue in
its scene's MAN, and the price is a script literal (see *Retail cost source*
below). The clean-room port (`engine-core::inn`) supplies the session state
machine; `open_inn(cost)` takes the scene's scripted cost.

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

## Flow overview

The engine port models the prompt as a menu session (retail runs it as plain
field-VM dialogue - see above). The port handles:

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
committing. The commit path:
1. Deducts `InnSession::cost` from `World::money`.
2. For every active party member in `World::roster`: sets `battle.hp_cur = battle.hp_max` and `battle.mp_cur = battle.mp_max`.
3. Calls `save_party()` to sync the roster records.

## Key data structure

### `InnSession` (`engine-core::inn`)

| Field | Type | Meaning |
|---|---|---|
| `cost` | `u32` | Gold required for one stay |

Key method:
- `can_afford(world_money: i32) -> bool` - `world_money >= cost`

Installed on `MenuRuntime` by `open_inn(cost)` before menu entry.

## Open items

- **Per-scene costs - RESOLVED.** The old "menu overlay DATA segment" reading
  is falsified: no cost table exists anywhere. Each cost is a field-VM script
  literal in the scene MAN (gate `0x4E` sub-3 + debit `0x3A`), parsed by
  `legaia_asset::inn_costs` and swept disc-wide by
  `crates/asset/tests/inn_costs_disc.rs` (see *Retail cost source* above).
  Remaining wiring: feed the scanned per-scene cost into `open_inn(cost)` at
  scene entry instead of a host-supplied constant.
- **Render layout.** Retail renders the prompt as ordinary field dialogue
  (MES text + option picker), not a dedicated cost window; the port's
  `InnConfirm` panel is an engine-side presentation choice. The scripted
  restore (`0x4C` records) and the `DREAM@@` hand-off are not yet mirrored.
- **Party filtering.** The retail engine may only restore party members who are
  currently in the active roster (not the reserve bench). The current port
  iterates all members of `World::roster.members` without a slot-active gate.

## Relationship to `legaia_save`

Gold is stored at `_DAT_8008459C` in retail RAM and in `World::money` in the
engine. Per-character HP/MP maxima are at `+0xFE / +0x102` (hp_max) and
`+0x104 / +0x106` (mp_max) within the 0x414-byte character record
(see `docs/reference/memory-map.md`).

## See also

**Reference** -
[Shop UI](shop.md) ·
[Save screen](save-screen.md) ·
[Level-up](level-up.md)
