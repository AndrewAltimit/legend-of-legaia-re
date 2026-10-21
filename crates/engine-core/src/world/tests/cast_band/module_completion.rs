//! Every module body runs to completion: chain bodies, capture ticks, summon
//! stagers, and the timed bands.

use super::*;

/// Every gated capture-class chain body finishes: its countdown table never
/// holds an arm for good.
#[test]
fn every_gated_chain_body_finishes() {
    use legaia_engine_cast::cast_module_ticks::{capture_trampoline_for, chain_bodies};
    use legaia_game_tables::spell_names::{CAPTURE_CLASS, SpellEntry, SpellNameTable};
    let mut stuck = Vec::new();
    for b in chain_bodies().iter().filter(|b| b.prot_entry >= 935) {
        let sub = (b.prot_entry - 935) as u8;
        let ids: Vec<u8> = if capture_trampoline_for(b.prot_entry).is_some() {
            b.action_ids.to_vec()
        } else {
            vec![0xF0]
        };
        for id in ids {
            let mut world = module_code_world();
            world.party.party_count = 3;
            world.battle_ctx.active_actor = 3;
            world.actors[3].battle_monster_id = Some(180);
            world.actors[3].battle.active_target = 0;
            let mut entries = vec![SpellEntry::default(); 0x100];
            entries[id as usize].class = CAPTURE_CLASS;
            entries[id as usize].sub_class = sub;
            world.menu.text = Some(legaia_engine_menus::pause_screens::MenuTextTables {
                spell_names: Some(SpellNameTable::from_entries(entries)),
                ..Default::default()
            });
            if world.cast_module_for(id) != Some(b.prot_entry) {
                continue;
            }
            world.arm_capture_cast_module(id);
            let mut done = false;
            for _ in 0..20_000 {
                let phase = world.casting.module_phase;
                match world.run_cast_module_code(id, phase) {
                    Some(r) if r.busy => {}
                    _ => {
                        done = true;
                        break;
                    }
                }
            }
            if !done {
                stuck.push((b.prot_entry, id, world.casting.module_phase));
            }
        }
    }
    assert!(stuck.is_empty(), "never finished: {stuck:x?}");
}

/// A module that seats its creature in a later arm (`module_seat_arm`) has
/// the seat requested as the module reaches that arm, never on the
/// stager's first tick, and always before the stager lets go.
#[test]
fn a_module_seats_its_creature_in_its_seat_arm() {
    let mut checked = 0usize;
    for id in 0x81u8..=0xA0 {
        let mut world = module_code_world();
        world.party.party_count = 3;
        let Some(entry) = world.cast_module_for(id) else {
            continue;
        };
        let Some(arm) =
            legaia_engine_cast::cast_module_camera::module_profile(entry).and_then(|p| p.seat_arm)
        else {
            continue;
        };
        world.battle_ctx.active_actor = 0;
        world.actors[0].battle.active_target = 3;
        for i in 0..8 {
            world.actors[i].battle.hp = 9000;
            world.actors[i].battle.max_hp = 9000;
        }
        world.arm_summon_stager(0, id);
        let mut seated_at = None;
        for t in 0..20_000 {
            let busy = world.summon_stager_tick();
            if seated_at.is_none() && world.take_pending_summon_spawn().is_some() {
                seated_at = Some((t, world.casting.module_phase));
            }
            if !busy {
                break;
            }
        }
        let Some((t, phase)) = seated_at else {
            panic!("PROT {entry:04} id {id:#04X}: the creature was never seated");
        };
        assert!(t > 0, "PROT {entry:04}: seated on the stager's first tick");
        assert!(
            phase >= arm,
            "PROT {entry:04}: seated at phase {phase} before its seat arm {arm}"
        );
        checked += 1;
    }
    assert!(checked >= 10, "only {checked} modules carry a seat arm");
}

