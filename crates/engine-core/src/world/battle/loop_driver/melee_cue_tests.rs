use super::*;

/// A battle with one party member and one monster, both alive.
fn duel() -> World {
    let mut w = World::new();
    w.enter_battle(1, 1);
    for i in 0..2 {
        w.actors[i].battle.liveness = 1;
        w.actors[i].battle.hp = 500;
        w.actors[i].battle.max_hp = 500;
    }
    w.set_battle_attack(0, 80);
    w.set_battle_attack(1, 80);
    w.actors[0].battle.active_target = 1;
    w.actors[1].battle.active_target = 0;
    w
}

/// A combo total a cast clip accumulated is landed as the capture band
/// leaves `0x71` for `0x50`, so the `0x51` settle gate has a written HP
/// to ramp the bar to - PROT 0955's Terror Scream left Gala's total
/// stranded and parked the band.
#[test]
fn a_cast_clip_total_lands_on_the_capture_band_exit() {
    use vm::battle_action::ActionState;
    let mut w = duel();
    w.battle_ctx.active_actor = 1;
    w.battle_ctx.action_state = ActionState::MagicCaptureFinalize.as_byte();
    for a in w.actors.iter_mut() {
        a.battle.current_anim = 0;
    }
    w.actors[0].battle.arm_hp_bar();
    w.actors[0].battle.damage_accum = 97;
    w.actors[0].battle.accumulate_hp_bar(97);
    let mut reached = false;
    for _ in 0..8 {
        if let Some(StepOutcome::Transition { to, .. }) = w.live_battle_tick()
            && to == ActionState::DoneCleanup.as_byte()
        {
            reached = true;
            break;
        }
    }
    assert!(reached, "the capture band exits into 0x50");
    assert_eq!(w.actors[0].battle.damage_accum, 0);
    assert_eq!(w.actors[0].battle.hp, 500 - 97, "the total is landed");
}

/// A Seru-carrying duel whose monster dies to one basic swing, with a
/// certain absorb roll and Vahn's Ra-Seru marker set.
fn absorb_duel() -> World {
    let mut w = duel();
    w.load_party(legaia_save::Party::zeroed(1));
    w.party.party_count = 1;
    let rec = &mut w.party.roster.members[0];
    let mut eq = rec.equipment();
    eq.slots[3] = 0x30;
    rec.set_equipment(eq);
    let mut def = crate::monster_catalog::MonsterDef::new(0x40, "Seru", 1, 10);
    def.absorb_seru = 1;
    def.absorb_chance_pct = 100;
    w.tables.monster_catalog.insert(def);
    w.actors[1].battle_monster_id = Some(0x40);
    w.actors[1].battle.hp = 1;
    w.battle_ctx.active_actor = 0;
    w
}

/// Retail's kill compare sits behind the apply gate
/// (`0x801EE128..0x801EE1A4`): a hit that empties the target's HP
/// mid-chain does not roll the absorb.
#[test]
fn a_killing_hit_off_the_apply_gate_rolls_no_absorb() {
    let mut w = absorb_duel();
    w.land_melee_hit(0, 1, BASIC_ATTACK_COMMAND, 0, false, false);
    assert!(w.actors[1].battle.damage_accum >= 1, "the swing connected");
    assert_eq!(w.battle_ctx.absorbed_seru, 0);
}

/// The hit that lands the total takes the compare - the accumulated
/// total against live HP, however early in the chain it crossed.
#[test]
fn the_hit_that_lands_the_total_rolls_the_absorb() {
    let mut w = absorb_duel();
    w.land_melee_hit(0, 1, BASIC_ATTACK_COMMAND, 0, false, false);
    w.land_melee_hit(0, 1, BASIC_ATTACK_COMMAND, 0, false, true);
    assert_eq!(w.battle_ctx.absorbed_seru, 1, "Seru 1 staged");
}

