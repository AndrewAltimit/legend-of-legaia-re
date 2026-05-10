# Inn Subsystem

Covers the HP / MP restore flow used at in-game inns. The inn UI lives inside
the **menu overlay** - the same 129-function binary as the shop, save screen, and
status screens. No separate inn overlay exists.

Per-scene inn costs are encoded in the menu overlay's DATA segment and are not
yet traced. The clean-room port (`engine-core::inn`) supplies the session state
machine; once the overlay is traced the cost tables can be wired in.

## Flow overview

The retail engine enters the inn from the field-VM shop-trigger opcode (same
entry point as the shop, dispatched differently by the sub-screen ID). The menu
overlay handles:

| Phase | Sub-screen | Description |
|---|---|---|
| Cost prompt | `InnConfirm` | Shows the cost for one night and a Yes / No cursor. |
| Commit | - | Deducts gold, restores all active party members' HP/MP. |
| Exit | - | Returns to field without resting if No or gold insufficient. |

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

- **Per-scene costs.** The retail cost for each inn is encoded in the menu
  overlay's DATA segment. Locating those values requires tracing the sub-screen
  that handles `InnConfirm` entry. Pending overlay binary capture
  (`overlay_shop_save`).
- **Render layout.** The cost prompt UI (cost amount + Yes/No cursor) mirrors
  the retail overlay; exact pixel offsets are pending the capture.
- **Party filtering.** The retail engine may only restore party members who are
  currently in the active roster (not the reserve bench). The current port
  iterates all members of `World::roster.members` without a slot-active gate.

## Relationship to `legaia_save`

Gold is stored at `_DAT_8008459C` in retail RAM and in `World::money` in the
engine. Per-character HP/MP maxima are at `+0xFE / +0x102` (hp_max) and
`+0x104 / +0x106` (mp_max) within the 0x414-byte character record
(see `docs/reference/memory-map.md`).