/// Every capture-class tick body (PROT 0935..=0966, each trampoline action
/// id) reaches its end from phase 0: no body's phase walk wraps the byte or
/// parks for good. PROT 0958's table once bounded the whole phase byte and
/// spun through the wrap, holding battle state `0x70` forever.
#[test]
fn every_capture_tick_body_finishes() {
    use legaia_engine_cast::cast_module_ticks::capture_trampoline_for;
    use legaia_game_tables::spell_names::{CAPTURE_CLASS, SpellEntry, SpellNameTable};
    let mut stuck = Vec::new();
    let mut checked = 0usize;
    for entry in 935u32..=966 {
        let sub = (entry - 935) as u8;
        let ids: Vec<u8> = match capture_trampoline_for(entry) {
            Some(t) => t.arms.iter().map(|(id, _)| *id).collect(),
            None => vec![0xF0],
        };
        for id in ids {
            let mut world = module_code_world();
            world.party.party_count = 3;
            world.battle_ctx.active_actor = 3;
            world.actors[3].battle_monster_id = Some(180);
            world.actors[3].battle.active_target = 0;
            for i in 0..8 {
                world.actors[i].battle.hp = 9000;
                world.actors[i].battle.max_hp = 9000;
            }
            let mut entries = vec![SpellEntry::default(); 0x100];
            entries[id as usize].class = CAPTURE_CLASS;
            entries[id as usize].sub_class = sub;
            world.menu.text = Some(legaia_engine_menus::pause_screens::MenuTextTables {
                spell_names: Some(SpellNameTable::from_entries(entries)),
                ..Default::default()
            });
            if world.cast_module_for(id) != Some(entry) {
                continue;
            }
            world.arm_capture_cast_module(id);
            let mut done = false;
            for _ in 0..40_000 {
                let phase = world.casting.module_phase;
                match world.run_cast_module_code(id, phase) {
                    Some(r) if r.busy => {}
                    _ => {
                        done = true;
                        break;
                    }
                }
                // The clip commit this world has no animation half for:
                // a staged byte becomes the playing one (a confirm gate
                // such as PROT 0960's arm 5 waits on exactly that).
                for a in world.actors.iter_mut() {
                    a.battle.current_anim = a.battle.queued_anim;
                }
            }
            if !done {
                stuck.push((entry, id, world.casting.module_phase));
            }
            checked += 1;
        }
    }
    assert!(
        checked >= 32,
        "only {checked} capture casts reached a module"
    );
    assert!(stuck.is_empty(), "never finished: {stuck:x?}");
}

/// The same for the summon band (PROT 0903..=0934): its gate is the
/// stager's return (`0x36` holds on `FUN_801F1ED4`), so every summon id's
/// stager lets go. A camera-only director parks its module phase on the
/// first arm it does not cover and the stager's own choreography ends the
/// band, so the module's phase alone is not the measure.
#[test]
fn every_summon_stager_lets_go() {
    let mut stuck = Vec::new();
    let mut checked = 0usize;
    for id in 0x81u8..=0xFF {
        let mut world = module_code_world();
        world.party.party_count = 3;
        let Some(entry) = world.cast_module_for(id) else {
            continue;
        };
        if !(903..=934).contains(&entry) {
            continue;
        }
        world.battle_ctx.active_actor = 0;
        world.actors[0].battle.active_target = 3;
        for i in 0..8 {
            world.actors[i].battle.hp = 9000;
            world.actors[i].battle.max_hp = 9000;
        }
        world.arm_summon_stager(0, id);
        let mut done = false;
        for _ in 0..40_000 {
            if !world.summon_stager_tick() {
                done = true;
                break;
            }
            let _ = world.take_pending_summon_spawn();
            for a in world.actors.iter_mut() {
                a.battle.current_anim = a.battle.queued_anim;
            }
        }
        if !done {
            stuck.push((entry, id, world.casting.module_phase));
        }
        checked += 1;
    }
    assert!(
        checked >= 20,
        "only {checked} summon casts reached a module"
    );
    assert!(stuck.is_empty(), "never let go: {stuck:x?}");
}

/// PROT 0905 (Vera) runs its own fades beside the band's flash: arm 2's
/// white rise, killed by arm 4, which spawns its warm and blue pair. Both
/// hosts draw them through `screen_fade_draws`.
#[test]
fn vera_runs_its_fades_beside_the_band_flash() {
    let mut world = module_code_world();
    world.party.party_count = 3;
    world.battle_ctx.active_actor = 0;
    world.actors[0].battle.active_target = 1;
    for i in 0..8 {
        world.actors[i].battle.hp = 9000;
        world.actors[i].battle.max_hp = 9000;
    }
    world.presentation.fade = Some(legaia_engine_system::fade::FadeState::load(
        &legaia_engine_battle_vm::battle_action::summon_fade_template(
            &legaia_engine_battle_vm::battle_action::SUMMON_FLASH_OUT,
        ),
    ));
    world.arm_summon_stager(0, 0x83);
    let mut rise = false;
    let mut pair = false;
    for _ in 0..4000 {
        world.summon_stager_tick();
        let _ = world.take_pending_summon_spawn();
        let fades = &world.presentation.module_fades;
        if fades.len() == 1 && fades[0].target_rgb() == [0xFF; 3] {
            rise = true;
        }
        if fades.len() == 2 {
            pair = true;
            break;
        }
    }
    assert!(rise, "arm 2 spawns the white rise");
    assert!(pair, "arm 4 replaces it with its pair");
    assert!(
        world.screen_fade_draws().len() >= 2,
        "drawn beside the band flash"
    );
}

