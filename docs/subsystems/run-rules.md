# Run rules: game modes and twists for a whole run

Run rules are the port's run-wide twists: a difficulty preset, New Game+, ironman, an in-engine randomizer and a Boss Rush mode. A player picks them on the New Game rules screen between the title's NEW GAME and the opening, on both play hosts, and a run's saves carry them. None of them exists in the original game. The default rules are the retail game, and every engine reader folds a rule in as an identity at that default, so a run with no rules plays bit-identically to one before run rules existed and writes byte-identical saves.

| Piece | Where |
|---|---|
| Rule model, presets, New Game+ curve, Boss Rush run, rules screen session, save body | `crates/engine-run-rules` (`legaia_engine_run_rules`), `World`-free |
| Randomizer planners and safety lists, shared with the disc patcher | `crates/randomizer` (`legaia_randomizer`) |
| The `World` side: knob readers, New Game+ carry, randomizer overrides, ironman, Boss Rush stages | `engine-core` `world::run_rules`, `world::run_rules_boss_rush` |
| The hosts' shared half: rules screen, run start, Boss Rush driver | `engine-session` `run_rules` |
| Card rules for ironman (one save per run, erase, autosave) | `engine-menu-runtime` `card_write` |
| Screens | `engine-ui` `run_rules_screen`, projected by `engine-screens` `run_rules` |
| Save block | `LGXF` in an LGSF file, the engine-ext tail's fourth segment in a card block |
| Replay row | `set = "run_rules"` in j-replay-v1 |

## The rules

`RunRules` holds a mode (`story` or `boss_rush`), a `Difficulty`, and three optional rules: `ng_plus`, `ironman`, `randomizer`. `RunRules::default()` is the retail game; `RunRules::is_retail` answers whether nothing is to be carried or saved.

The rules sit on `World::run_rules`. A host stages them before New Game (`World::stage_run`); `World::begin_new_game` takes the staged run (retail when nothing is staged), and `begin_new_game_seeded` folds in the New Game+ carry and the randomizer's starting gear after the starting party and bag are seeded. A save load restores the rules from the save's block, and a save without one plays retail rules.

## Difficulty presets

A preset is a bundle of scales and gates (`Knobs`). Each scale multiplies the matching session tweak ([`gameplay-tweaks.md`](gameplay-tweaks.md)) at the reader the tweak already uses, so a preset and a tweak stack; at `1000` the reader sees exactly the tweak's value.

| Knob | Easy | Normal | Hard | Nightmare | Reader |
|---|---|---|---|---|---|
| Enemy stats | `0.75x` | `1x` | `1.35x` | `1.8x` | `seed_monster_seat_stats` and the escape and side-effect stat reads, through `scale_monster_def` |
| Enemy attack count | `0.75x` | `1x` | `1.25x` | `1.5x` | `arm_monster_strike_budget` |
| EXP | `1.5x` | `1x` | `1x` | `0.9x` | `apply_battle_loot`, the run-away EXP sum |
| Gold | `1.5x` | `1x` | `1x` | `0.8x` | `apply_battle_loot`, after the pack's multiplier |
| Encounter rate | `0.6x` | `1x` | `1.25x` | `1.5x` | `RegionEncounterTracker::on_step`'s increment |
| Shop prices | `0.8x` | `1x` | `1.25x` | `1.5x` | `try_arm_field_shop`'s price lookup |
| Healing items | `1.5x` | `1x` | `0.75x` | `0.5x` | `use_item`'s HP / MP outcome, held to what the target is missing |
| AP gain | `1.25x` | `1x` | `1x` | `0.75x` | `spirit_turn_ap`, `accrue_spirit_gauge` (a draining setting passes through) |
| Smarter monsters | no | no | yes | yes | the monster target pick, as `--smart-monsters` |
| No escape | no | no | no | yes | `roll_battle_escape`, as the formation's own no-escape byte |

The AI knob is the existing smarter-targeting rule (a single-target attack picks the weakest living member); the monster AI port has no other clean aggression knob. Tetsu, the unwinnable sparring partner, keeps his stats at every preset, as the enemy-strength tweak pins him.

## New Game+

A New Game+ starts a new run from any save on either card port. The rules screen lists every readable save; picking one turns on New Game+ at that save's own cycle plus one, with a carry set (levels, equipment, items, Seru magic, arts, gold, cards), each part switchable.

The carry is lifted by loading the source save into the world (`load_full`, so it carries exactly what a Continue of that save would) and snapshotting the parts (`World::ng_plus_snapshot`); the New Game then resets the world and folds the chosen parts back in (`apply_ng_plus`):

