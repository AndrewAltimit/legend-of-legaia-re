//! Stager arms run from the cast-band seam, the sweep modules, and the seat-row
//! mapping.

use super::*;

/// PROT 0909 (Viguro) is spell id `0x87` through the action-id dispatcher's
/// own arithmetic (`903 + (id - 0x81)`), so this resolves with no disc: a
/// queued op-`0x20` call to the stager's arm 0 poses the summon seat and
/// advances the module phase.
#[test]
fn the_viguro_stager_runs_from_the_cast_band_seam() {
    let mut world = module_code_world();
    assert_eq!(world.cast_module_for(0x87), Some(909));
    world.casting.summon_actor_slot = Some(7);
    world.actors[7].battle.active_target = 2;
    world.actors[7].battle.render_flag = 0xFF;
    // The stager is reached through a part's op `0x20`, queued for the
    // module's next tick.
    world
        .casting
        .module_stager_calls
        .push((0, Default::default()));

    let run = world
        .run_cast_module_code(0x87, 0)
        .expect("PROT 0909 is a band entry");
    assert_eq!(run.prot_entry, 909);
    assert_eq!(
        world.actors[7].battle.active_target,
        legaia_engine_cast::cast_module_ticks::TARGET_CODE_ENEMY_ROW,
        "arm 0 retargets the summon seat to the enemy row"
    );
    assert_eq!(
        world.actors[7].battle.render_flag, 0,
        "and makes it visible"
    );
    assert_eq!(world.casting.module_phase, 1, "arm 0 advances ctx+0x279");
}

/// PROT 0922 (Puera, spell `0x94`)'s stager writes one byte and nothing
/// else, and only on arm 0 - the `bnez a1` at the routine's head. It is
/// reached through a part's op-`0x20` call, not off the module phase.
#[test]
fn the_puera_stager_writes_ctx_278_only_on_arm_zero() {
    let mut world = module_code_world();
    assert_eq!(world.cast_module_for(0x94), Some(922));
    // Arm 3 of the body writes no `ctx[+0x278]` of its own.
    world.casting.module_phase = 3;
    world
        .casting
        .module_stager_calls
        .push((1, Default::default()));
    world.run_cast_module_code(0x94, 3).unwrap();
    assert_eq!(world.casting.module_ctx_278, 0);
    world.casting.module_phase = 3;
    world
        .casting
        .module_stager_calls
        .push((0, Default::default()));
    world.run_cast_module_code(0x94, 3).unwrap();
    assert_eq!(world.casting.module_ctx_278, 3);
}

/// PROT 0927 (Juggernaut, spell `0x99`) sweeps the enemy row with the
/// never-kill clamp: the party is untouched and no monster drops below 1 HP.
#[test]
fn the_juggernaut_sweep_spares_the_party_and_never_kills() {
    let mut world = module_code_world();
    // A party of three: seats `0..3` are the party, `3..8` the enemy row.
    world.party.party_count = 3;
    assert_eq!(world.cast_module_for(0x99), Some(927));
    for i in 3..8 {
        world.actors[i].battle.hp = 40;
        world.actors[i].battle_monster_id = Some(1);
    }
    world.set_battle_attack(0, 400);
    let run = world
        .run_cast_module_aoe(0x99, 0)
        .expect("PROT 0927 has a never-kill damage shape");
    assert_eq!(run.prot_entry, 927);
    assert!(
        !run.aoe_hits.is_empty(),
        "the sweep reached at least one seat"
    );
    assert!(
        run.aoe_hits.iter().all(|h| h.seat >= 3),
        "the party row is not swept"
    );
    for i in 0..3 {
        assert_eq!(world.actors[i].battle.hp, 100, "party seat {i} untouched");
    }
    for h in &run.aoe_hits {
        assert!(
            world.actors[h.seat as usize].battle.hp >= 1,
            "seat {} was killed by a HP-1 clamp",
            h.seat
        );
    }
    // A spell whose module has no never-kill shape takes the ordinary fold.
    assert!(world.run_cast_module_aoe(0x87, 0).is_none());
}

