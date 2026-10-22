//! The action SM's cast band as the engine drives it: a player Seru cast
//! routed through the magic + summon bands with the stager folding the
//! outcome at its strike, and a monster cast routed through the magic band
//! with the fold on the `0x29` exit.

use super::*;
use legaia_engine_mode::mode::SceneMode;

mod band_flow;
mod creature_seat_reuse;
mod fight_kind_gates;
mod hit_folds;
mod module_completion;
mod module_geometry;
mod monster_casts;
mod nighto;
mod restore_spells;
mod shatter_writers;
mod stagers_and_seats;
mod summon_bodies;

/// A one-member party with Gimard (0x81) learned, one monster, the retail
/// Seru-magic catalog, the spell submenu open on the caster.
fn seru_cast_world() -> World {
    let mut world = World {
        party: legaia_engine_menus::party_state::PartyState {
            party_count: 1,
            ..Default::default()
        },
        ..World::default()
    };
    // A live session seats the creature above the eight battle slots.
    while world.actors.len() < 12 {
        world.actors.push(Actor::default());
    }
    world.battle.player_driven = true;
    world.mode = SceneMode::Battle;
    world.tables.spell_catalog = legaia_engine_battle::retail_magic::retail_seru_magic_catalog();
    world.actors[0].active = true;
    world.actors[0].battle.max_hp = 200;
    world.actors[0].battle.hp = 200;
    world.actors[0].battle.mp = 50;
    world.actors[0].battle.liveness = 1;
    world.actors[0].move_state.world_x = 82;
    world.actors[0].move_state.world_z = -542;
    world.actors[0].battle.facing_angle = 0xFD9;
    world.set_battle_magic(0, 100);
    world.actors[1].active = true;
    world.actors[1].battle.max_hp = 300;
    world.actors[1].battle.hp = 300;
    world.actors[1].battle.liveness = 1;
    world.actors[1].move_state.world_z = 543;
    let mut party = legaia_save::Party::zeroed(1);
    let mut list = party.members[0].spell_list();
    list.count = 1;
    list.ids[0] = 0x81;
    party.members[0].set_spell_list(list);
    world.party.roster = party;
    world.battle_ctx.active_actor = 0;
    world.battle.spell_menu = world.build_battle_spell_session(0);
    world
}

fn confirm_cast(world: &mut World) {
    use legaia_engine_system::input::PadButton;
    // Cross on the spell row opens the target cursor; Cross again confirms.
    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_spell_menu();
    take_commit_begin(world);
    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_spell_menu();
    take_commit_begin(world);
    world.set_pad(0);
}

// ---------------------------------------------------------------------------
// The band's PORT half: the slot-B module code kernels reached from the
// stager tick the action SM drives (`World::run_cast_module_code`).
// ---------------------------------------------------------------------------

/// A minimal battle world with the summon seat live, so the module kernels
/// that pose `actor_table[7]` have something to write.
fn module_code_world() -> World {
    let mut world = World {
        party: legaia_engine_menus::party_state::PartyState {
            party_count: 1,
            ..Default::default()
        },
        ..World::default()
    };
    while world.actors.len() < 12 {
        world.actors.push(Actor::default());
    }
    world.mode = SceneMode::Battle;
    for i in 0..8 {
        world.actors[i].active = true;
        world.actors[i].battle.hp = 100;
        world.actors[i].battle.max_hp = 100;
        world.actors[i].battle.liveness = 1;
        world.actors[i].battle.anim_rate = legaia_engine_battle_vm::battle_anim_rate::AnimRate(8);
    }
    world.battle_ctx.active_actor = 0;
    world
}

/// A one-member roster whose record carries `spell_id` at magic level `level`
/// (the cure ladder reads the record, not the actor).
fn roster_with_spell(world: &mut World, spell_id: u8, level: u8) {
    let mut member = legaia_save::CharacterRecord::parse(&[0u8; 0x414]).expect("blank record");
    let mut list = member.spell_list();
    list.count = 1;
    list.ids[0] = spell_id;
    list.levels[0] = level;
    member.set_spell_list(list);
    world.party.roster.members = vec![member];
}

/// Re-enter the module from arm 0 through `last`, each arm until it lets the
/// phase through (the counted arms hold on the module's own countdown).
fn drive_module_through(world: &mut World, spell_id: u8, last: u8) {
    for arm in 0..=last {
        if world.casting.module_phase > arm {
            continue;
        }
        world.casting.module_phase = arm;
        for _ in 0..2048 {
            if world.run_cast_module_code(spell_id, arm).is_none() {
                break;
            }
            if world.casting.module_phase != arm {
                break;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// PROT 0907 (Nighto): the kill / confuse fork, driven at the band
// ---------------------------------------------------------------------------

/// Run PROT 0907's phase chain far enough that the fork arm has executed
/// and the leg it took has run: the director holds each counted arm for its
/// countdown, so this runs until the kill leg's settle (arm 14, which holds
/// on the victim's fade) or the end of the cast.
fn run_nighto_to_the_fork(world: &mut World) {
    use legaia_engine_cast::seru_magic_ticks::NIGHTO_KILL_SETTLE_ARM;
    for _ in 0..8192 {
        if world.run_cast_module_code(0x85, 0).is_none() {
            break;
        }
        let phase = world.casting.module_phase;
        if phase == NIGHTO_KILL_SETTLE_ARM || phase == 0xFF {
            break;
        }
    }
}