- each character record takes the carried spans over the fresh record (`carry_record`): levels are EXP, grown stats and HP / MP (`+0x000..+0x0F4`, `+0x104..+0x13C`), Seru magic `+0x13C..+0x185`, arts `+0x185..+0x196`, equipment `+0x196..+0x19E`; the ability bits are rebuilt from the gear, the name and the rest stay the fresh record's;
- members the story has not introduced yet (Noa, Gala, Terra) are written into the roster without joining the party, so the story's join keeps the carried record (the join only seeds an empty slot);
- the bag carries without its quest and key items (the priced-zero items), so no story gate opens early;
- the per-character Seru capture log, learned arts and saved chains carry with their parts;
- an active pack's store carries too: its cards, its items and its worn gear, and its learned arts and spells (with the arts and Seru parts, through `World::expansion_carry_learned`).

Enemies scale on a curve over the cycle and the party's strongest level (`scaling::ng_plus_permille`): `1000 + 250 * cycle + 12 * (level - 1)` permille for an ordinary monster, `20` per level for a story boss, capped at `5x`, so a set piece stays a fight at a cleared game's levels. The curve multiplies the preset's enemy scale.

A pack can gate content on the run: while a pack is active the world raises the pack flags `run.ng_plus`, `run.hard` (Hard or Nightmare), `run.ironman` and `run.randomizer`, and the counter `run.ng_plus_cycle`, so a trigger or quest gates with `requires = { flags = ["run.ng_plus"] }`. Under retail rules nothing is raised.

## Ironman

An ironman run draws a run id at New Game, which its saves carry. The card writer every host shares (`write_save_into_card` and its siblings) keeps one save per run: after writing a save of an ironman run it erases every other save of that run from the card. When the party falls (the wipe that returns to the title) the world marks the run, and the host erases the run's save from port 1 (`ironman_erase_run`). Each scene entry owes an autosave, which the host writes over the run's own save or into the lowest free save number (`ironman_autosave`). Hardcore adds one rule: the battle Item command is refused, as a pack challenge's `no_items` rule refuses it.

An erase marks the save's directory frame deleted (`0xA1`, `MountedCard::erase_save_block`), as the BIOS's own delete does.

## The randomizer

The in-engine randomizer reassigns game data from a seed string with no disc patch. The seed string is resolved to a number by the patcher's own rule (`legaia_randomizer::rng::seed_from_str`). Each part is applied at its reader, keyed by the run seed and the site, never by the order the player meets the sites and never through the game's `rand()` stream:

| Part | Reader | Rule |
|---|---|---|
| Encounters | the region and mean-rate roll sites, after a pack's substitution | the rolled formation's seats redrawn from the scene's own random-encounter monsters (`encounters::reroll_formation`), at most two species; the new formation is registered under `0xD000 | row`, below the pack formations |
| Monster stats | the battle-entry catalog swap (`run_rules_prepare_battle`) | each stat moved inside `0.8x..=1.25x` per monster (`bands::band_stats`) |
| Drops | the same swap | each dropping monster's item drawn from the sellable pool, its chance kept |
| Steals | both steal rolls (`Steal` and the spell steal) | the same, per monster |
| Shops | `try_arm_field_shop` | each slot drawn from the sellable pool per scene, shop and slot, no repeats within a shop |
| Chests and gifts | the field VM's give-item host | each item drawn from the sellable pool per scene and item |
| Starting gear | the New Game | Vahn's weapon and body armour drawn from the gear he can wear |

The safety rules are the patcher's, read from the shared crate: the story bosses and the tutorial enemies (`monsters::PROTECTED_MONSTER_IDS`) keep their stats and never enter a random formation, the Gimard fight (`encounters::PROTECTED_FORMATION_IDS`) is never redrawn, scripted fights never pass the roll sites, a formation never exceeds `MAX_DISTINCT_SPECIES`, and a quest or key item (`items::ItemPools::keep`) never moves and never enters a pool - which is what keeps a randomized run completable. A pack's own fights keep the pack's monsters.

A chest's dialogue still names the item the script names; the bag receives the randomized one. Arts commands are not randomized in the engine: the arts input is read both from the battle's art-animation bank and from the menu's arts table, and a remap has to move both (the patcher's `--arts` writes the disc records both are read from). The shuffle modes of the patcher need the whole disc at once and are not offered: each part is the patcher's `random` mode with order-independent site keys (`sites::site_rng`).

## Boss Rush

