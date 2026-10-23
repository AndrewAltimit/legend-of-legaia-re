# Gameplay tweaks: patcher features in the engine

The disc patcher ([`randomizer.md`](../tooling/randomizer.md)) proves a change against retail: it runs on the original game, on a console or an emulator. A change that is a fix or a gameplay option rather than new disc content is expected to graduate into an engine switch, so a player of the port gets it without patching a disc. This page sorts every patcher feature into one of three classes, and documents the switches the engine carries.

## Two ways a patcher feature reaches the engine

The engine reads its game data from whatever disc it is given, and it never executes the game's MIPS code. That splits the patcher in two:

- **Data edits reach the engine on their own.** A randomized drop table, a re-priced shop, a rescaled monster record or a re-routed door is bytes in `PROT.DAT` or `SCUS_942.54`, and the engine parses those bytes at boot or scene entry. A disc patched this way plays patched in the port too.
- **Code hooks do not.** A hook is MIPS spliced into the executable or an overlay. The engine runs its own Rust ports of those routines, so a hook's behaviour exists in the port only when the port implements it. The one hook whose data the engine reads is Seru trading, whose offer configuration block the engine parses out of the patched `SCUS_942.54` (`World::install_seru_trade_config`).

A switch below therefore stacks with a patched disc: the engine's EXP rate multiplies a disc already patched with `--exp-scale`.

## Classification

Classes:

