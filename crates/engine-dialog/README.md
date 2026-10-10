# legaia-engine-dialog

The field dialog layer's `World`-free kernels, split out of
`legaia-engine-menus`: the dialog pager and the state of the script contexts
that drive it. `engine-menus` depends on this crate, never the other way
round, and re-exports every module at its old path
(`legaia_engine_menus::dialog`, and through `engine-core`,
`legaia_engine_core::dialog`), so hosts and tests name the same paths they
always did.

## What lives here

| Module | Covers |
|---|---|
| `dialog` | The pager panel `OwnedDialogPanel` over a scene MES message or an inline segment, and `SceneMes`, the scene's resolved MES container. See [`mes.md`](../../docs/formats/mes.md). |
| `dialog_window` | The pager's row window and its scroll. |
| `dialog_pacing` | The typewriter reveal clock. |
| `dialog_picker_slide` | How a picker box enters. |
| `text_balloon` | The `4C E1` one-line balloon. |
| `inline_dialogue` | The resumable state of an actor's inline interaction script run through the field VM. |
| `cutscene_timeline` | The spawned partition-2 record context: the modal cutscene timeline. See [`cutscene.md`](../../docs/subsystems/cutscene.md). |

## The split line

These modules name each other, the MES interpreter (`legaia-mes`), the dialog
font's layout (`legaia-font`), the field VM's context and motion state
(`legaia-engine-vm`) and one narration enum from `legaia-asset` - and nothing
in the menu front end. No menu module names them either: the dialog pager and
the pause / shop / save screens share no state, which is what makes this a
leaf.

What stays above: the *stepping* of the inline-dialogue and cutscene-timeline
contexts lives on `engine-core`'s `World`, which holds the field host borrow.
These modules hold the state those steps advance. Drawing is `engine-ui`'s.

## See also

- [`crates/engine-menus`](../engine-menus/README.md) - the menu, title and
  memory-card front end this was split from.
- [`docs/subsystems/script-vm.md`](../../docs/subsystems/script-vm.md) - the
  field VM whose ops open these panels.