/// Seat monster clips on slot 1: idle (tag 0), high flinch (tag 2) at
/// entry 1, knockdown (tag 4) at entry 2, and - with `getup` - a get-up
/// (tag 5) at entry 3.
fn seat_reaction_clips(w: &mut World, getup: bool) {
    use legaia_asset::monster_archive::{MonsterAnimation, PartPose};
    let clip = |action_id: u8| MonsterAnimation {
        action_id,
        rate: 2,
        attach_key: 0,
        solo_flag: 0,
        impact_class: 0,
        effect_script: Vec::new(),
        part_count: 1,
        frame_count: 2,
        frames: vec![vec![PartPose::default()]; 2],
    };
    let mut clips = vec![Some(clip(0)), Some(clip(2)), Some(clip(4))];
    if getup {
        clips.push(Some(clip(5)));
    }
    w.set_actor_battle_action_clips(1, std::sync::Arc::new(clips));
}

/// `0x801EE350`: a killing total with a Seru staged in `ctx[+0x269]`
/// skips the knockdown load. A monster with no get-up entry keeps the
/// flinch; one with a get-up loads the knockdown inside the absorb block
/// (`0x801EE2F4..0x801EE304`); a kill with nothing staged knocks down.
#[test]
fn an_absorbing_kill_flinches_a_monster_without_a_get_up() {
    const UDF: u8 = 0x16;
    for (getup, absorbed, want) in [
        (false, true, 1u8),
        (true, true, 2),
        (false, false, 2),
        (true, false, 2),
    ] {
        let mut w = absorb_duel();
        seat_reaction_clips(&mut w, getup);
        w.actors[1].battle.damage_accum = 5;
        if absorbed {
            w.battle_ctx.absorbed_seru = 1;
        }
        assert_eq!(
            w.melee_reaction_entry(0, 1, UDF, true, absorbed),
            Some(want),
            "getup {getup} absorbed {absorbed}"
        );
    }
    // End to end: the hit that lands the total rolls a certain absorb
    // and the get-up-less monster commits its flinch.
    let mut w = absorb_duel();
    seat_reaction_clips(&mut w, false);
    w.land_melee_hit(0, 1, BASIC_ATTACK_COMMAND, 0, false, true);
    assert_eq!(w.battle_ctx.absorbed_seru, 1, "Seru 1 staged");
    assert_eq!(w.actors[1].battle_reaction_entry, Some(1), "flinch");
}

/// The War God Icon carry (apply mode `0xFF`) branches from `0x801EE12C`
/// to `0x801EE3B8`, one instruction past the knockdown load at
/// `0x801EE3B4`: the carried hit keeps its flinch even on a lethal
/// total. Its callers pass `kill_check = false`.
#[test]
fn the_war_god_carry_keeps_the_flinch() {
    let mut w = absorb_duel();
    seat_reaction_clips(&mut w, true);
    w.actors[1].battle.damage_accum = 5;
    assert_eq!(w.melee_reaction_entry(0, 1, 0x16, false, false), Some(1));
    // The same lethal total on the kill compare knocks down.
    assert_eq!(w.melee_reaction_entry(0, 1, 0x16, true, false), Some(2));
}

/// The damage roll reads the approach pair before the hit path zeroes it
/// (`lh 0x6d4` at `0x801ED1E0`, `sh zero` at `0x801EE3C8`): a long
/// walk-in adds `(DEF * term) >> 10` to the guard, so on the same RNG
/// stream the opening hit lands softer - and the next hit pays nothing.
#[test]
fn the_opening_hit_pays_the_approach_terms() {
    let hit = |atk: u16, guard_ramp: i16, attack_ramp: i16| {
        let mut w = duel();
        w.set_battle_attack(0, atk);
        w.set_battle_defense(1, 60);
        w.rng_state = 0x1234_5679;
        w.battle.guard_ramp = guard_ramp;
        w.battle.attack_ramp = attack_ramp;
        w.land_melee_hit(0, 1, BASIC_ATTACK_COMMAND, 0, false, false);
        assert_eq!(
            (w.battle.attack_ramp, w.battle.guard_ramp),
            (0, 0),
            "the hit spends both terms"
        );
        w.actors[1].battle.damage_accum
    };
    let flat = hit(640, 0, 0);
    // `0x400` doubles the guard term's DEF (`60 * 0x400 >> 10 = 60`).
    let walked = hit(640, 0x400, 0);
    assert!(walked < flat, "walk-in {walked} vs flat {flat}");
    // `0x800` in the back adds `ATK * 0x800 >> 16 = ATK / 32`.
    let face_on = hit(640, 0, 0);
    let in_the_back = hit(640, 0, 0x800);
    assert!(
        in_the_back > face_on,
        "back {in_the_back} vs face-on {face_on}"
    );
}

