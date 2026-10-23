//! Unit tests for [`World`].

use super::*;
use legaia_engine_battle_vm::battle_action::BattleActionHost;
use legaia_engine_mode::mode::SceneMode;
use vm::Insn;

/// Take `Begin` on the party's commit-confirm screen (`0x6E`) when the last
/// commit raised it, so the round plays out. Returns whether it was up.
/// One vsync of the live battle loop with the display clock advanced, as
/// `World::tick` advances it: the action SM runs one pass a battle frame
/// (`World::battle_sm_pass_due`), so a loop that never moves the clock
/// would run it once.
fn live_tick(w: &mut World) -> Option<legaia_engine_battle_vm::battle_action::StepOutcome> {
    w.clock.display_frames += 1;
    w.live_battle_tick()
}

fn take_commit_begin(w: &mut World) -> bool {
    use legaia_engine_battle_menus::battle_input::CommandPhase;
    let Some(cmd) = w.battle.command.as_mut() else {
        return false;
    };
    if !matches!(cmd.phase, CommandPhase::CommitConfirm { .. }) {
        return false;
    }
    cmd.phase = CommandPhase::BeginRound;
    w.tick_battle_command();
    true
}

// ---- tile-board step + collision (A2) ----

/// Tile-board world: 3x3 board, all floor except a wall at (1,1);
/// player actor in slot 0 placed at its start-tile centre.
/// The world seed on which Bandit Boss (monster id 5, magic `[Flame 0x20,
/// Thunder Bolt 0x23]`) picks Flame against a lone party member. Derived by
/// hand off the BIOS stream (`s' = s * 0x41C64E6D + 0x3039`, draw =
/// `(s' >> 16) & 0x7FFF`), which for seed 10 runs 4543, 28214, 11245, 8870,
/// 16887, 2234. The picker draws them in this order: the pick
/// `rand % (1 + 2)` (`4543 % 3 = 1`, so `magic[0]`), the single-target pick
/// `rand % 1`, the monster's scripted `0x51` override `rand & 3 == 0`
/// (`11245 & 3 = 1`, so it stays off), then the flee checkpoint's two score
/// draws and its `rand & 7 == 0` gate (`2234 & 7 = 2`, so no flee). The raw
/// stream's seed `0` did the same job only because its first **raw** word is
/// `1 mod 3`.
const BANDIT_BOSS_FLAME_SEED: u32 = 10;

/// Run the live loop until the armed cast's owed outcome has folded - the
/// band's `0x29` exit for a non-summon cast, the stager's strike for a Seru
/// one (`World::casting.pending_cast` goes `None`). A world with nothing armed
/// returns at once.
fn tick_until_cast_folds(w: &mut World) {
    for _ in 0..0x400 {
        if w.casting.pending_cast.is_none() {
            return;
        }
        w.set_pad(0);
        let _ = w.tick();
    }
    panic!(
        "the cast never folded (state {:#04x}, actor {})",
        w.battle_ctx.action_state, w.battle_ctx.active_actor
    );
}

fn tile_board_world() -> World {
    let mut w = World::new();
    w.mode = SceneMode::Field;
    w.player_actor_slot = Some(0);
    w.actors[0].active = true;
    let cells = vec![
        1,
        1,
        1,
        1,
        legaia_engine_minigames::tile_board::CELL_WALL,
        1,
        1,
        1,
        1,
    ];
    let board = legaia_engine_minigames::tile_board::TileBoard::new(3, 3, 0, 0, cells);
    w.board.grid = Some(board);
    let (x, z) = w.board.grid.as_ref().unwrap().player_world();
    w.actors[0].move_state.world_x = x as i16;
    w.actors[0].move_state.world_z = z as i16;
    w
}

