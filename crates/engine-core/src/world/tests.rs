//! Unit tests for [`World`]. Split out of `world.rs`.

use super::*;
use vm::Insn;
use vm::battle_action::BattleActionHost;

// ---- tile-board step + collision (A2) ----

/// Tile-board world: 3x3 board, all floor except a wall at (1,1);
/// player actor in slot 0 placed at its start-tile centre.
fn tile_board_world() -> World {
    let mut w = World::new();
    w.mode = SceneMode::Field;
    w.player_actor_slot = Some(0);
    w.actors[0].active = true;
    let cells = vec![1, 1, 1, 1, crate::tile_board::CELL_WALL, 1, 1, 1, 1];
    let board = crate::tile_board::TileBoard::new(3, 3, 0, 0, cells);
    w.tile_board = Some(board);
    let (x, z) = w.tile_board.as_ref().unwrap().player_world();
    w.actors[0].move_state.world_x = x as i16;
    w.actors[0].move_state.world_z = z as i16;
    w
}

/// Build a 1-party-member, 1-monster battle world for the offensive-item
/// tests. The monster sits at slot 1 (party_count = 1) with the supplied
/// A purpose-built item catalog covering every effect *type* at stable test
/// ids, for the world item-apply tests. These exercise the apply/grant kernels,
/// not the shipped consumable list, so they use fixed ids rather than
/// [`crate::items::ItemCatalog::vanilla`]'s real retail ids (which only models a
/// faithful subset; stat-boost / capture / damage / battle-escape consumables
/// aren't shipped yet). `vanilla()`'s real-id correctness is pinned in `items.rs`
/// + the disc-gated `item_catalog_disc` test.
#[cfg(test)]
fn full_test_catalog() -> crate::items::ItemCatalog {
    use crate::items::{ItemCatalog, ItemEffect, ItemEntry, StatBoostTarget};
    use legaia_engine_vm::status_effects::StatusKind;
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
        party_count: 1,
        ..World::default()
    };
    world.battle_player_driven = true;
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
    use crate::monster_catalog::{MonsterCatalog, MonsterDef};
    use crate::seru_learning::{SeruDef, SeruRegistry};

    let mut world = World {
        party_count,
        ..World::default()
    };
    // Zeroed roster (empty spell lists) so `build_battle_spell_session`
    // resolves a member per party slot; learned spells come from the log.
    world.roster = legaia_save::Party::zeroed(party_count.max(1) as usize);
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
        party_count: 1,
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    world.roster = Party::zeroed(1);
    let rec = &mut world.roster.members[0];
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
    world.battle_defense[1] = 0;
    world
}

fn gimard_spell_def() -> crate::spells::SpellDef {
    use crate::spells::{SpellDef, SpellEffect, SpellElement, SpellTarget};
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
    }
}

mod actor_cadence;
mod battle_anim;
mod battle_capture_bgm;
mod battle_effects_ai2;
mod battle_items_magic;
mod battle_loot_use_item;
mod battle_special_ai;
mod battle_status;
mod battle_turns_items;
mod battle_xp_attack;
mod core;
mod dialogue_runner_fx;
mod effects_actors;
mod encounters;
mod field_events;
mod field_grid;
mod field_interaction;
mod field_npc_motion;
mod field_records;
mod inline_dialogue;
mod live_battle;
mod locomotion;
mod minigames;
mod move_vm_ext;
mod move_vm_flags;
mod party_composition;
mod physics_steal_shop;
mod save_state;
mod script_teleport;
mod shiny;
mod summon_final_heal;
mod tile_board;
mod worldmap;