/// PROT 0910's wind-up (arm 7) drains `speed << 8` by `rate * speed` a
/// battle pass, so it lasts 256 vsyncs whatever the cadence - 256 engine
/// ticks, one vsync each, at any frame step - and its exit sets the summon's
/// animation rate to the speed scalar, the battle's normal rate.
#[test]
fn the_swordie_wind_up_lasts_256_vsync_ticks_and_restores_the_normal_rate() {
    let mut world = module_code_world();
    world.clock.frame_step = 2;
    world.casting.summon_actor_slot = Some(7);
    world.actors[0].battle.active_target = 3;
    world.casting.module_phase = 6;
    world.run_cast_module_code(0x88, 0).expect("band entry");
    assert_eq!(world.casting.module_phase, 7);
    let mut ticks = 0;
    while world.casting.module_phase == 7 {
        world.run_cast_module_code(0x88, 0).expect("band entry");
        ticks += 1;
        assert!(ticks < 1000, "the wind-up never ended");
    }
    assert_eq!(
        ticks, 257,
        "2048 drained 8 a tick holds 256 ticks, the 257th advances"
    );
    assert_eq!(
        world.actors[7].battle.anim_rate.get(),
        legaia_engine_battle_vm::battle_anim_rate::RATE_NORMAL
    );
}

/// PROT 0904 (Theeder, `0x82`) paces its band: the stager holds `0x36` on the
/// module, so the band stays up through arm 11's 64-tick ramp and arm 12's
/// 64-tick sweep, and lets go only after the ray has turned a full circle.
#[test]
fn the_theeder_band_holds_through_its_ramp_and_sweep() {
    use legaia_engine_cast::seru_magic_ticks::{THEEDER_RETARGET_ARM, THEEDER_SWEEP_ARM};
    let mut world = module_code_world();
    world.party.party_count = 3;
    assert_eq!(world.cast_module_for(0x82), Some(904));
    world.battle_ctx.active_actor = 0;
    world.actors[0].battle.active_target = 3;
    for i in 0..8 {
        world.actors[i].battle.hp = 9000;
        world.actors[i].battle.max_hp = 9000;
    }
    world.arm_summon_stager(0, 0x82);
    let (mut retarget_ticks, mut sweep_ticks) = (0, 0);
    let mut done = false;
    // Which packet kinds the module's draw side emitted, in first-seen order.
    let mut kinds: Vec<&str> = Vec::new();
    let mut seat_at: Option<(i16, i16)> = None;
    for _ in 0..40_000 {
        match world.casting.module_phase {
            THEEDER_RETARGET_ARM => retarget_ticks += 1,
            THEEDER_SWEEP_ARM => sweep_ticks += 1,
            _ => {}
        }
        if !world.summon_stager_tick() {
            done = true;
            break;
        }
        use legaia_render_kernels::cast_theeder_packet::TheederPacket as P;
        let kind = match world.theeder_draw() {
            Some(P::Prongs { .. }) => Some("prongs"),
            Some(P::Charge { .. }) => Some("charge"),
            Some(P::Sweep { .. }) => Some("sweep"),
            Some(P::Retract { .. }) => Some("retract"),
            None => None,
        };
        if let Some(k) = kind
            && !kinds.contains(&k)
        {
            kinds.push(k);
        }
        // Arm 4 seats the creature once; no later arm moves it.
        if (5..=THEEDER_SWEEP_ARM).contains(&world.casting.module_phase)
            && let Some(slot) = world.casting.summon_actor_slot
        {
            let a = &world.actors[slot as usize];
            let at = (a.move_state.world_x, a.move_state.world_z);
            assert_eq!(*seat_at.get_or_insert(at), at, "the Theeder seat moved");
        }
        if world.take_pending_summon_spawn().is_some() {
            world.seat_summon_actor(7);
        }
        for a in world.actors.iter_mut() {
            a.battle.current_anim = a.battle.queued_anim;
        }
    }
    assert!(done, "the stager let go");
    let want = legaia_engine_cast::seru_magic_ticks::theeder_seat_placement(
        (
            world.actors[3].move_state.world_x,
            world.actors[3].move_state.world_z,
        ),
        (
            world.actors[0].move_state.world_x,
            world.actors[0].move_state.world_z,
        ),
    );
    assert_eq!(seat_at, Some((want.0, want.1)), "arm 4's placement");
    assert!(retarget_ticks >= 64, "arm 11 held {retarget_ticks} ticks");
    assert!(sweep_ticks >= 64, "arm 12 held {sweep_ticks} ticks");
    assert!(world.casting.module_phase > THEEDER_SWEEP_ARM);
    assert_eq!(kinds, vec!["prongs", "charge", "sweep", "retract"]);
    assert!(
        world.theeder_draw().is_none(),
        "nothing draws past the band"
    );
}

