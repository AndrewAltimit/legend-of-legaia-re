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
| `sfx_residency` | What each mode's initialiser does to the SPU region VAB slots `2` and `6` share: the field-bank latch, which bank the region holds, each slot's enable, and the per-mode sync (`SfxBankResidency::sync`). See [`audio.md`](../../docs/subsystems/audio.md). |

## Layering

Depends on `legaia-bytes`, `legaia-game-tables` (the SFX bank PROT indices) and
`legaia-engine-system` (mode-entry initialisers, the boot entry word, music
labels, pad input). It must not depend on `legaia-engine-field`,
`legaia-engine-menus` or anything above them: the field camera reads
`mode::SceneMode`, so this crate sits below the field crate. Both play
hosts name it directly; `legaia-engine-core`'s `mode` module holds the
functions that read a `World`.