/// A lone member's monsters sit at engine slots `1..6`, but the kernels walk
/// retail's row, where they sit at `3..8` (`FUN_800513F0` seats monster `k` at
/// `3 + k` for every party size). Juggernaut's sweep over that row reaches
/// every engine monster and reports them by engine slot; nothing lands on the
/// lone member.
#[test]
fn a_lone_members_juggernaut_sweeps_the_engine_monster_row() {
    let mut world = module_code_world();
    assert_eq!(world.party.party_count, 1);
    for i in 1..6 {
        world.actors[i].battle_monster_id = Some(1);
        world.actors[i].battle.hp = 40;
    }
    for i in 6..12 {
        world.actors[i].active = false;
    }
    world.set_battle_attack(0, 400);
    assert_eq!(world.cast_module_ctx().monster_count, 5);
    let run = world.run_cast_module_aoe(0x99, 0).expect("PROT 0927");
    let mut seats: Vec<u8> = run.aoe_hits.iter().map(|h| h.seat).collect();
    seats.sort_unstable();
    assert_eq!(seats, vec![1, 2, 3, 4, 5], "every engine monster slot");
    assert_eq!(world.actors[0].battle.hp, 100, "the lone member untouched");
    for i in 1..6 {
        assert!(world.actors[i].battle.hp >= 1);
        assert!(world.actors[i].battle.hp < 40, "monster {i} took the sweep");
    }
}

/// A lone member casting Freed (PROT 0912, spell `0x8A`): the body freezes,
/// hides and hits retail's monster row `3..7`. Through the seam that row is
/// the engine's compacted monsters `1..5`, so every engine monster is hit and
/// the lone member - and the empty table above the monsters - is not.
#[test]
fn a_lone_members_freed_sweeps_the_engine_monster_row() {
    let mut world = module_code_world();
    assert_eq!(world.cast_module_for(0x8A), Some(912));
    for i in 1..5 {
        world.actors[i].battle_monster_id = Some(1);
    }
    for i in 5..12 {
        world.actors[i].active = false;
        world.actors[i].battle.hp = 0;
    }
    world.casting.summon_actor_slot = Some(9);
    world.actors[9].active = true;
    world.actors[9].battle.hp = 100;
    world.actors[0].battle.active_target = 1;
    let mut hit: Vec<u8> = Vec::new();
    let mut froze = false;
    for _ in 0..4000 {
        let run = world.run_cast_module_code(0x8A, 0).expect("PROT 0912");
        hit.extend(run.aoe_hits.iter().map(|h| h.seat));
        froze |= (1..5).all(|i| world.actors[i].battle.anim_rate.get() == 0);
        if !run.busy {
            break;
        }
    }
    hit.sort_unstable();
    hit.dedup();
    assert_eq!(
        hit,
        vec![1, 2, 3, 4],
        "Freed's sweep reached the engine monsters"
    );
    assert!(froze, "arm 0x10 froze every engine monster");
    assert_eq!(
        world.actors[0].battle.anim_rate.get(),
        8,
        "the caster is untouched"
    );
}

/// A full party's seat row is the engine's table unchanged: the kernels see
/// the same views, so the map adds nothing for a party of three.
#[test]
fn a_full_partys_seat_row_is_the_engine_table() {
    let mut world = module_code_world();
    world.party.party_count = 3;
    world.actors[3].battle.active_target = 5;
    let row = world.cast_seat_row();
    assert_eq!(row.len(), world.actors.len());
    for (i, st) in row.iter().enumerate() {
        assert_eq!(*st, world.cast_view(i as u8));
        assert_eq!(st.target_code, world.actors[i].battle.active_target);
    }
}