/// State `0x14` seeds the attack-angle term as the distance from
/// face-on (`0x801E3094..0x801E30C8`): `0` when the two face each other,
/// `0x800` for a strike in the back, `0x400` from the side - never
/// negative, which is what every retail battle capture parked between
/// the seed and the first hit carries (`ctx[+0x6D2]` = 22, 212, 390).
#[test]
fn the_attack_angle_term_is_the_distance_from_face_on() {
    use vm::battle_action::{ActionState, StepOutcome};
    for (attacker, target, want) in [
        (0x000u16, 0x800u16, 0i16),
        (0x800, 0x000, 0),
        (0x100, 0x100, 0x800),
        (0x400, 0x000, 0x400),
        (0x000, 0x400, 0x400),
        (0x7F0, 0x000, 0x010),
    ] {
        let mut w = duel();
        w.battle_ctx.active_actor = 0;
        w.actors[0].battle.facing_angle = attacker;
        w.actors[1].battle.facing_angle = target;
        w.track_block_approach_terms(ActionState::AttackFace.as_byte(), &StepOutcome::Stay);
        assert_eq!(w.battle.attack_ramp, want, "{attacker:#x} vs {target:#x}");
    }
}

/// The `0x800788B8` duration table with the melee entry (`0x0C`) at its
/// retail value, `373` -> `(373 * 60 + 99) / 100 = 224` sectors.
fn durations_with_melee_entry() -> Vec<u16> {
    let mut t = vec![0u16; 0x40];
    t[0x0C] = 373;
    t
}

/// An ordinary party swing commits the defender's flinch / knockdown, not
/// its block entry, so the grunt gate (`s7 == defender[+0x1F3]`,
/// `0x801EEA88..0x801EEAA0`) skips it - the retail capture of four party
/// swings took the skip every time. The port used to grunt on every
/// strike.
#[test]
fn an_ordinary_party_swing_is_silent() {
    let mut w = duel();
    w.audio.xa_cue_durations = Some(durations_with_melee_entry());
    w.battle_ctx.active_actor = 0; // the party member attacks
    assert_eq!(
        w.battle.monster_ai_state.flag_bd84, 0,
        "the `_DAT_8007BD84` word is zero at battle start"
    );
    {
        let atk = w.battle_ctx.active_actor;
        w.land_melee_hit(atk, 1 - atk, BASIC_ATTACK_COMMAND, 0, false, true);
    }
    assert!(w.drain_battle_sfx_cues().is_empty());
    assert!(w.drain_battle_xa_cues().is_empty(), "no grunt, no sting");
}

/// A strike that commits the defender's **block** entry (`+0x1F3`, tag
/// `0x0B`) takes the grunt: `FUN_8003D53C(0x1D, 0, 0x26)` for Vahn.
#[test]
fn a_blocked_party_swing_grunts() {
    let mut w = duel();
    w.audio.xa_cue_durations = Some(durations_with_melee_entry());
    // A clip-carrying defender has a reaction map; an actor with no
    // monster id reads the hardcoded party family, block entry `0x0B`.
    w.actors[1].battle_action_clips = Some(std::sync::Arc::new(vec![None; 12]));
    w.fire_melee_impact_cue(0, 1, Some(0x02));
    assert!(w.drain_battle_xa_cues().is_empty(), "a flinch is silent");
    w.fire_melee_impact_cue(0, 1, Some(0x0B));
    assert!(w.drain_battle_sfx_cues().is_empty());
    let xa = w.drain_battle_xa_cues();
    assert_eq!(xa.len(), 1, "one block, one grunt: {xa:?}");
    assert_eq!(
        (xa[0].clip, xa[0].channel, xa[0].duration_sectors),
        (0x1D, 0, 0x26)
    );
    assert_eq!(
        w.audio.battle_xa_busy_frames, 0x26,
        "the modelled drive stays busy for the read span"
    );
}

