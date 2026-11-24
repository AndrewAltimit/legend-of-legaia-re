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

The *Custom* preset is the player's own bundle: its ten knobs are rows of their own, each stepped by Left / Right (`Knobs::step`: `x0.05` up to `x2`, `x0.25` past it, inside `Knobs::BOUNDS`; encounters may go to `x0`, every other scale keeps `x0.1`; the two gates flip). It opens on the knobs of the preset it was reached from, or on the ones last set. A custom run saves its knobs in `RunRules::custom` (a missing field reads as retail's, and a preset's rules carry none, so a save written before *Custom* decodes unchanged). The difficulty tab's help says which session tweaks stack on the preset (`engine-screens` `run_rules::stacking_tweaks_note`: foe stats, foe attacks, EXP, AP, smart foes).

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

An ironman run draws a run id at New Game, which its saves carry. The card writer every host shares (`write_save_into_card` and its siblings) keeps one save per run: after writing a save of an ironman run it erases every other save of that run from the card. When the party falls (the wipe that returns to the title) the world marks the run, and the host erases the run's save from port 1 (`ironman_erase_run`). Each scene entry owes an autosave, which the host writes over the run's own save or into the lowest free save number (`ironman_autosave`). Hardcore adds one rule: the battle Item command is refused, as a pack challenge's `no_items` rule refuses it - except in the sparring tutorial, whose Item lesson rewinds on any other pick and so would hold Tetsu's spar on it for good.

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