- **(a) Avoided by construction** - a retail defect the patcher repairs and the port cannot reproduce, because the structure the defect lives in does not exist in the port. No switch.
- **(b) Engine switch** - a fix or a gameplay option that belongs in the engine. Either carried (the switches below) or open (ranked under [Open](#open)).
- **(c) Patcher only** - disc-content randomizers, content mods and per-record edits. Their output is a disc, the engine plays it as data, and an engine switch would be a second implementation of the same edit.

| Patcher feature | Class | Engine side |
|---|---|---|
| `--approach-softlock-fix` | (a) | The approach drive does not depend on a live clip, so the state-`0x19` park cannot form ([battle-action-exit-gates](battle-action-exit-gates.md)). |
| `--conkram-freeze-fix` | (a) | Move-VM op `0x3C`'s keyframe pose is a `Vec` sized from its count, not a heap block; there is no failed allocation to copy through (`engine-vm` `move_vm::dispatch`). |
| Charm softlock fix (`charm_fix`, ships with `--enemy-ally`) | (a) | The victory arm keeps the acting slot only when it is a living party slot (`victory_pose_fixup`, `engine-battle-vm` `battle_action::done`); the patch mirrors that invariant. |
| Arts name fix (ZetaPhoenix, ships with `--super-arts-pack`) | (a) | The banner is centred on the name the port draws, measured each time (`engine-ui` `battle_name_banner`), so there is no stale width to keep. |
| `--shiny-seru` | (b) carried | `WorldToggles::shiny_seru`. |
| `--flee-exp` | (b) carried | `WorldToggles::flee_exp`. |
| `--exp-scale` | (b) carried | `WorldToggles::exp_rate_permille`. |
| `--fortress-encounter-fix` | (b) carried | `WorldToggles::fortress_encounter_fix`. |
| `--jewel-fix` | (b) carried | `WorldToggles::jewel_fix`. |
| (no patcher flag) state-`0x51` HP-readout park | (b) carried | `WorldToggles::hp_readout_guard`; the patcher has no fix for this park. |
| `--enemy-hp-bar` | (b) open | Needs a HUD draw over each monster. |
| `--equipment-drops` (bonus drop) | (b) open | A spoils-time roll over the equipment pool. |
| `--enemy-ally` | (b) open | Needs a monster seated on the party's side through the round scheduler. |
| `--seru-trade` | (b) open | Runs today only on a disc patched with the config; a switch needs an engine-side config source. |
| `--show-super-arts` | (b) open | A move-list row source for performed Super Arts. |
| `--spirit-ap`, `--damage-ap`, `--seru-catch-rate`, `--enemy-attack-count`, `--enemy-stat-scale` | (b) open | Tuning sliders over values the engine already reads; each is a scale at its reader. |
| `--oscillating-ap` | (b) open | A per-battle mechanic over the arts AP cost. |
| `--drops`, `--chests`, `--steals`, `--shops`, `--casino`, `--encounters` (with `--encounter-scope`, the solo-strong pass) | (c) | Disc tables the engine parses. |
| `--doors`, house doors, map doors, `--rename-location` | (c) | Scene MAN and place-name data. |
| `--monster-stats`, `--move-power`, `--element-affinity`, `--spell-cost`, `--equip-bonus`, `--equip-mask`, `--weapon-specialty`, `--swing-cost`, `--equip-owner` | (c) | Randomized or hand-set records the engine reads. |
| `--arts`, `--arts-power`, `--super-art-power`, `--arts-ap-grant`, `--arts-ap-cost` | (c) | Per-art record edits. |
| `--fishing-price`, `--earth-egg-price` | (c) | Script and table literals the engine reads. |
| `--starting-items` and the starting-bag toggles, `--start-with`, `--all-warps`, `--starting-level` | (c) | New-game template and opening-scene script edits. |
| `--unused-enemies`, `--unused-items`, `--custom-items` | (c) | Content the disc carries or gains. |
| `--super-arts-pack`, `--delilas-challenge`, `--delilas-party` and its `--delilas-*` modes | (c) | Content mods: new arts, a course, a party swap. |
| Texture replacement, custom monster models, translation packs | (c) | Asset and text replacement. |

## The carried switches

Each switch is one field of `legaia_engine_session::session_switches::GameplayTweaks`, which both play hosts carry whole: the native window takes it at launch (`LaunchFidelity::tweaks`, from the launcher's *Gameplay tweaks* rows or the `play-window` flags), the browser play page at any time (`PageToggles::tweaks`, its *Gameplay tweaks* panel). `SessionSwitches::apply` writes it onto `WorldToggles`, except in retail-faithful mode, where `WorldToggles::pin_retail` holds every switch at retail's value.

| Switch | `play-window` flag | Default | Retail mode |
|---|---|---|---|
| Shiny Seru | `--no-shiny-seru` | on | off |
| Run-away EXP | `--flee-exp` | off | off |
| EXP rate | `--exp-rate MULT` (`0.1..=5`) | `1x` | `1x` |
| Fortress battles fix | `--no-fortress-encounter-fix` | on | off |
| Boss casts respect jewels | `--jewel-fix` | off | off |
| Battle freeze guard | `--no-hp-readout-guard` | on | off |

The two fixes default on because a game the defect never touched plays identically with them on: the Fortress fix changes nothing on a save without the stale flag, and the readout guard is an identity while retail's readout invariant holds. Shiny Seru keeps the default it shipped with. The jewel fix, run-away EXP and the EXP rate change the game's balance and default to retail.

### Shiny Seru

At each formation battle entry, after the monster stats are seeded, the engine rolls `SeruState::shiny_chance_pct` (2%): on a hit one capturable monster gets +35% HP / ATK / DEF / SPD, and capturing it marks the learned spell shiny, whose casts deal +35% damage (`World::roll_shiny_enemy`, `seru_learning`). The roll is the patcher hook's rule.

Off, the roll is skipped whole, including its `rand()` draw. Retail has no shiny roll, so the draw it took moved every later battle-entry roll in the shared stream (the formation advantage first) off retail's position. Before the switch existed the roll was unconditional, so retail-faithful mode carried that draw.

### Run-away EXP

On a successful escape, by any path (the Run command, an escape item or spell), the engine sums the listed EXP of every monster in the fled formation, at the EXP rate, and banks `FLEE_EXP_PCT` (5%) of it into each party member's cumulative XP (`World::bank_flee_exp`, called from the escape arm of `begin_battle_end_sequence`). It is banked, as the hook banks it: the XP total moves, no level-up runs, and the next won battle's level check crosses whatever threshold the total reached. Each total saturates at `9,999,999`. A special battle (`_DAT_8007BAC0 != 0`) banks nothing, as its win pays nothing.

The hook runs in the action SM's escape teardown (state `0x66`); the engine grants at the end-sequence arm where every escape path meets, so an escape through an item or a spell pays too.

### EXP rate

Each monster record's base EXP is scaled before the victory sum (`apply_battle_loot`) with the patcher's `--exp-scale` arithmetic (`scale_exp_permille`): round half up, a zero reward stays zero, a non-zero one clamps to `1..=65535`. The retail 3/4 scale and the per-member ceiling split then run on the scaled sum unchanged. Run-away EXP sums the scaled values too, as the patch makes the hook read the scaled record field.

### Fortress battles fix

The encounter condition walk (`FUN_801D9E1C`, `RegionEncounterTable::select_group`) asks `World::encounter_condition_flag_test` instead of the raw flag bank. In `deroa` and `chitei2` that answers system flag `0x15` as clear, so the walk falls past the Floating Castle's copied "no battles" group to the group retail reaches on a healthy save. The castle `taiku` still reads the flag, so its last-minute quiet window stays. The defect and the scenes are traced in [field-oddities](../reference/glitches/field-oddities.md#no-encounters-in-jettes-absolute-fortress). The patch reaches the same walk by rewriting group 0 into a duplicate of group 1; the engine reaches it without touching the table.

### Boss casts respect jewels

The capture-class boss casts whose modules call the resist-bypass wrapper `FUN_801DD6B4` (Guilty Cross, Bloody Horns, Terio Punch, Bull Charge, Blazing Slash, Megaton Press, Plasma Strike) roll through the respecting wrapper `FUN_801DD4B0` instead (`enemy_move_predamage`, `casting/predamage.rs`). That is what the patch's `jal` retarget does, so the engine swaps the whole roll, not only the finisher's resist flag. The engine's ports of the two wrappers roll different attacker and defender terms (`engine-battle-vm` `battle_damage_wrappers`, [battle-formulas](battle-formulas.md)), where the patch's module header (`code-hooks` `jewel_fix`) reads the retarget as changing only the finisher's `param_5`; the switch follows the wrapper ports.

### Battle freeze guard

Before each frame of the HP-readout ramp (`World::tick_battle_hp_bars`, retail `FUN_80047430`), a party slot whose readout differs from its live HP while its pending accumulator is zero gets the difference re-seeded into the accumulator. That restores the invariant `display - live == pending`, the ramp counts the readout to live HP, and state `0x51`'s settle check opens. Retail re-syncs only on a poison or regen tick (`FUN_801E752C`); without one, every party-targeted action parks in `0x51` for good ([battle-action-exit-gates](battle-action-exit-gates.md#softlock-class-the-absorbing-readout)). Monster slots are untouched: their readout settles in one frame and the gate never waits on it.

## Verification

Each switch has a disc-free test that it changes behaviour when on and that its retail value is the unmodified path:

| Test | Crate |
|---|---|
| `shiny_toggle_off_takes_no_battle_entry_draw`, `flee_exp_banks_a_share_of_the_fled_formation`, `exp_rate_scales_the_victory_payout`, `fortress_fix_skips_the_stale_castle_group`, `hp_readout_guard_releases_the_absorbing_readout` (`world::tests::gameplay_tweaks`) | `engine-core` |
| `jewel_fix_makes_a_bypass_cast_respect_the_resist_ladder` (`casting::capture_bypass_tests`) | `engine-core` |
| `retail_masks_every_tweak`, `tweak_defaults_are_the_engine_defaults_and_rates_cycle` (`session_switches`) | `engine-session` |
| `gameplay_tweak_rows_ride_the_args` (`launcher`), `no_subcommand_parses_and_play_args_reach_play_window` (`launcher_window`) | `engine-shell` |
| `gameplay_tweaks_start_at_the_engine_defaults` (`play_toggles`) | `web-play` |

Tier 3 of the [host-drift gate](../tooling/host-drift.md) requires both hosts' switch sites to pass the `tweaks` field, and the host frame trace digests the tweaks, so a switch thrown on one host only shows as a divergence.

## Open

Ranked by how much of the engine each needs:

1. **Tuning sliders** (`--spirit-ap`, `--damage-ap`, `--seru-catch-rate`, `--enemy-attack-count`, `--enemy-stat-scale`) - a scale at one reader each, the EXP rate's shape.
2. **Bonus equipment drop** - one extra roll in `apply_battle_loot` over the equipment pool `bonus_drop` uses.
3. **Seru trading without a patched disc** - an engine-built `SeruTradeConfig` in place of the one parsed from the executable.
4. **Enemy HP bars** - a battle-HUD draw over each living monster, on both hosts.
5. **Super Arts on the move list** and **oscillating AP** - battle-menu row sources.
6. **Enemy ally (charm)** - a monster acting on the party's side through the round scheduler, with the victory fixup already in place.