#[test]
fn an_ordinary_monster_swing_is_silent_at_this_site() {
    let mut w = duel();
    w.battle_ctx.active_actor = 1; // the monster attacks
    {
        let atk = w.battle_ctx.active_actor;
        w.land_melee_hit(atk, 1 - atk, BASIC_ATTACK_COMMAND, 0, false, true);
    }
    // `sltiu v0,a0,0x3` at `0x801EEA7C` skips the grunt for a monster
    // seat, and the re-read of the zero word at `0x801EEB60` skips the
    // cue: a monster's ordinary swing makes no sound from this routine.
    assert!(w.drain_battle_sfx_cues().is_empty());
    assert!(w.drain_battle_xa_cues().is_empty());
}

#[test]
fn a_flagged_swing_on_a_monster_enqueues_the_runtime_row() {
    let mut w = duel();
    // Non-zero word: `bne v0,zero,0x801EEB70` at `0x801EEAC8` takes the
    // cue arm.
    w.battle.monster_ai_state.flag_bd84 = 1;
    w.battle_ctx.active_actor = 0; // the party member attacks
    w.land_melee_hit(0, 1, BASIC_ATTACK_COMMAND, 0, false, true);
    // The funnel's category is the **target's** index (`s4`): a struck
    // monster takes the high leg, `0x10C + 0x19C`, on the SFX ring.
    assert_eq!(
        w.take_sfx_ring_ops(),
        vec![crate::world::SfxRingOp::Push(0x2A8)]
    );
    assert!(
        w.drain_battle_xa_cues().is_empty(),
        "no grunt on the cue arm"
    );
}

#[test]
fn a_flagged_swing_on_a_party_member_takes_the_xa_leg() {
    let mut w = duel();
    w.battle.monster_ai_state.flag_bd84 = 1;
    w.audio.xa_cue_durations = Some(durations_with_melee_entry());
    w.battle_ctx.active_actor = 1; // the monster attacks
    w.land_melee_hit(1, 0, BASIC_ATTACK_COMMAND, 0, false, true);
    assert!(
        w.take_sfx_ring_ops().is_empty(),
        "a struck party member's `0x10C` is a CD-XA voice request, not a ring id"
    );
    let xa = w.drain_battle_xa_cues();
    assert_eq!(xa.len(), 1, "the sting, and no grunt: {xa:?}");
    // Clip `(0x0C >> 3) = 1` remapped to `26` (`XA27`), channel `0x0C & 7`.
    assert_eq!(
        (xa[0].clip, xa[0].channel, xa[0].duration_sectors),
        (26, 4, 224)
    );
}

#[test]
fn a_flagged_sting_is_dropped_while_the_drive_is_busy() {
    let mut w = duel();
    w.battle.monster_ai_state.flag_bd84 = 1;
    w.audio.xa_cue_durations = Some(durations_with_melee_entry());
    w.battle_ctx.active_actor = 1;
    // `FUN_8003DE7C(1) != 0` at `0x8004FE9C`: a read in flight drops the
    // voice leg's request.
    w.audio.battle_xa_busy_frames = 5;
    w.land_melee_hit(1, 0, BASIC_ATTACK_COMMAND, 0, false, true);
    assert!(w.take_sfx_ring_ops().is_empty());
    assert!(w.drain_battle_xa_cues().is_empty());
}

/// Seat a playing clip on `slot` whose event-frame list (`+0x10..`)
/// starts at `first_beat` - the input of the `+0x1F7` juggle window.
fn play_clip_with_beat(w: &mut World, slot: usize, first_beat: u8) {
    use legaia_asset::monster_archive::{MonsterAnimation, PartPose};
    let mut head = vec![0u8; legaia_asset::monster_archive::EFFECT_SCRIPT_HEAD_BYTES];
    head[0x10] = first_beat;
    let clip = MonsterAnimation {
        action_id: 2,
        rate: 2,
        attach_key: 0,
        solo_flag: 0,
        impact_class: 0,
        effect_script: head.clone(),
        part_count: 1,
        frame_count: 12,
        frames: vec![vec![PartPose::default()]; 12],
    };
    let a = &mut w.actors[slot];
    a.battle_animation = crate::battle_anim::MonsterAnimPlayer::new_one_shot(&clip);
    a.battle_effect_script = Some(head);
    // The anim tick's `+0x1F7` write for the seated clip.
    a.battle_juggle_window = World::juggle_window_open(a);
}