/// PROT 0920 (Slippery, `0x92`) paces its band through its director: the
/// stager holds until the chain reaches `0xFF`, the rain budget is set from
/// arm 5's exit to the done arm, the routine draws streaks while it is set,
/// the colour has turned red by the time the rain ends, and nothing draws
/// once the band lets go.
#[test]
fn the_slippery_band_rains_from_arm_five_to_the_done_arm() {
    let mut world = module_code_world();
    world.party.party_count = 3;
    assert_eq!(world.cast_module_for(0x92), Some(920));
    world.actors[0].battle.active_target = 3;
    for i in 0..8 {
        world.actors[i].battle.hp = 9000;
        world.actors[i].battle.max_hp = 9000;
    }
    world.arm_summon_stager(0, 0x92);
    let (mut rain_ticks, mut drawn_ticks, mut first_rain_phase) = (0u32, 0u32, None);
    let mut red = false;
    let mut done = false;
    for _ in 0..20_000 {
        if !world.summon_stager_tick() {
            done = true;
            break;
        }
        if world.casting.module_rain.budget != 0 {
            rain_ticks += 1;
            first_rain_phase.get_or_insert(world.casting.module_phase);
            red |= world.casting.module_rain.colour == 0x00FF_4040;
        }
        if !world.module_rain_lines().is_empty() {
            drawn_ticks += 1;
        }
        if world.take_pending_summon_spawn().is_some() {
            world.seat_summon_actor(7);
        }
    }
    assert!(done, "the stager let go");
    assert_eq!(
        first_rain_phase,
        Some(6),
        "the budget is seeded on arm 5's exit"
    );
    // Arms 6..10 at the engine's step: 256 + 256 + 32 + 128 + 128 ticks.
    assert!(rain_ticks >= 800, "rain lasted {rain_ticks} ticks");
    assert!(drawn_ticks > 0, "the routine drew streaks");
    assert!(red, "arm 9 pinned the colour red");
    assert!(world.module_rain_lines().is_empty());
}

/// PROT 0910 (Swordie, `0x88`) draws its split panels from arm 0 and its
/// streak in arm 3, and nothing once the band has let go.
#[test]
fn the_swordie_band_draws_its_split_and_streak() {
    let mut world = module_code_world();
    world.party.party_count = 3;
    world.actors[0].battle.active_target = 3;
    world.arm_summon_stager(0, 0x88);
    let (mut split, mut streak) = (0u32, 0u32);
    for _ in 0..20_000 {
        if !world.summon_stager_tick() {
            break;
        }
        if let Some(d) = world.swordie_draw() {
            split += u32::from(d.split.is_some());
            streak += u32::from(d.streak.is_some());
        }
        if world.take_pending_summon_spawn().is_some() {
            world.seat_summon_actor(7);
        }
        for a in world.actors.iter_mut() {
            a.battle.current_anim = a.battle.queued_anim;
        }
    }
    // Arm 3: 0x40 / 2 a tick = 32 ticks; arm 4: 65 ticks; arms 0..2 one each.
    assert_eq!(streak, 32);
    assert_eq!(split, 3 + 32 + 65);
    assert!(world.swordie_draw().is_none());
}