/// Build a 1-party-member, 1-monster battle world for the offensive-item
/// tests. The monster sits at slot 1 (party_count = 1) with the supplied
/// A purpose-built item catalog covering every effect *type* at stable test
/// ids, for the world item-apply tests. These exercise the apply/grant kernels,
/// not the shipped consumable list, so they use fixed ids rather than
/// [`legaia_engine_menus::items::ItemCatalog::vanilla`]'s real retail ids (which only models a
/// faithful subset; stat-boost / capture / damage / battle-escape consumables
/// aren't shipped yet). `vanilla()`'s real-id correctness is pinned in `items.rs`
/// + the disc-gated `item_catalog_disc` test.
#[cfg(test)]
fn full_test_catalog() -> legaia_engine_menus::items::ItemCatalog {
    use legaia_engine_battle_vm::status_effects::StatusKind;
    use legaia_engine_menus::items::{ItemCatalog, ItemEffect, ItemEntry, StatBoostTarget};
    let mut c = ItemCatalog::new();
    let mut add = |id, name, effect, b, f| {
        c.insert(ItemEntry {
            id,
            name,
            effect,
            usable_in_battle: b,
            usable_in_field: f,
        })
    };
    add(0x01, "Heal", ItemEffect::Heal { amount: 100 }, true, true);
    add(0x04, "Heal All", ItemEffect::HealAll, true, true);
    add(0x05, "Magic", ItemEffect::HealMp { amount: 30 }, true, true);
    add(
        0x08,
        "Cure Poison",
        ItemEffect::Cure {
            kind: StatusKind::Venom,
        },
        true,
        true,
    );
    add(0x09, "Cure All", ItemEffect::CureAll, true, true);
    add(
        0x0C,
        "Revive",
        ItemEffect::Revive { factor: 128 },
        true,
        true,
    );
    add(
        0x0E,
        "Attack Up",
        ItemEffect::StatBoost {
            target: StatBoostTarget::Attack,
            delta: 1,
        },
        false,
        true,
    );
    add(
        0x0F,
        "HP Up",
        ItemEffect::StatBoost {
            target: StatBoostTarget::HpMax,
            delta: 10,
        },
        false,
        true,
    );
    add(
        0x10,
        "Spirit",
        ItemEffect::Spirit { amount: 5 },
        true,
        false,
    );
    add(
        0x11,
        "Capture",
        ItemEffect::Capture { strength: 100 },
        true,
        false,
    );
    add(0x12, "Escape", ItemEffect::Escape, true, false);
    add(
        0x13,
        "Bomb",
        ItemEffect::Damage { amount: 200 },
        true,
        false,
    );
    c
}

/// HP and a `battle_monster_id` so it shows up as an enemy target row.
#[cfg(test)]
fn offensive_item_world(monster_hp: u16, monster_id: u16) -> World {
    let mut world = World {
        party: legaia_engine_menus::party_state::PartyState {
            party_count: 1,
            ..Default::default()
        },
        ..World::default()
    };
    world.battle.player_driven = true;
    world.mode = SceneMode::Battle;
    world.set_item_catalog(full_test_catalog());
    world.actors[0].battle.max_hp = 200;
    world.actors[0].battle.hp = 200;
    world.actors[0].battle.liveness = 1;
    world.actors[1].battle.max_hp = monster_hp;
    world.actors[1].battle.hp = monster_hp;
    world.actors[1].battle.liveness = 1;
    world.actors[1].battle_monster_id = Some(monster_id);
    world.battle_ctx.active_actor = 0;
    world
}

/// A registry whose Seru hits its learn threshold in one capture, and a
/// monster catalog linking monster id 7 -> Seru 1.
#[cfg(test)]
fn capture_world(party_count: u8) -> World {
    use legaia_engine_battle::monster_catalog::{MonsterCatalog, MonsterDef};
    use legaia_engine_battle::seru_learning::{SeruDef, SeruRegistry};

    let mut world = World {
        party: legaia_engine_menus::party_state::PartyState {
            party_count,
            ..Default::default()
        },
        ..World::default()
    };
    // Zeroed roster (empty spell lists) so `build_battle_spell_session`
    // resolves a member per party slot; learned spells come from the log.
    world.party.roster = legaia_save::Party::zeroed(party_count.max(1) as usize);
    let mut reg = SeruRegistry::new();
    reg.insert(SeruDef {
        id: 1,
        name: "Spark".into(),
        spell_id: 0x20,
        capture_points: 100,
        learnable_mask: 0b0000_0111,
        learn_threshold: 100,
    });
    reg.insert(SeruDef {
        id: 2,
        name: "Slow".into(),
        spell_id: 0x21,
        capture_points: 40, // below threshold in one capture
        learnable_mask: 0b0000_0111,
        learn_threshold: 100,
    });
    world.set_seru_registry(reg);
    let mut cat = MonsterCatalog::new();
    cat.insert(MonsterDef::new(7, "Killer Bee", 25, 9).with_seru(1));
    cat.insert(MonsterDef::new(8, "Slime", 40, 8).with_seru(2));
    cat.insert(MonsterDef::new(9, "Wolf", 35, 12)); // no Seru
    world.set_monster_catalog(cat);
    world
}

