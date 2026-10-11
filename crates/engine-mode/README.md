# legaia-engine-mode

The game-mode seat of the engine, as a `World`-free crate.

Retail drives the whole game off one master mode word (`_DAT_8007B83C`): 28
modes, each an `(init, frame)` handler pair in the mode table
(`legaia_game_tables::mode_table`). This crate holds the port of that seat and the
pieces that only make sense next to it.

| Module | Scope |
|---|---|
| `mode` | `GameMode` / `SceneMode`, the mode driver, the per-mode init and frame stages, and `ModeSeat` - the owner of the mode word, the boot hand-off chain and the front-end entry word. |
| `minigame_entry` | What entering a minigame mode sets up: the per-minigame overlay, scene and return bookkeeping. |
| `card_request` | `FUN_801D84B4`, the two-store leaf that hands the frame to the card / menu mode. |

## Layering

Depends on `legaia-asset` (the mode table), `legaia-bytes` and
`legaia-engine-system` (mode-entry initialisers, the boot entry word, music
labels, pad input). It must not depend on `legaia-engine-field`,
`legaia-engine-menus` or anything above them: the field camera reads
`mode::SceneMode`, so this crate sits below the field crate, and both play
hosts reach it through `legaia-engine-core`'s `mode` module.