/// The limb-vs-height miss (`0x801EC488..0x801EC554`): a party hit whose
/// power byte cannot reach the target's `+0x1E` class does no damage,
/// accumulates nothing, surfaces no hit event and consumes one effect
/// record and one cue - while a byte of the reachable class, or one at
/// `0x16` and above, resolves as before.
#[test]
fn a_limb_mismatched_party_hit_misses() {
    use crate::monster_catalog::{MonsterCatalog, MonsterDef};
    let seat = |class: u8| {
        let mut w = duel();
        let mut cat = MonsterCatalog::new();
        let mut def = MonsterDef::new(7, "Floater", 500, 10);
        def.swing_class = class;
        cat.insert(def);
        w.set_monster_catalog(cat);
        w.actors[1].battle_monster_id = Some(7);
        w
    };
    let hit = |power_byte| vm::battle_action::HitEvent {
        hit_index: 0,
        power_byte,
        event_frame: 4,
    };
    let frames = [4u8, 0, 0, 0];
    for (class, pb, misses) in [
        (vm::battle_action::MISS_CLASS_LOW, 0x12u8, true),
        (vm::battle_action::MISS_CLASS_LOW, 0x0Cu8, false),
        (vm::battle_action::MISS_CLASS_HIGH, 0x0Cu8, true),
        (vm::battle_action::MISS_CLASS_HIGH, 0x12u8, false),
        (vm::battle_action::MISS_CLASS_LOW, 0x18u8, false),
        (0, 0x12u8, false),
    ] {
        let mut w = seat(class);
        w.resolve_hit_event(0, hit(pb), [pb, 0, 0, 0], frames);
        let events = std::mem::take(&mut w.battle.hit_events);
        assert_eq!(
            events.is_empty(),
            misses,
            "class {class} byte {pb:#04x}: {events:?}"
        );
        if misses {
            assert_eq!(w.actors[1].battle.damage_accum, 0);
            assert_eq!(w.actors[1].battle.hp, 500);
            assert_eq!(w.actors[0].battle_effect_cursor, 1);
            assert_eq!(w.actors[0].battle_anim_cue_cursor, 1);
            assert_eq!(w.battle_ctx.effect_skip_strobe, 0, "consumed");
        } else {
            assert_eq!(w.actors[0].battle_effect_cursor, 0);
        }
    }
    // A monster attacker never limb-misses, whatever the party seat reads.
    let mut w = seat(vm::battle_action::MISS_CLASS_LOW);
    w.resolve_hit_event(1, hit(0x12), [0x12, 0, 0, 0], frames);
    assert_eq!(w.battle.hit_events.len(), 1);
}

/// A blocked hit skips the damage body (`bne s7,zero,0x801EE6D4`): no
/// damage, no accumulation, no popup, and the attacker's anim cue cursor
/// steps over one cue. Driven through the real roll: the first seed
/// whose roll blocks is the hit under test.
#[test]
fn a_blocked_hit_lands_no_damage() {
    let mut found = false;
    let setup = |seed: u32| {
        let mut w = duel();
        w.actors[0].battle_action_clips = Some(std::sync::Arc::new(vec![None; 12]));
        w.battle.speed[0] = 200;
        w.battle.guarding[0] = true;
        w.rng_state = seed;
        w
    };
    for seed in 1..400u32 {
        if setup(seed).roll_block(1, 0, BASIC_ATTACK_COMMAND).is_none() {
            continue;
        }
        let mut w = setup(seed);
        found = true;
        let before = w.actors[0].battle.hp;
        let dmg = w.land_melee_hit(1, 0, BASIC_ATTACK_COMMAND, 0, false, true);
        assert_eq!(dmg, 0);
        assert_eq!(w.actors[0].battle.damage_accum, 0);
        assert_eq!(w.actors[0].battle.hp, before);
        assert_eq!(w.actors[1].battle_anim_cue_cursor, 1);
        assert!(w.drain_battle_hit_fx().is_empty(), "no popup for a block");
        break;
    }
    assert!(found, "some seed blocks");
    // A defender with no clips has no block entry: the hit connects.
    let mut w = duel();
    let dmg = w.land_melee_hit(1, 0, BASIC_ATTACK_COMMAND, 0, false, true);
    assert!(dmg > 0);
}

