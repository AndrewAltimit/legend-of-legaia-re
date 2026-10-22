# legaia-engine-save-state

LGSF saves over a running `World`. The `WorldSaveState` extension trait puts
the full save-file round trip on `World`:

- `save_full` / `load_full` - the party records plus every global a save
  carries (story flags and the system-flag bank, gold, the bag in both its
  slot-array and compact forms, the minigame purses, the present-party
  composition, learned arts, Seru capture progress and shiny spells, play
  time, the field position, the audio levels and the option words), over the
  LGSF container in [`legaia-save`](../save/README.md);
- `load_story_flags` - the flag half alone, which a card load hydrates before
  the landing scene's field init;
- `retail_options` / `set_retail_options` / `take_loaded_options` - the
  options screen's nine words as the session plays them, and the words the
  last load restored.

It sits directly above [`engine-core`](../engine-core/README.md), beside
[`engine-cheats`](../engine-cheats/README.md): nothing in the simulation saves
a file, so a save-format change does not rebuild `World`. The per-record half
a save is built from (`World::save_party` / `World::load_party`) stays in
`engine-core`, because the tick and New Game seeding run it.

The callers bring the trait into scope (`use
legaia_engine_save_state::WorldSaveState;`): the scene host's resume, the menu
runtime's save slots and card writer, the session's scene entry and frame
steps, the native window, the browser runtime's card and session saves, and
the parity runners.

## Tests

`tests/` holds the LGSF round-trip tests: globals, minigame purses, the bag's
slot array, the present party, capture progress and shiny spells across a
save, the level-up tracker's hydration, retail card-block imports and the
block checksum, and the end-to-end gameplay loop. The memory-card variants key
on a card image under `~/.mednafen/sav/`; the real-encounter variant on an
`extracted/PROT.DAT`. Both skip when the data is absent.

See [`docs/subsystems/engine.md`](../../docs/subsystems/engine.md#the-lgsf-save-format)
for the LGSF format.