// ---------------------------------------------------------------------------
// Summon spell XP + level-up (FUN_801ddb30 tail / FUN_801e70bc) and the
// Lost Grail Final Heal revive (FUN_801e6968).
// ---------------------------------------------------------------------------

/// World with one party member (slot 0), a roster record carrying summon
/// spell `0x81` at level 1, and an enemy in slot 1.
fn summon_xp_world(enemy_hp: u16, enemy_max_hp: u16) -> World {
    use legaia_save::Party;
    let mut world = World {
        party: legaia_engine_menus::party_state::PartyState {
            party_count: 1,
            ..Default::default()
        },
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    world.party.roster = Party::zeroed(1);
    let rec = &mut world.party.roster.members[0];
    let mut list = rec.spell_list();
    list.count = 1;
    list.ids[0] = 0x81;
    list.levels[0] = 1;
    rec.set_spell_list(list);

    world.actors[0].battle.max_hp = 400;
    world.actors[0].battle.hp = 400;
    world.actors[0].battle.mp = 200;
    world.actors[0].battle.liveness = 1;
    world.set_battle_magic(0, 40);
    world.actors[1].battle.max_hp = enemy_max_hp;
    world.actors[1].battle.hp = enemy_hp;
    world.actors[1].battle.liveness = 1;
    world.battle.defense[1] = 0;
    world
}

fn gimard_spell_def() -> legaia_engine_battle::spells::SpellDef {
    use legaia_engine_battle::spells::{SpellDef, SpellEffect, SpellElement, SpellTarget};
    SpellDef {
        id: 0x81,
        name: "Gimard".into(),
        mp_cost: 4,
        element: SpellElement::Neutral,
        target: SpellTarget::OneEnemy,
        effect: SpellEffect::Damage {
            base_power: 100,
            element: SpellElement::Neutral,
        },
        anim_id: 0,
        ..Default::default()
    }
}

mod actor_alloc_init;
mod actor_cadence;
mod ambient_cell_cycler;
mod ambient_collision_exempt;
mod battle_anim;
mod battle_ap_carry;
mod battle_auto_command;
mod battle_capture_bgm;
mod battle_capture_class_disc;
mod battle_clip_ladder;
mod battle_dead_target;
mod battle_intro_names_sweep;
mod battle_item_use_cue;
mod battle_items_magic;
mod battle_loot_use_item;
mod battle_loss_window;
mod battle_outcome_flag;
mod battle_party_hp_seed;
mod battle_queue_gates;
mod battle_return_camera;
mod battle_rot_gate;
mod battle_small_party_seats;
mod battle_small_party_settle_gate_disc;
mod battle_special_ai;
mod battle_special_word;
mod battle_status;
mod battle_stone_gaze;
mod battle_turns_items;
mod battle_tutorial_flow;
mod battle_ui_cue_routing;
mod battle_xp_attack;
mod camera_offset_ease;
mod cast_band;
mod cast_presentation_disc;
mod commit_confirm;
mod core;
mod counterattack;
mod dialogue_runner_fx;
mod effect_playback;
mod effects_actors;
mod encounters;
mod fatal_decision;
mod field_events;
mod field_grid;
mod field_interaction;
mod field_kernels;
mod field_npc_motion;
mod field_records;
mod field_timer_actors;
mod flash_limiter;
mod fog_region_reset;
mod game_over_hold;
mod gameplay_tweaks;
mod inline_dialogue;
mod item_bag_steal;
mod item_held_query;
mod live_battle;
mod locomotion;
mod minigames;
mod model_rebind;
mod monster_flee_disc;
mod monster_scripted_ai;
mod move_vm_ext;
mod move_vm_flags;
mod party_composition;
mod physics_steal_shop;
mod placement_heading;
mod player_clip_pick;
mod player_scale;
mod round_prompt_gate;
mod save_state;
mod script_teleport;
mod scripted_menu_press;
mod seru_absorb;
mod seru_side_effect_live_disc;
mod set_equip_slot;
mod shiny;
mod slide;
mod sound_stream_gates;
mod sparring_solo;
mod spirit_gauge;
mod summon_final_heal;
mod system_bbox_target;
mod take_item;
mod tile_board;
mod worldmap;

mod battle_auto_attack;
mod battle_formation_span;
mod battle_make_way;
mod battle_rage_turn;
mod battle_target_ring;
