# Lane 5 handoff - minigame-band worklist rows

Six worklist rows ported. Everything below is a cross-scope item this lane could
not action.

## Cross-scope: `crates/asset` (whoever owns it)

**`legaia_asset::dance_art::DanceWidget` is missing the record's `+0x13` byte.**
The dance HUD emitter (`FUN_801d2f38`) folds it into the texpage attribute as
`tpage + abr * 0x20` - `1` is the additive `B + F` blend the glow glyphs use -
and it is read from `table[+0x13]` whenever the widget id's mode field is `0`.
`parse_widgets` decodes every other field of the 20-byte record but stops at
`+0x12`. The Baka sibling (`baka_opponents::BakaHudWidget`) does carry it, as
`abr`.

Stopgap in place: `engine_core::dance::dance_widgets_with_abr` lifts the byte off
the same committed offsets (`WIDGET_TABLE_VA`, `DANCE_OVERLAY_BASE_VA`,
`WIDGET_STRIDE`) that `parse_widgets` uses, and `dance_hud_widget_quad` takes it
as a parameter. When `DanceWidget` grows an `abr: u8` field, delete
`dance_widgets_with_abr` and drop the parameter.

**`legaia_asset::baka_opponents::parse_actions` decodes two of the eight columns
the developer dump prints.** `BakaActionSet` carries `power` (`+0x18`) and
`keyframes` (`+0x1C`); `FUN_801D553C` prints the record's first eight words
(`+0x00`..`+0x1C`) plus the four halfwords of every sub-keyframe
(`+0x20`/`+0x22`/`+0x24` TRS, `+0x26` frame index, `0x08` stride). The port
(`engine_core::baka_cabinet::action_table_dump`) emits the two columns it has and
says so. The same missing `+0x26` column is already the stated blocker on
`baka_fighter::keyframe_in_range`, so widening the parser closes two rows at
once.

## Cross-scope: the site (`site/` + `crates/web-viewer`)

Two findings the minigames page states as *readings* are now pinned to the
disassembly and could be restated as traced:

- a mid-run Baka Fighter loss forfeits the whole accumulated pot - cabinet state
  `0x97` zeroes `_DAT_80084440` on its first frame;
- the final rung pays out automatically - the all-clear chain
  (`0x67` -> `0xFA`..`0xFE` -> `0x1F4`) has no other exit.

New content the page could carry: the **score-gated secret opponents**. Roster
ids `3` / `4` are offered mid-run at stages 5 and 13 once the running high score
passes 250,000 / 700,000, inserted into the ladder rather than replacing a rung
(`engine_core::baka_cabinet::{secret_opponent_gate, rung_fold}`).
`baka_fighter::LadderRun` does not model that gate - it walks the flat
`baka_ladder()` order - so wiring it would need `LadderRun` to consult the
cabinet.

## Cross-scope: `crates/engine-core/src/world.rs` (lane 4)

`BakaFight` now owns a `BakaCabinet` and steps it every tick, so the cabinet is
reached from the existing `SceneMode::BakaFighter` host root with no world
change. But two cabinet inputs are stubbed at that seam because the world does
not surface them:

- `CabinetInput::pad_edge` is left at `0`, so the in-duel pause menu (`0x110`)
  and the developer menu (`_DAT_8007B868`) are never entered. The tally's
  face-button flag is deliberately **not** substituted: `0xF0` and `0x110`
  overlap on Triangle, so feeding it across would open the pause menu whenever
  the player fast-forwards the tally;
- `CabinetInput::rung_prize` is `0`, so the cabinet's own pot stays empty - the
  world's mode-24 winnings accumulator is still the one that banks the prize.

If the world starts handing the raw pad edge and the current rung's prize to
`BakaFight::tick_with_input`, the cabinet's pot / forfeit / payout arms become
the live path and `LadderRun` could be driven from them instead of separately.

## Cross-scope: `crates/engine-core/README.md`

New top-level module `baka_cabinet` (the `FUN_801CF388` cabinet shell + its HUD
renderer + the developer action dump) is not in the README's module map - that
file is outside this lane's scope. It belongs next to `baka_fighter` /
`baka_fighter_chrome`.

## Left open

- `FUN_801D7BB8` is ported but disclosed `NOT WIRED`
  (`engine_core::minigame_floor::polar_offset`): it indexes two quadrature
  tables through **runtime** pointers (`_DAT_8007B81C` / `_DAT_8007B7F8`) and
  nothing in the engine decodes either table, so no caller can supply the pair.
  Locating those tables is the prerequisite.
