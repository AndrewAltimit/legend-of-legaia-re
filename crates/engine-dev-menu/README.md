# legaia-engine-dev-menu

The field overlay's developer-menu bodies, minus `World`: the 24-row dev-menu
model and its list picker, the value-step kernels, the EVENT FLAG editor, the
per-slot equip applier and the row-action dispatcher. Ported from the PROT
0897 routines' disassembly, with no bytes from the original executable. Free
of wgpu, winit and cpal, so it builds for native and `wasm32` alike.

## The split line

These are field-overlay routines, but two crates run them:

- `legaia-engine-field`'s op-`0x49` submode hub (`submode_hub_actors`) and
  panel code size panels, resolve equip slots and draw the equip stat panel
  through `dev_menu_leaves`;
- `legaia-engine-menus`' dev menu screen (`dev_menu_host`) steps every module
  here, and its pause-menu equip session commits through
  `dev_equip_commit::commit_equip`, the same applier body.

So the crate sits below both, and neither names the other for these. Its
only engine dependency is [`legaia-engine-system`](../engine-system/README.md)
(the packed pad layout the flag editor reads); `legaia-overlay-images`
supplies the field overlay's load base. Consumers name the modules here
directly (`legaia_engine_dev_menu::dev_menu_leaves`). Doc comments that point
up at a dependent crate are plain code spans, since rustdoc cannot resolve a
link into one.

## Modules

| Module | Covers |
|---|---|
| `dev_menu_leaves` | The dev-menu row model (`DevMenuRow`), the list picker's panel geometry, swap-wrap cursor and phase gate, camera readout decode, the equip-slot resolver and equip stat panel, and the records screen. |
| `dev_menu_adjust` | The row value steps (`wrap12_step`, `clamp1_255_step`). |
| `dev_menu_flags` | The EVENT FLAG editor: index / value steps and the flag list tags. |
| `dev_equip_commit` | The per-slot equip applier `FUN_801E5A08` (`commit_equip` over an `EquipCommitHost`). |
| `dev_menu_action` | The row-action dispatcher `FUN_801EA9B0`: its bound, park phase and unconditional result. |

## See also

- [`docs/subsystems/world-map.md`](../../docs/subsystems/world-map.md) - the
  dev menu and panel window system in retail.
- [`docs/subsystems/field-menu.md`](../../docs/subsystems/field-menu.md) - the
  equip applier and the EVENT FLAG editor.
- [`docs/subsystems/engine.md`](../../docs/subsystems/engine.md#the-layering-gate)
  - where the crate sits in the layering table.