Boss Rush fights the story bosses back to back from a New Game. Each stage is a scripted battle row of a retail scene (`boss_rush::RETAIL_STAGES`): Gimard, Caruban, Songi, Zeto, Xain, Dohati, Koru and Cort. The run seeds Vahn, Noa and Gala in their starter gear; before each stage the host enters the stage's scene as an opening leg, the party is raised to the stage's level through the ordinary EXP path and rested, and the fight is armed the way `play-window --battle` arms a row (its record's flag arm and music replayed, then the encounter transition).

An active pack's bosses follow the retail stages, easiest first: each at the level its quest chain first admits it at plus two (`legaia_engine_expansion::quest::boss_chain_levels`: the highest `min_level` along the prerequisite chain and the `reach_level` objectives that chain completes), and a boss no quest names at the last retail stage's level plus five per such boss.

A won stage pays `STAGE_PURSE` gold and opens the intermission: next battle, rest, buy supplies (resolved by name against the disc's item table), or give up. A lost stage ends the run. The clock counts the frames a stage is entered and fought, not the intermission. The leaderboard (`BossRushRecord`: runs, clears, the most stages, the fastest clear) lives on the world and rides every save the session makes.

## The rules screen

The title's NEW GAME opens the rules screen (`engine-session` `open_run_rules_screen`) on both hosts, except in retail-faithful mode, where NEW GAME is retail's straight start. It opens on the retail rules, on the *Run* tab with the cursor on *Start*, so one confirm is the original New Game.

Its rows sit on four tabs (`setup::SetupTab`), switched with L1 / R1:

| Tab | Rows |
|---|---|
| Run | the mode (Story or Boss Rush), ironman (Off, On, Hardcore), *Start*; under Boss Rush, its record and stages |
| Difficulty | the preset, with every scale it sets listed under it (`Knobs::rows`), or under *Custom* each knob as a row |
| New Game+ | the save to carry from, then the seven keep switches while one is picked |
| Randomizer | on or off, then the seed and the seven parts (`Randomizer::parts`) while it is on |

Up / Down pick a row, Left / Right change it (the selected row shows the menu's gold arrows beside its value), Cross confirms, Square puts the tab on screen back to the original game's rules (`RulesSetup::reset_tab`), Start begins the run, Circle returns to the title. A list longer than the window scrolls, marked with the menu font's own list arrows (icons `0x67` / `0x68`). A help window says what the selected row does at its current value (`RulesSetup::help`), a summary line under it gives the run as the pause menu will (`Hard / NG+1 / Ironman / Seed RIM-0042`), and the key hint names the host's own bindings or the last pressed controller's buttons (`engine-screens` `run_rules::PromptKeys`, built from each host's `prompt_label`, the minigame captions' rule).

Start on the original game's rules begins at once, so one confirm is still the retail New Game; any other rules open a confirm window first (*Begin with these rules?*, listing each rule that differs, `RulesSetup::confirm_lines`). Cross on the New Game+ row opens the save list (`setup::SetupOverlay::Sources`): every readable save on either card port with its leader, the place it was written and its play time (the launcher's and the Continue screen's summary), and the help names the cycle it would start.

A Boss Rush hides the rows it ignores: the keep switches (it seats its own party, so only the cycle's tougher foes carry) and the randomizer parts it never reads (`RulesSetup::BOSS_RUSH_PARTS`: drops and steals stay; its fights are scripted, it buys by name, opens no chest, fights the protected story bosses and re-seeds the party's gear). The run tab then lists its record (the best any save on the cards or the session holds) and its stages with their levels, the active pack's bosses after the retail eight.

Left / Right on the seed row rolls a readable seed; Cross opens the seed editor (`setup::SeedEditor`), a letter grid over the rows: the capitals, the dash and the digits, then *Del* and *Done*. Cross types the cell, Circle erases (and leaves an empty grid), Start takes the seed.

A keyboard types into the same editor: the native window sends letters, digits and the dash to it while it is up (Backspace erases, Enter takes the seed, Escape drops the edit), and the page lays a text box over the editor's text line (`play_run_rules_seed_rect`), whose text the engine filters to the characters the font draws. An empty seed keeps the old one. The help line says what the disc patcher does not: a seed names a different shuffle there, since each engine part is drawn per site rather than over the whole disc.

On the page a tap or click goes to the engine, which resolves it from the geometry it draws (`run_rules_screen::rules_menu_hit`, `engine-screens` `run_rules::setup_tap`, `play_run_rules_tap`): a tab, a row (the first tap selects it, a second acts as Cross), a value's arrows, a window's choice, a seed cell; *Begin* and *Back* buttons sit under the canvas (`play_run_rules_button`).

The screen is drawn in the game's own menu style (`engine-ui` `run_rules_screen::RulesMenuView`): the pause menu's 9-slice windows over black, the carved title-tab plaques for the tabs (those off screen darkened), the pointing hand as the cursor, the menu inks. Its sprites sample the system-UI chrome atlas both hosts already hold for the pause menu, its labels the menu font; the black backdrop is a screen prim. Every piece is laid out on the 320x240 stage through the one stage transform (`pause_menu::stage_transform`), which is also the native window's scene viewport for screen prims, so the windows, the backdrop and the text stay together at any window size.

The screen sits on the world (`RunRulesState::setup`), so the shared screen layers draw it (`engine-screens` `run_rules::active_screen`): the native window through its screen-prim pass, its system-UI sprite pass and its text layer, the browser play page on the title's menu canvas (`play_run_rules_draws_json`). The Boss Rush intermission and result are the same composition with the screen's title on one plaque (`run_rules::boss_rush_screen`), drawn in the field through the same passes. The pause menu shows the run's summary line at the foot of the root panel; under retail rules it shows nothing.

## Saves and replays

The run's block is versioned JSON (`save::RunSave`): the rules, a Boss Rush in progress, the leaderboard and the ironman run id. An LGSF file carries it in the optional `LGXF` block after `LGXD`; a card block carries it as the engine-ext tail's fourth `u16`-length segment, after the expansion one, reserved first so the ironman id outranks a quest log. A run with retail rules, no Boss Rush and an empty leaderboard writes no block.

A j-replay-v1 file records the rules as a value row, `set = "run_rules"` with the rules as a `rules` table, noted whenever the world's rules change ([`determinism-replay.md`](../tooling/determinism-replay.md)). The row sets the rules; a New Game+ carry needs its source save, which a replay does not hold.

## Tests

| Test | Crate |
|---|---|
| `retail_rules_are_the_identity`, `presets_order_and_step`, `rules_round_trip_and_summarise` (`rules`); `the_curve_grows_with_cycle_and_level_and_favours_bosses` (`scaling`); `a_run_walks_its_stages_and_records`, `the_intermission_cursor_wraps` (`boss_rush`); `retail_writes_nothing_and_rules_round_trip` (`save`); `knob_rows_read_as_multipliers`, `an_older_rules_block_decodes_with_every_part_on`, `custom_knobs_step_inside_their_bounds_and_save` (`rules`) | `engine-run-rules` |
| `one_confirm_starts_the_retail_game`, `start_begins_the_run_from_any_row_and_tab`, `tabs_wrap_and_keep_their_cursors`, `rows_set_every_rule`, `the_seed_is_typed_on_the_grid_or_the_keyboard` (`setup`); `custom_difficulty_edits_its_knobs_and_remembers_them`, `the_save_list_picks_a_source_and_starts_its_next_cycle`, `a_boss_rush_hides_the_rows_it_ignores`, `taps_act_as_the_pad_does` (`setup`) | `engine-run-rules` |
| `retail_rules_leave_every_reader_alone`, `difficulty_presets_apply_and_clear`, `staged_rules_take_over_at_new_game_and_round_trip_the_save_block`, `ironman_marks_the_run_on_a_wipe_and_owes_autosaves`, `ng_plus_carries_the_chosen_parts`, `the_randomizer_is_a_function_of_its_seed` (`world::tests::run_rules`); `carry_copies_only_the_chosen_spans` (`world::run_rules`) | `engine-core` |
| `picks_are_stable_per_key_and_spread_across_keys`, `kept_items_and_the_empty_sentinel_never_move`, `bands_hold_and_protected_monsters_keep_their_stats`, `rerolls_keep_the_seat_count_the_species_cap_and_the_guard`, `priced_items_pool_and_named_free_ones_are_kept` | `randomizer` |
| `the_menu_keeps_each_label_in_its_window`, `hidden_rows_are_marked_with_the_list_arrows`, `taps_land_where_the_screen_draws`, `the_seed_grid_covers_the_rows_under_it`, `one_heading_is_a_title_plaque` (`run_rules_screen`) | `engine-ui` |
| `the_setup_screen_lists_the_rows_and_the_run`, `the_difficulty_tab_lists_its_scales_and_the_tweaks_on_top`, `a_boss_rush_fills_the_run_tab_with_its_stages_and_record`, `the_confirm_and_the_save_list_are_windows_over_the_rows`, `the_hint_names_the_hosts_keys` (`run_rules`) | `engine-screens` |
| `the_rules_screen_draws_nothing_until_opened` (`play_run_rules`) | `web-play` |
| `erasing_a_save_frees_its_block` (`emu`) | `save` |
| `ironman_keeps_one_save_and_a_wipe_erases_it` (`card_write`) | `engine-menu-runtime` |
| `the_shoulders_switch_the_rules_tabs`, `the_best_boss_rush_record_wins` (`run_rules`) | `engine-session` |
| `boss_rush_rows_seat_their_bosses`, `boss_rush_runs_its_first_stages` (disc-gated, `tests/boss_rush_disc.rs`); `switches_round_trip_in_every_shape` (`replay`); `seed_keys_type_capitals_and_digits` (`window::event_handler::keyboard`) | `engine-shell` |
