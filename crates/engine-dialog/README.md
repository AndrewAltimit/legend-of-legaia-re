# legaia-engine-dialog

The field dialog layer's `World`-free kernels: the dialog pager, its row window, pacing and picker
slide, and the text balloon. The state of the script contexts that drive the
pager (`inline_dialogue`, `cutscene_timeline`) is field-VM context and lives
in `legaia-engine-field`. `engine-menus` depends on this crate, never the other way
round. Hosts and tests name its modules directly
(`legaia_engine_dialog::dialog`).

## What lives here

| Module | Covers |
|---|---|
| `dialog` | The pager panel `OwnedDialogPanel` over a scene MES message or an inline segment, and `SceneMes`, the scene's resolved MES container. See [`mes.md`](../../docs/formats/mes.md). |
| `dialog_window` | The pager's row window and its scroll. |
| `dialog_pacing` | The typewriter reveal clock. |
| `dialog_picker_slide` | How a picker box enters. |
| `text_balloon` | The `4C E1` one-line balloon. |
| `cursor_sprite` | The animation half of the cursor sprite primitive `FUN_8002B994`: the frame and idle bob of each hand, pager triangle and page mark (`CursorSprites`). |

## The split line

These modules name each other, the MES interpreter (`legaia-mes`) and the
dialog font's layout (`legaia-font`) - no other engine crate, and nothing in
the menu front end. No menu module names them either: the dialog pager and
the pause / shop / save screens share no state, which is what makes this a
leaf.

What stays above: the *stepping* of the inline-dialogue and cutscene-timeline
contexts lives on `engine-core`'s `World`, which holds the field host borrow.
These modules hold the state those steps advance. Drawing is `engine-ui`'s.

## See also

- [`crates/engine-menus`](../engine-menus/README.md) - the menu, title and
  memory-card front end above this crate.
- [`docs/subsystems/script-vm.md`](../../docs/subsystems/script-vm.md) - the
  field VM whose ops open these panels.