/// A lone member's row inserts retail's two zeroed empty seats and shifts
/// the monster row (and its target codes) up to `3`; writing the row back
/// drops the empty seats' writes and restores engine target codes.
#[test]
fn a_lone_members_seat_row_round_trips() {
    let mut world = module_code_world();
    world.actors[0].battle.active_target = 2; // engine monster 2 = retail 4
    world.actors[2].battle.active_target = 8; // the party-wide group code
    let row = world.cast_seat_row();
    assert_eq!(row.len(), world.actors.len() + 2);
    assert_eq!(row[1], Default::default());
    assert_eq!(row[2], Default::default());
    assert_eq!(row[0].target_code, 4);
    assert_eq!(row[4].hp, world.actors[2].battle.hp);
    assert_eq!(row[4].target_code, 8);
    let mut edited = row.clone();
    edited[1].hp = 7; // an empty seat: dropped
    edited[3].target_code = 0; // engine monster 1 retargets the member
    world.write_cast_seat_row(&edited);
    assert_eq!(world.actors[0].battle.active_target, 2);
    assert_eq!(world.actors[1].battle.active_target, 0);
    assert_eq!(world.actors[2].battle.active_target, 8);
    assert_eq!(world.actors[1].battle.hp, 100);
}

/// The tick seam the action SM already drives: arming the stager zeroes the
/// module phase (retail's `0x801E4B1C`) and each tick re-enters the module.
#[test]
fn arming_the_stager_zeroes_the_module_phase() {
    let mut world = module_code_world();
    world.casting.module_phase = 9;
    world.casting.module_ctx_278 = 7;
    world.arm_summon_stager(0, 0x87);
    assert_eq!(world.casting.module_phase, 0);
    assert_eq!(world.casting.module_ctx_278, 0);
    world.casting.summon_actor_slot = Some(7);
    assert!(
        world.summon_stager_tick(),
        "the stager is busy from tick one"
    );
    assert_eq!(
        world.casting.module_phase, 1,
        "the stager tick re-entered PROT 0909's module code"
    );
}

/// PROT 0912's arm 4 turns the four crystal triples arm 2 seated in round
/// the creature on its held passes - the first handle of each corner takes
/// the new spot and the other two copy its trio - and kills all twelve on
/// its pass (`0x801F7184..0x801F730C`, `0x801F7158..0x801F7174`).
#[test]
fn freed_arm_4_turns_then_kills_the_crystals() {
    use legaia_engine_cast::cast_module_camera as cam;
    let mut world = module_code_world();
    world.casting.summon_actor_slot = Some(9);
    world.actors[9].active = true;
    world.actors[9].move_state.world_x = 100;
    world.actors[9].move_state.world_z = -800;
    let mut scene = legaia_engine_effects::summon::SummonScene::spawn_parts(&[], &[], 0, [0; 3]);
    for _corner in 0..4 {
        for record in cam::FREED_CRYSTAL_RECORDS {
            scene
                .parts
                .push(legaia_engine_effects::summon::SummonPartRuntime {
                    model_sel: -1,
                    reserved: 0,
                    buf: vec![0x08],
                    state: Default::default(),
                    finished: false,
                    origin: Some([0; 3]),
                    tag: Some(record),
                    seat_pokes: Vec::new(),
                    seat_halve_speed: false,
                });
        }
    }
    world.casting.active_summon = Some(scene);
    // Arm 3's pass zeroes the turn word; arm 4's first held pass steps it.
    world.casting.module_freed_turn = 99;
    world.freed_body_effects(3, 4, 0);
    assert_eq!(world.casting.module_freed_turn, 0);
    world.casting.module_settle_countdown = (8 << 6) - 8;
    world.freed_body_effects(4, 4, 0);
    assert_eq!(
        world.casting.module_freed_turn,
        -cam::FREED_CRYSTAL_TURN_STEP
    );
    let corners = cam::freed_crystal_corners(-cam::FREED_CRYSTAL_TURN_STEP, (8 << 6) - 8);
    let parts = &world.casting.active_summon.as_ref().unwrap().parts;
    for (k, &(dx, dz)) in corners.iter().enumerate() {
        for j in 0..3 {
            let s = &parts[3 * k + j].state;
            assert_eq!((s.world_x, s.world_z), (100 + dx, -800 + dz), "corner {k}");
        }
    }
    world.freed_body_effects(4, 5, 0);
    let scene = world.casting.active_summon.as_ref().unwrap();
    assert!(scene.parts[..12].iter().all(|p| p.finished));
    // The pass also seats arm 4's fifteen-part burst; with no pool installed
    // there is nothing to seat.
    assert_eq!(scene.parts.len(), 12);
}