Boss Rush fights the story bosses back to back from a New Game. Each stage is a scripted battle row of a retail scene (`boss_rush::RETAIL_STAGES`): Gimard, Caruban, Songi, Zeto, Xain, Dohati, Koru and Cort. The run seeds Vahn, Noa and Gala in their starter gear; before each stage the host enters the stage's scene as an opening leg, the party is raised to the stage's level through the ordinary EXP path and rested, and the fight is armed the way `play-window --battle` arms a row (its record's flag arm and music replayed, then the encounter transition). An active pack's bosses follow the retail stages.

A won stage pays `STAGE_PURSE` gold and opens the intermission: next battle, rest, buy supplies (resolved by name against the disc's item table), or give up. A lost stage ends the run. The clock counts the frames a stage is entered and fought, not the intermission. The leaderboard (`BossRushRecord`: runs, clears, the most stages, the fastest clear) lives on the world and rides every save the session makes.

## The rules screen

The title's NEW GAME opens the rules screen (`engine-session` `open_run_rules_screen`) on both hosts, except in retail-faithful mode, where NEW GAME is retail's straight start. It opens on the retail rules with the cursor on *Start*, so one confirm is the original New Game. Up / Down pick a row, Left / Right change it, Cross confirms, Circle returns to the title. The seed row rolls a readable seed; the page's `play_run_rules_set_seed` takes a typed one.

The screen sits on the world (`RunRulesState::setup`), so the shared screen layers draw it (`engine-screens` `run_rules::active_screen`): the native window through its screen-prim pass and text layer, the browser play page on the title's menu canvas (`play_run_rules_draws_json`) and in game through the same pass. The pause menu shows the run's summary line (`Hard / NG+1 / Ironman / Seed RIM-0042`) at the foot of the root panel; under retail rules it shows nothing.

## Saves and replays

The run's block is versioned JSON (`save::RunSave`): the rules, a Boss Rush in progress, the leaderboard and the ironman run id. An LGSF file carries it in the optional `LGXF` block after `LGXD`; a card block carries it as the engine-ext tail's fourth `u16`-length segment, after the expansion one, reserved first so the ironman id outranks a quest log. A run with retail rules, no Boss Rush and an empty leaderboard writes no block.

A j-replay-v1 file records the rules as a value row, `set = "run_rules"` with the rules as a `rules` table, noted whenever the world's rules change ([`determinism-replay.md`](../tooling/determinism-replay.md)). The row sets the rules; a New Game+ carry needs its source save, which a replay does not hold.

## Tests

| Test | Crate |
|---|---|
| `retail_rules_are_the_identity`, `presets_order_and_step`, `rules_round_trip_and_summarise` (`rules`); `the_curve_grows_with_cycle_and_level_and_favours_bosses` (`scaling`); `a_run_walks_its_stages_and_records`, `the_intermission_cursor_wraps` (`boss_rush`); `retail_writes_nothing_and_rules_round_trip` (`save`); `one_confirm_starts_the_retail_game`, `rows_set_every_rule` (`setup`) | `engine-run-rules` |
| `retail_rules_leave_every_reader_alone`, `difficulty_presets_apply_and_clear`, `staged_rules_take_over_at_new_game_and_round_trip_the_save_block`, `ironman_marks_the_run_on_a_wipe_and_owes_autosaves`, `ng_plus_carries_the_chosen_parts`, `the_randomizer_is_a_function_of_its_seed` (`world::tests::run_rules`); `carry_copies_only_the_chosen_spans` (`world::run_rules`) | `engine-core` |
| `picks_are_stable_per_key_and_spread_across_keys`, `kept_items_and_the_empty_sentinel_never_move`, `bands_hold_and_protected_monsters_keep_their_stats`, `rerolls_keep_the_seat_count_the_species_cap_and_the_guard`, `priced_items_pool_and_named_free_ones_are_kept` | `randomizer` |
| `the_panel_lists_its_rows_and_scrolls_to_the_cursor` (`run_rules_screen`) | `engine-ui` |
| `the_setup_screen_lists_the_rows_and_the_run` (`run_rules`) | `engine-screens` |
| `the_rules_screen_draws_nothing_until_opened` (`play_run_rules`) | `web-play` |
| `erasing_a_save_frees_its_block` (`emu`) | `save` |
| `ironman_keeps_one_save_and_a_wipe_erases_it` (`card_write`) | `engine-menu-runtime` |
| `boss_rush_rows_seat_their_bosses`, `boss_rush_runs_its_first_stages` (disc-gated, `tests/boss_rush_disc.rs`); `switches_round_trip_in_every_shape` (`replay`) | `engine-shell` |