/// The `+0x1F7` window: a defender before its playing clip's first beat
/// cannot block (the juggle arm clears `s7`), and after it the block
/// pose no longer latches - a block clip's list starts at `0`, so its
/// window is never open and a block never repeats on its own.
#[test]
fn the_juggle_window_gates_the_block() {
    let mut w = duel();
    w.actors[0].battle_action_clips = Some(std::sync::Arc::new(vec![None; 12]));
    w.battle.speed[0] = 999;
    w.set_battle_attack(0, 999);
    play_clip_with_beat(&mut w, 0, 6);
    w.actors[0].battle_reaction_entry = Some(2);
    assert!(World::juggle_window_open(&w.actors[0]), "frame 0 < beat 6");
    for _ in 0..20 {
        assert_eq!(w.roll_block(1, 0, BASIC_ATTACK_COMMAND), None);
    }
    // Block pose with a zero first beat: the window is shut, so the
    // verdict is the roll's alone - it is not forced to `Some`.
    let mut w = duel();
    w.actors[0].battle_action_clips = Some(std::sync::Arc::new(vec![None; 12]));
    play_clip_with_beat(&mut w, 0, 0);
    w.actors[0].battle_reaction_entry = Some(0x0B);
    assert!(!World::juggle_window_open(&w.actors[0]));
    let blocks = (0..200)
        .filter(|_| w.roll_block(1, 0, BASIC_ATTACK_COMMAND).is_some())
        .count();
    assert!(
        blocks < 200,
        "the block pose does not latch: {blocks} / 200"
    );
}

/// The kernel reads `+0x1F7` as the anim tick last wrote it. A block
/// pose committed since that tick - by the previous hit of the same
/// combo - plays a clip whose live window is open (frame 0 < beat 6),
/// but the byte still holds the tick's value, so the pose does not
/// latch onto the next hit: the verdict stays the roll's.
#[test]
fn a_block_committed_since_the_last_tick_reads_the_ticks_window() {
    let mut w = duel();
    w.actors[0].battle_action_clips = Some(std::sync::Arc::new(vec![None; 12]));
    play_clip_with_beat(&mut w, 0, 6);
    w.actors[0].battle_reaction_entry = Some(0x0B);
    w.actors[0].battle_juggle_window = false;
    assert!(World::juggle_window_open(&w.actors[0]), "live window open");
    let blocks = (0..200)
        .filter(|_| w.roll_block(1, 0, BASIC_ATTACK_COMMAND).is_some())
        .count();
    assert!(blocks < 200, "the fresh block pose latched: {blocks} / 200");
    // With the tick's byte up as well, the pose holds.
    w.actors[0].battle_juggle_window = true;
    assert!(
        (0..20).all(|_| w.roll_block(1, 0, BASIC_ATTACK_COMMAND).is_some()),
        "a held block pose inside the window keeps blocking"
    );
}

#[test]
fn an_attacker_playing_an_art_bank_clip_is_silent() {
    let mut w = duel();
    w.battle.monster_ai_state.flag_bd84 = 1;
    w.battle_ctx.active_actor = 0;
    // Retail gate `0x801EEB88` reads the **attacker's** `+0x1D9`: the cue
    // is submitted only while it plays a plain action-table clip.
    w.actors[0].battle.current_anim = 0x11;
    w.land_melee_hit(0, 1, BASIC_ATTACK_COMMAND, 0, false, true);
    assert!(w.take_sfx_ring_ops().is_empty());
}
