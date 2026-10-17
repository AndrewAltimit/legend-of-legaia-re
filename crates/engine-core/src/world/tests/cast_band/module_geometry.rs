//! Module context seeding and the geometric reaches: the ring cone, Theeder's
//! ray and Swordie's slashes.

use super::*;

/// `ctx[+0]` is the **party** count, the bound of every `0..ctx[+0]` sweep in
/// the band - Evil Seru Magic's whole-row hit (`FUN_801F8D64`), Orb's heal,
/// Element Change's hide. Seeding it from the actor table made all three run
/// over the monster row as well, which retail's separate `ctx[+1]` sweep is
/// what covers.
#[test]
fn the_module_context_takes_ctx0_from_the_party_row() {
    use legaia_engine_cast::cast_module_ticks::FIRST_MONSTER_SEAT;
    let mut world = module_code_world();
    // Eight seated combat actors, three of them party; `ctx[+1]` counts the
    // seats a monster sits in.
    for i in 1..8 {
        world.actors[i].battle_monster_id = Some(1);
    }
    world.party.party_count = 3;
    let ctx = world.cast_module_ctx();
    assert_eq!(ctx.party_count, 3, "ctx[+0] followed the actor table");
    assert!(ctx.party_count <= FIRST_MONSTER_SEAT);
    assert_eq!(
        ctx.monster_count, 5,
        "ctx[+1] still counts the live monster row"
    );
    // A lone party member narrows `ctx[+0]` and leaves `ctx[+1]` alone.
    world.party.party_count = 1;
    let ctx = world.cast_module_ctx();
    assert_eq!(ctx.party_count, 1);
    assert_eq!(ctx.monster_count, 5);
}

/// PROT 0904's ring sweep hits a seat when the bearing difference is within
/// `+-0x30` **modulo a turn** - retail's `(|ref - seat| - 0x30)` compared
/// unsigned against `0xFB1`, whose underflow is what makes the near end of
/// the cone count. A cone that only tested one side would drop every seat on
/// the wrapping edge.
#[test]
fn the_ring_cone_wraps_at_both_ends() {
    use legaia_engine_cast::seru_magic_ticks::THEEDER_CONE_HALF_WIDTH;
    let mut world = module_code_world();
    // Seat three monsters around a centre at the origin: due +Z (bearing 0),
    // just inside the cone's far edge, and well outside it.
    let centre = (0i16, 0i16);
    let place = |w: &mut World, slot: usize, x: i16, z: i16| {
        w.actors[slot].battle.seat = Some((x, z));
    };
    place(&mut world, 3, 0, 1000); // bearing 0x000
    place(&mut world, 4, 1000, 0); // bearing 0x400 - a quarter turn out
    place(&mut world, 5, 0, -1000); // bearing 0x800 - half a turn out
    // Seat 6 is in the swept range too; park it off the ray rather than at
    // the centre, where a zero displacement would read as bearing 0 and sit
    // inside every cone.
    place(&mut world, 6, -1000, 0); // bearing 0xC00
    // A ray pointing at seat 3 takes it and nothing else.
    let hit = world.seats_in_cone(centre, 0x000, THEEDER_CONE_HALF_WIDTH, 3..7);
    assert_eq!(hit, vec![3]);
    // The same ray one unit the OTHER side of the wrap still takes seat 3:
    // `0xFFF` is one step below a full turn, i.e. one step before bearing 0.
    let hit = world.seats_in_cone(centre, 0x0FFF, THEEDER_CONE_HALF_WIDTH, 3..7);
    assert_eq!(hit, vec![3], "the cone did not wrap at 0x1000");
    // A ray a whole quarter turn away takes seat 4 instead.
    let hit = world.seats_in_cone(centre, 0x0400, THEEDER_CONE_HALF_WIDTH, 3..7);
    assert_eq!(hit, vec![4]);
    // A ray between the seats takes nobody.
    let hit = world.seats_in_cone(centre, 0x0200, THEEDER_CONE_HALF_WIDTH, 3..7);
    assert!(hit.is_empty(), "the cone reached {hit:?} from a gap");
}

/// PROT 0904's ray swings `+-0x100` about the summon's facing, not a full
/// circle (`0x801F7B90..0x801F7C80`): over one sweep it reaches a seat
/// straight ahead and never one behind the summon or off to its side.
#[test]
fn the_theeder_ray_only_reaches_the_arc_ahead_of_the_summon() {
    use legaia_engine_cast::seru_magic_ticks::THEEDER_CONE_HALF_WIDTH;
    let mut world = module_code_world();
    // Summon at the origin facing +Z (bearing 0).
    world.actors[7].move_state.world_x = 0;
    world.actors[7].move_state.world_z = 0;
    world.actors[7].battle.facing_angle = 0;
    let place = |w: &mut World, slot: usize, x: i16, z: i16| {
        w.actors[slot].battle.seat = Some((x, z));
    };
    place(&mut world, 3, 0, 2000); // dead ahead
    place(&mut world, 4, 0, -2000); // behind
    place(&mut world, 5, 2000, 0); // a quarter turn off
    let mut ever = std::collections::BTreeSet::new();
    for c in (0u16..0x1000).step_by(8) {
        let (mouth, tip) = world.theeder_ray(7, c);
        let bearing = legaia_engine_battle_vm::battle_action::bearing_12bit_approx(
            mouth[2], mouth[0], tip[2], tip[0],
        );
        let d = bearing.wrapping_add(0x100) & 0xFFF;
        assert!(
            d <= 0x200,
            "ray bearing {bearing:#x} left the +-0x100 swing"
        );
        ever.extend(world.seats_in_cone(
            (mouth[0], mouth[2]),
            bearing,
            THEEDER_CONE_HALF_WIDTH,
            3..6,
        ));
    }
    assert_eq!(ever.into_iter().collect::<Vec<_>>(), vec![3]);
}

/// PROT 0910 (Swordie, spell `0x88`) lands its four slashes from the cast band
/// seam: the module's own timers schedule them and each landing stages the
/// victim's reaction clip, while the HP outcome stays the fold's (a neutral
/// wrapper return, so no seat's HP moves here).
#[test]
fn the_swordie_slashes_land_from_the_cast_band_seam() {
    let mut world = module_code_world();
    assert_eq!(world.cast_module_for(0x88), Some(910));
    world.casting.summon_actor_slot = Some(7);
    world.actors[0].battle.active_target = 3;
    world.casting.module_phase = 8;
    // The victim's alternate reaction clip (`+0x1EF`).
    world.actors[3].battle.params[0x1EF - 0x1DF] = 0x21;
    let hp_before: Vec<u16> = world.actors.iter().map(|a| a.battle.hp).collect();
    let mut hits = 0;
    let mut done = false;
    for _ in 0..2000 {
        let run = world.run_cast_module_code(0x88, 0).expect("band entry");
        hits += run.aoe_hits.len();
        if !run.busy {
            done = true;
            break;
        }
    }
    assert!(done, "the settle arm finishes the cast");
    assert_eq!(hits, 4, "one landing per slash");
    assert_eq!(world.casting.module_swordie.hits, 4);
    let hp_after: Vec<u16> = world.actors.iter().map(|a| a.battle.hp).collect();
    assert_eq!(hp_before, hp_after, "the fold owns the HP outcome");
    assert_eq!(
        world.actors[3].battle.queued_anim, 0x21,
        "a live victim takes the alternate reaction on every landing"
    );
}
