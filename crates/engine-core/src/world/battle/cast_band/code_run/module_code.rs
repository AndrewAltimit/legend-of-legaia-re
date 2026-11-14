//! `run_cast_module_code`: one tick of a slot-B cast module's code, dispatched
//! by module id.

use super::*;

/// The Spikefish flute's band entry (spell `0x97`).
const SPIKEFISH_ENTRY: u32 = 925;

/// The player-Seru modules whose `0xFF` arm clears the creature's
/// `+0x21B` / `+0x176` (Theeder, Gizam, Nighto, Zenoir, Swordie, Orb, Freed,
/// Nova). PROT 0903 and 0905 have no such store, and 0909's clear sits in
/// its stager instead.
const SERU_EXIT_LOOP_CLEARS: [u32; 8] = [904, 906, 907, 908, 910, 911, 912, 913];

impl World {
    pub fn run_cast_module_code(&mut self, spell_id: u8, arm: u8) -> Option<CastModuleCodeRun> {
        use legaia_engine_cast::cast_arm_ticks as arms;
        use legaia_engine_cast::cast_module_ticks as ticks;
        use legaia_engine_cast::seru_magic_ticks as ticks_a;

        let entry = self.cast_module_for(spell_id)?;
        self.step_module_drive(arm);
        self.raise_cast_voice_on_arm(arm);
        self.fold_glare_on_its_arm(entry, spell_id, arm);
        // PROT 0954's body is ported whole - camera, records, wheel and
        // outcomes - and drives the band on its own
        // (`world::battle::fatal_decision`).
        if entry == legaia_engine_cast::cast_fatal_decision::FATAL_DECISION_ENTRY {
            return Some(self.run_fatal_decision(entry));
        }
        self.casting.module_running_entry = Some(entry);
        self.casting.module_bump_commits = 0;
        let mut ctx = self.cast_module_ctx();
        // The phase the tick enters on, before a state-touching stager below
        // steps it (PROT 0909's arm 0 is its stager's).
        let phase_entry = ctx.phase;
        let caster_slot = self.battle_ctx.active_actor;
        let live_victim = self
            .actors
            .get(caster_slot as usize)
            .map(|a| a.battle.active_target)
            .unwrap_or(0);
        // PROT 0908's prologue takes its victim from the caster's `+0x1DD`
        // on arm 0 alone, which latches it into the module word `0x801F8BA8`
        // (`0x801F6B6C..0x801F6B74`); every later arm reads it back from
        // there (`lw s3,-0x7458(v0)` at `0x801F6A5C`). Arm 8 then turns the
        // caster's `+0x1DD` to the enemy-row code `9`, so a live read from
        // arm 9 on named no seat: arm 10 tested reach from an empty slot's
        // pair and arm 11 lost its finisher to the splash.
        let victim_slot = if entry == 908 && phase_entry != ticks_a::ZENOIR_LATCH_ARM {
            self.casting.module_cam.victim_slot.unwrap_or(live_victim)
        } else {
            live_victim
        };
        let seat_slot = self.casting.summon_actor_slot.unwrap_or(ticks::SUMMON_SEAT);

        // The kernels index a retail-shaped seat row (`seat_map`): these
        // three are the same actors' seats in it.
        let map = self.cast_seat_map();
        let (r_caster, r_victim, r_seat) = (
            usize::from(map.retail(caster_slot)),
            usize::from(map.retail(victim_slot)),
            usize::from(map.retail(seat_slot)),
        );
        let mut caster = self.cast_view(caster_slot);
        let mut victim = self.cast_view(victim_slot);
        let mut seat = self.cast_view(seat_slot);
        let (mut caster_orig, mut victim_orig, mut seat_orig) = (caster, victim, seat);
        let mut run = CastModuleCodeRun {
            prot_entry: entry,
            busy: true,
            ..Default::default()
        };

        // The seven state-touching spawn stagers, by owning entry.
        // PROT 0906's stager is called from its spawned record's program,
        // not off the phase: its arms are placed below on the director's
        // phase-5 record clock.
        match entry {
            // PROT 0909's stager is reached only through its parts' op-`0x20`
            // calls - two of them are the tick body's phase rendezvous.
            909 => self.run_viguro_stager_calls(&mut ctx, &mut seat),
            // PROT 0907's and 0908's stagers only seat more records off the
            // calling parts (Nighto's rays and rings, Zenoir's floor rings,
            // rays and final blasts).
            e if Self::runs_part_stager_calls(e) => self.run_part_stager_calls(e, 0),
            922 => self.casting.run_puera_stager_calls(&mut ctx),
            927 => self.run_juggernaut_stager_calls(spell_id),
            // PROT 0923's stager is reached through its parts' op-`0x20`
            // calls, never off the phase: its arm 0 (the stage byte `3`) runs
            // when the record that calls it does. A retail cast holds
            // `ctx[+0x278] = 2` through arms 4 and 5's head and `3` only
            // later in arm 5; run off the phase, arm 0 dropped the stage
            // from the cast's first tick.
            // Arms 0 and 2 also seat a record on their caller: arm 0 the
            // fire bowl the creature stands in from arm 5 on.
            923 => {
                for (call_arm, caller) in std::mem::take(&mut self.casting.module_stager_calls) {
                    ticks::gilium_stager(&mut ctx, call_arm);
                    // Arm 1 halts arm 8's last three parts and spawns its
                    // grey flash fade.
                    if call_arm == 1 {
                        for slot in ticks::GILIUM_STAGER_HALTS {
                            self.casting.kill_parked_module_part(
                                legaia_overlay_images::cast_presentation::HandleSlot::Abs(slot),
                            );
                        }
                        let mut fade = None;
                        legaia_engine_system::fade::spawn_fade(
                            &mut fade,
                            &legaia_engine_battle_vm::battle_action::summon_fade_template(
                                &ticks::GILIUM_STAGER_FADE,
                            ),
                            1,
                        );
                        self.presentation.module_fades.extend(fade);
                    }
                    for s in &ticks::gilium_stager_spawns(call_arm, caller) {
                        self.casting.seat_stager_spawn(entry, s);
                        // Arm 0 parks the bowl's handle where the module's
                        // halt loops find it, beside the tick body's own
                        // parked parts.
                        if s.record == ticks::GILIUM_BOWL_RECORD {
                            self.casting.park_module_part(
                                legaia_overlay_images::cast_presentation::HandleSlot::Abs(
                                    ticks::GILIUM_BOWL_HANDLE,
                                ),
                                s.record,
                            );
                        }
                    }
                }
            }
            // PROT 0949's stager is reached through its parts' op-`0x20` calls,
            // never off the phase: a retail cast walks the rate `7..=0` while
            // the crystals close, and the hit's commit leaves the victim's
            // knockdown on its own rate. Run off the phase, phases 4 and 5
            // wrote `3` and `2` over it.
            949 => {
                for (call_arm, _) in std::mem::take(&mut self.casting.module_stager_calls) {
                    ticks::water_crystals_stager(&mut victim, call_arm);
                }
            }
            _ => {}
        }
        // Only these modules' stager calls are run; any other module's are
        // dropped rather than left to pile up.
        if entry != 909 && !Self::runs_part_stager_calls(entry) {
            self.casting.module_stager_calls.clear();
        }

        // The tick bodies, by owning entry. `hit` is `None` because the fold
        // is the band seam's, not the tick's (see the note above).
        //
        // A capture-class module reaches its body through a **trampoline**
        // (`ticks::capture_tick_body`), which switches on the caster's queued
        // action id, so a multi-spell cell picks a different choreography for
        // each of its ids. Where the module has one, the trampoline decides
        // whether anything ticks at all: an id it does not name returns zero
        // and the drive loop proceeds.
        //
        // The arm has to be keyed on `(entry, body)`, never on the body VA
        // alone: six modules in the band put a tick body at `0x801F69D8` -
        // the load base itself - so PROT 0960's Neo Star Slash and PROT
        // 0965's Doomsday wear the same address in different images.
        let body = ticks::capture_tick_body(entry, spell_id);
        let has_trampoline = ticks::capture_trampoline_for(entry).is_some();
        // A player-Seru module whose camera arms are ported paces its own
        // arms: the director runs first and, while an arm's countdown holds,
        // the phase-chain body does not run at all (retail's arm returns
        // before any of its writes). Its shot is the camera the summon band's
        // `0x35` / `0x36` hand the battle camera.
        let profile = if has_trampoline {
            None
        } else {
            legaia_engine_cast::cast_module_camera::module_profile(entry)
        };
        // The module countdown this pass opens on (PROT 0917's and 0922's
        // seat writes read it).
        let countdown_before = self.casting.module_cam.countdown.0;
        // PROT 0916's arm-0 kill roll, drawn once per cast ahead of its
        // director.
        if entry == legaia_engine_cast::cast_module_camera::ALURU_ENTRY {
            self.draw_aluru_verdict(caster_slot, victim_slot, spell_id);
        }
        // PROT 0917's arm `0xF` re-runs the target-size framing `FUN_801F0348`
        // at the head of every pass (`jal` at `0x801F805C`), ahead of the
        // shot that reads `ctx[+0x6D0]` (`0x801F8094`, `0x801F8104`): the
        // victim on the ground is framed off its own size, not the depth
        // whatever ran before left.
        //
        // The same call heads an arm of seven more, each before a shot that
        // reads the word: PROT 0906 arm `0xD` (`0x801F7570`), 0907 arm `0xB`
        // (`0x801F7C1C`), 0910 arm 9 (`0x801F796C`), 0916 arm `0x11`
        // (`0x801F7CE4`), 0918 arm `0xE` (`0x801F7EA4`), and behind a seat
        // target (`bVar < 8`) 0914 arm `0xD` (`0x801F75DC`) and 0920 arm 8
        // (`0x801F75E0`). Without it those shots read the summon band's `0x600`.
        let seat_target = self
            .actors
            .get(usize::from(caster_slot))
            .is_some_and(|a| a.battle.active_target < 8);
        if matches!(
            (entry, ctx.phase),
            (917, 0xF) | (906, 0xD) | (907, 0xB) | (910, 9) | (916, 0x11) | (918, 0xE)
        ) || (seat_target && matches!((entry, ctx.phase), (914, 0xD) | (920, 8)))
        {
            let d = self.cast_target_frame_depth(caster_slot);
            self.battle.camera_frame_height = d;
            self.battle_ctx.camera_frame_height = d;
        }
        // PROT 0915's arm `0xC` turns its creature before it frames it.
        if entry == 915
            && profile.is_some()
            && ctx.phase == legaia_engine_cast::cast_module_camera::MUSHURA_HIT_ARM
        {
            self.mushura_chase_ahead(ctx.monster_count, seat_slot, caster_slot, spell_id);
        }
        let drive_holds = self.module_drive_holds(ctx.phase);
        let direction = if has_trampoline {
            None
        } else {
            legaia_engine_cast::cast_module_camera::module_director(entry).map(|direct| {
                let latched = *self
                    .casting
                    .module_cam
                    .victim_slot
                    .get_or_insert(victim_slot);
                let seats = self.module_cam_seats(caster_slot, latched);
                let mut st = self.module_cam_with_yaw_base();
                self.feed_seru_cam(entry, caster_slot, &mut st);
                // The latched victim's display pair `+0x3C` / `+0x40`, which
                // some arms frame instead of its live pair (PROT 0910's arm 9
                // and settle, 0916's arms `0x13` / `0x14`, 0917's arm `0xF`).
                st.victim_body = self.module_body_seat(latched);
                // The battle frame this tick belongs to: an arm that steps a
                // fixed amount once a pass (PROT 0913's arm `0x10` yaw) lands
                // it on the tick that opens the frame.
                st.frame_step = self.module_frame_step();
                st.opens_pass = self.module_tick_opens_pass();
                // A settle gate's row (`summon_holds::Gate::Settle`): every
                // monster seat idle, or faded out once dead.
                let pc = usize::from(self.party.party_count);
                st.row_settled =
                    (pc..pc + usize::from(ctx.monster_count)).all(|s| match self.actors.get(s) {
                        Some(a) if a.battle.hp == 0 => a.battle.render_color == 0 || !a.active,
                        _ => self.chain_seat_settled(s, false),
                    });
                // PROT 0916's arm `0x14` settle poll on the latched victim.
                st.victim_settled = self.chain_seat_settled(usize::from(latched), true);
                // The applier stage `ctx[+0x276]` a side-band wait arm polls:
                // up while the engine's stand-in for its reads runs, or while
                // a capture replay holds it.
                let held = st.sideband_busy;
                st.sideband_busy |= self.battle_ctx.menu_open != 0;
                // The poll holds the arm's way out, not what runs ahead of it:
                // PROT 0917's arm 1 rises to TR y `0x800` first
                // (`iRam800840bc` under `0x800`), PROT 0919's drifts while its
                // word runs. An arm that would pass makes none of its exit's
                // stores, and keeps its drain and its drift.
                // The drive's wait holds the same way: the seek arm until the
                // drive takes the seek, the poll arm until the seek is done.
                let polling = (st.sideband_busy
                    && legaia_engine_cast::cast_module_camera::sideband_wait_arm(entry)
                        == Some(ctx.phase))
                    || drive_holds;
                let d = if polling {
                    let mut ahead = st;
                    let d = direct(&mut ahead, ctx.phase, seats);
                    if d.hold {
                        st = ahead;
                        d
                    } else {
                        st.countdown = ahead.countdown;
                        legaia_engine_cast::cast_module_camera::ArmDirection {
                            drift: d.drift,
                            ..legaia_engine_cast::cast_module_camera::ArmDirection::HOLD
                        }
                    }
                } else {
                    direct(&mut st, ctx.phase, seats)
                };
                st.sideband_busy = held;
                self.store_module_cam(st);
                d
            })
        };
        if direction.as_ref().is_some_and(|d| !d.hold) {
            self.raise_cast_voice_on_pass(ctx.phase);
        }
        // PROT 0903's settle arm halves the victim's animation rate on every
        // pass, held or not: the store (`lbu v0,0x69(a0)` / `srl v0,v0,0x1` /
        // `sb v0,0x21d(s2)` at `0x801F76C8..0x801F76D8`) sits past the
        // countdown's `bgtz` at `0x801F7654`, which jumps to it. The body's
        // own arm 12 runs only once the director lets the phase through, so
        // the half the hold covers is stored here - it is what slows the dead
        // victim's knock-back clip, whose fade the arm then waits out.
        if entry == 903
            && ctx.phase == ticks_a::GIMARD_SETTLE_ARM
            && let Some(a) = self.actors.get_mut(usize::from(victim_slot))
        {
            a.battle.anim_rate = legaia_engine_battle_vm::battle_anim_rate::AnimRate(
                ticks_a::GIMARD_SETTLE_ANIM_RATE,
            );
        }
        if direction.is_some() {
            self.apply_seru_seat_writes(entry, ctx.phase);
        }
        if entry == 966 {
            self.evil_seru_ride_lead();
            // Arm 0 zeroes the shake word (`sw zero,-0x5B98` at
            // `0x801F6B90`) and the body's epilogue runs it down once a pass
            // (`0x801F8A94..0x801F8AE8`), the code PROT 0927's has.
            if phase_entry == 0 {
                self.casting.module_shake = 0;
            }
            self.juggernaut_shake();
        }
        if entry == 927
            && let Some(d) = direction
        {
            self.apply_juggernaut_writes(ctx.phase, !d.hold, ctx.monster_count, spell_id);
        }
        if entry == 918
            && let Some(d) = direction
        {
            self.apply_kemaro_writes(ctx.phase, !d.hold, victim_slot);
        }
        if entry == 917
            && let Some(d) = direction
        {
            self.apply_barra_writes(ctx.phase, !d.hold, countdown_before, victim_slot);
        }
        if entry == 922
            && let Some(d) = direction
        {
            self.apply_puera_writes(ctx.phase, !d.hold, countdown_before, ctx.monster_count);
        }
        run.camera_shot = direction.and_then(|d| d.shot);
        // PROT 0909: the pass whose arm spawns a stager-calling record starts
        // that record's clock; the spawn's seating drops it again
        // (`stage_module_arm_spawns`) when the record runs as a part.
        if entry == 909
            && let Some(d) = direction
            && !d.hold
        {
            use legaia_engine_cast::cast_module_camera::{SpawnRecord, VIGURO_STAGER_CALLERS};
            for spawn in d.spawns {
                if let SpawnRecord::Module(va) = spawn.record
                    && let Some(&(record, arm, wait)) =
                        VIGURO_STAGER_CALLERS.iter().find(|c| c.0 == va)
                {
                    self.casting.module_record_clocks.retain(|c| c.0 != record);
                    self.casting.module_record_clocks.push((record, arm, wait));
                }
            }
        }
        // PROT 0906: record `0x801F7B80`'s `op 0x20` calls into the stager,
        // on the clock the director runs through the phase-5 hole. Arm 0 is
        // the flash pair, arm 1 shows the creature; arm 2 is the director's
        // pass out of phase 5.
        if entry == 906 {
            use legaia_engine_cast::cast_module_camera as cam;
            match cam::gizam_stager_arm_due(ctx.phase, self.casting.module_cam.countdown.0) {
                Some(0) => {
                    for (t, id) in &cam::GIZAM_STAGER_FLASHES {
                        let mut fade = None;
                        legaia_engine_system::fade::spawn_fade(
                            &mut fade,
                            &legaia_engine_battle_vm::battle_action::summon_fade_template(t),
                            *id,
                        );
                        self.presentation.module_fades.extend(fade);
                    }
                }
                Some(a) => ticks::gizam_stager(&mut ctx, &mut seat, a),
                None => {}
            }
        }
        // PROT 0905's arms 4 and 5 kill the effect parts the arm before them
        // spawned, by the handles kept at `0x801F8828` (`ori 0x8` into each
        // actor's flag word, `0x801F6ECC..0x801F6F2C` and
        // `0x801F71A0..0x801F71E8`): arm 0's four glows and arm 4's three
        // cut parts. Arm 5's two parts run until they halt.
        if entry == 905
            && direction.is_some_and(|d| !d.hold)
            && let Some(killed) = legaia_engine_cast::cast_module_camera::vera_arm_kills(ctx.phase)
            && let Some(scene) = self.casting.active_summon.as_mut()
        {
            for part in &mut scene.parts {
                if part.tag.is_some_and(|t| killed.contains(&t)) {
                    part.finished = true;
                }
            }
        }
        run.capture_drift = direction.and_then(|d| d.drift);
        self.casting.module_tick_rest = direction.and_then(|d| d.rest_drift);
        run.capture_drift_after_shot = direction.is_some_and(|d| d.drift_after_shot);
        run.tr_z_store = direction.and_then(|d| d.tr_z_store);
        let mut capture_held = false;
        let mut capture_arm = None;
        let mut capture_vram_move = None;
        let phase_in = ctx.phase;
        let mut death_game_pass = None;
        // A capture-class body's camera arms, on the phase it is about to
        // run (they make no gate of their own: the body's port does).
        if let Some(direct) = legaia_engine_cast::cast_module_camera::capture_camera_director(
            entry,
            body.unwrap_or(legaia_engine_cast::cast_module_camera::SINGLE_BODY),
        ) {
            // The victim the cast latched on its first pass: a spawn on the
            // victim seat resolves through it.
            self.casting
                .module_cam
                .victim_slot
                .get_or_insert(victim_slot);
            // PROT 0961's arm 0 stands the seats before its cut frames the
            // caster; its `0xFF` arm stands them back.
            if entry == 961
                && body == Some(legaia_engine_cast::cast_module_camera::DEAD_END_CRISIS_BODY)
            {
                self.dead_end_crisis_seats(ctx.phase);
            }
            let seats = self.module_cam_seats(caster_slot, victim_slot);
            let mut st = self.module_cam_with_yaw_base();
            self.feed_capture_cam(caster_slot, victim_slot, &mut st);
            // Curse All frames on the target row's centroid (`FUN_801DCEAC`).
            if entry == 944 || entry == 935 || entry == 938 || entry == 963 {
                self.feed_seru_cam(entry, caster_slot, &mut st);
            }
            // PROT 0964's strike picks its opening record off the last roll.
            if entry == 964 {
                st.module_words[0] = i32::from(self.cast_element_change_last_roll());
            }
            let death_game = entry == 957 && body == Some(ticks::SUMMON_EFFECT_TICK_B);
            if death_game {
                self.feed_death_game(ctx.phase, seats.caster_monster, caster_slot, &mut st);
            }
            let arm = direct(&mut st, ctx.phase, seats);
            if death_game {
                death_game_pass = Some(st.death_game);
            }
            self.store_module_cam(st);
            if let Some(v) = arm.latch
                && let Some(i) = self.caster_latch_index(caster_slot)
            {
                self.battle.monster_ai_state.dat[i] = v;
            }
            if arm.skips_fold {
                self.casting.module_skips_fold = true;
            }
            if let Some(v) = arm.first_monster_record_1c
                && let Some(first) = self.battle_seat_map().engine(3)
                && let Some(b) = self
                    .battle
                    .monster_ai_state
                    .b3_armed
                    .get_mut(usize::from(first) % 8)
            {
                *b = v;
            }
            run.camera_shot = arm.shot;
            run.capture_drift = arm.drift;
            run.capture_pose_set = arm.pose_set;
            if let Some(v) = arm.ctx_278 {
                ctx.ctx_278 = v;
            }
            if let Some((clip, window)) = arm.caster_clip_window {
                self.write_clip_loop_window(caster_slot, clip, window);
            }
            if let Some((clip, bounds)) = arm.caster_clip_bounds {
                self.write_clip_loop_bounds(caster_slot, clip, bounds);
            }
            if let Some((clip, speed)) = arm.caster_clip_speed {
                self.write_clip_root_speed(caster_slot, clip, speed);
            }
            run.capture_focus_z = arm.focus_z;
            run.capture_drift_first = arm.drift_first;
            capture_held = arm.hold;
            self.casting.module_debris.extend(
                arm.pass_spawns
                    .iter()
                    .chain(&arm.charge_spawns)
                    .flatten()
                    .copied(),
            );
            self.casting.module_debris_at_pass_end = arm.spawns_at_pass_end;
            self.casting.module_scatter = arm.scatter;
            self.casting.module_trail = arm.trail;
            self.casting.module_rock_burst = arm.rock_burst;
            self.casting.module_spray = arm.spray;
            self.casting.module_ring = arm.ring;
            self.casting.module_scatter_pairs = arm.scatter_pairs;
            self.casting.module_scatter_level = arm.parts_level;
            capture_vram_move = arm.vram_move;
            capture_arm = Some(arm);
        }
        if let Some(g) = death_game_pass {
            self.carry_death_game_pass(&g, phase_in, caster_slot);
            if let Some(outcome) = g.outcome {
                let max_hp = [caster_slot, victim_slot].map(|s| {
                    self.actors
                        .get(usize::from(s))
                        .map_or(0, |a| a.battle.max_hp)
                });
                let opened = [victim.hp, caster.hp];
                ticks::death_game_outcome(outcome, &mut caster, &mut victim, max_hp);
                // The bleed's number pops (`FUN_801F44A0` at `0x801F93AC`,
                // `0x801F9558`), each for the `(hp + 3) >> 2` it opens with.
                if outcome == legaia_engine_cast::cast_module_camera::DeathGameOutcome::BothBleed {
                    let pops = [0, 1].map(|i| ticks::AoeHit {
                        seat: [victim_slot, caster_slot][i],
                        applied: (i32::from(opened[i]) + 3) >> 2,
                    });
                    run.aoe_hits.extend(pops);
                }
            }
        }
        if let Some(reach) = capture_arm.as_ref().and_then(|a| a.caster_reach) {
            self.store_caster_reach(caster_slot, reach);
        }
        if let Some(arm) = capture_arm.as_ref() {
            if let Some(r) = arm.caster_radius {
                self.store_caster_radius(caster_slot, Some(r));
            }
            if arm.caster_radius_restore {
                self.store_caster_radius(caster_slot, None);
            }
        }
        // A director arm that names the body's damage-wrapper call folds the
        // owed cast on the pass it makes it (PROT 0962 `0xA2` arm 3), so the
        // victim's reaction - which the body's next arm waits out - starts
        // there, as retail's does.
        if let Some(arm) = capture_arm
            && arm.folds_hit
            && !arm.hold
            && self.casting.module_hit_arm != Some(phase_in)
        {
            self.casting.module_hit_arm = Some(phase_in);
            // Thunder Storm's arm 5 seats its scatter pairs and calls its
            // ribbons (`0x801F7330..0x801F7498`) ahead of the countdown test
            // and the party's hit (`jal 0x801DD4B0` at `0x801F7514`), so on
            // a pass that opens and lands the hit at once both draw first.
            if entry == 957
                && arm.scatter_pairs
                && arm.ribbons_on_scatter_y
                && self.module_tick_opens_pass()
                && std::mem::take(&mut self.casting.module_scatter_pairs)
            {
                self.seat_storm_scatter_pairs(entry, arm.shot);
                self.battle.cast_ribbons = arm.ribbons;
                self.deal_storm_ribbons(true);
                self.casting.module_storm_dealt = true;
            }
            // Gigaton Press seats its debris ring - sixteen `rand() % 4`
            // picks - ahead of its `FUN_801DD4B0` (`0x801F717C..0x801F72B4`),
            // so the ring draws first.
            if entry == 962
                && body == Some(legaia_engine_cast::cast_module_camera::GIGATON_PRESS_BODY)
                && let Some(v) = self.actors.get(usize::from(victim_slot))
            {
                let victim = legaia_engine_cast::cast_module_camera::ModuleSeat {
                    x: v.move_state.world_x,
                    y: v.move_state.world_y,
                    z: v.move_state.world_z,
                    facing: v.battle.facing_angle & 0xFFF,
                };
                self.casting.module_debris =
                    legaia_engine_cast::cast_module_camera::gigaton_press_debris(victim, || {
                        self.next_rand()
                    });
            }
            // The sweep's seats are those with HP and no Stone before the hit.
            let struck: Vec<u8> = (0..self.party.party_count)
                .filter(|&s| {
                    arm.party_faces_caster
                        && self.actors[usize::from(s)].battle.hp > 0
                        && !self.battle.actor_is_petrified(s)
                })
                .collect();
            self.fold_pending_cast_at_rate(
                if has_trampoline {
                    body.and_then(|b| ticks::chain_body_for(entry, b))
                } else {
                    ticks::direct_chain_body(entry)
                }
                .and_then(|c| c.arm(phase_in))
                .and_then(|a| a.hit_rate),
            );
            for s in struck {
                self.face_seat_away_from_caster(s, caster_slot);
            }
            caster = self.cast_view(caster_slot);
            victim = self.cast_view(victim_slot);
            seat = self.cast_view(seat_slot);
            (caster_orig, victim_orig, seat_orig) = (caster, victim, seat);
        }
        // PROT 0966 (Evil Seru Magic) has no other port: its seat half rides
        // the director's gate, on the phase the director is answering for.
        if entry == 966
            && let Some(arm) = capture_arm
        {
            let (phase, passed) = (ctx.phase, !arm.hold);
            if passed && let Some(t) = evil_seru_magic_fade(phase) {
                legaia_engine_system::fade::spawn_fade(&mut self.presentation.fade, &t, 1);
            }
            // The module's own move-VM stager hit lands mid-cast, not at the
            // fold: arm 10 spawns record `0x801F937C`, whose script waits
            // `0x7F` (`<< 3`, drained `scalar * delta` - 127 vsyncs) and then
            // runs op `0x20` with arm 4, the `0x100` never-kill sweep. That
            // is arm 11's gate (`0x80` vsyncs), so it lands before arm 26's
            // kill-capable hit, and the band's fold owes nothing more.
            let mut stager_hits = Vec::new();
            if passed
                && phase == legaia_engine_cast::cast_module_camera::EVIL_SERU_MAGIC_STAGER_HIT_ARM
            {
                // The stager's arm 4 (`0x801F8E4C`) opens on Juggernaut's
                // salmon flash and its shake (`+0x14` into `0x801FA468`)
                // before the sweep.
                if let Some(t) = ticks::juggernaut_stager_fade(AOE_STAGER_WORKING_ARM) {
                    let mut fade = None;
                    legaia_engine_system::fade::spawn_fade(
                        &mut fade,
                        &legaia_engine_battle_vm::battle_action::summon_fade_template(&t),
                        1,
                    );
                    self.presentation.module_fades.extend(fade);
                }
                self.casting.module_shake += ticks::juggernaut_stager_shake(AOE_STAGER_WORKING_ARM);
                if let Some(r) =
                    self.run_cast_module_aoe_as(caster_slot, spell_id, AOE_STAGER_WORKING_ARM)
                {
                    stager_hits = r.aoe_hits;
                }
                self.casting.module_skips_fold = true;
            }
            let mut seats: Vec<ticks::CastActorState> = self.cast_seat_row();
            let mut rolls: Vec<(u8, i32)> = Vec::new();
            if passed && phase == legaia_engine_cast::cast_module_camera::EVIL_SERU_MAGIC_SWEEP_ARM
            {
                // Rolled only for the seats the hit visits, in seat order,
                // so the shared RNG cursor moves as retail's does.
                for s in 0..ctx.party_count {
                    if seats
                        .get(s as usize)
                        .is_some_and(ticks::aoe_seat_is_hittable)
                    {
                        let r = self
                            .capture_module_roll(
                                &ticks::EVIL_SERU_MAGIC_SWEEP_SHAPE,
                                caster_slot,
                                s,
                            )
                            .unwrap_or(0);
                        rolls.push((s, r));
                    }
                }
                self.refresh_seat_spirit(&mut seats);
            }
            let take = |s: u8| {
                rolls
                    .iter()
                    .find(|(seat, _)| *seat == s)
                    .map_or(0, |(_, r)| *r)
            };
            let hits = ticks::evil_seru_magic_seat_writes(
                phase,
                passed,
                ctx.party_count,
                r_caster as u8,
                &mut seats,
                take,
            );
            self.write_cast_seat_row(&seats);
            run.aoe_hits = stager_hits;
            let hits = self.cast_hits_to_engine(hits.iter().map(|h| ticks::AoeHit {
                seat: h.seat,
                applied: h.applied as i32,
            }));
            run.aoe_hits.extend(hits);
        }
        // A capture body with no camera director still gates its arms on
        // the module countdown: the arm's phase chain runs only on the tick
        // the gate lets through (`cast_module_camera::capture_countdown`).
        let arm_countdown = if capture_arm.is_none() {
            legaia_engine_cast::cast_module_camera::capture_arm_countdowns(
                entry,
                body.unwrap_or(legaia_engine_cast::cast_module_camera::SINGLE_BODY),
            )
            .and_then(|t| legaia_engine_cast::cast_module_camera::arm_countdown(t, phase_in))
        } else {
            None
        };
        if let Some(a) = arm_countdown
            && a.holds(&mut self.casting.module_cam.countdown)
        {
            capture_held = true;
        }
        // A phase-chain body's hit lands on the tick its arm first runs -
        // retail calls the damage wrapper inside the arm - and an arm that
        // then waits on the hit seats' clips holds until they settle. The
        // runner itself holds no clip state, so both happen here.
        let chain = if has_trampoline {
            body.and_then(|b| ticks::chain_body_for(entry, b))
        } else {
            ticks::direct_chain_body(entry)
        };
        // The arm's party-row rate store sits ahead of its countdown test, so
        // it runs on every pass the arm holds.
        if let Some(rate) = legaia_engine_cast::cast_module_ticks::party_rate_sweep(
            entry,
            if has_trampoline { body } else { None },
            phase_in,
        ) {
            for s in 0..usize::from(self.party.party_count) {
                if let Some(a) = self.actors.get_mut(s) {
                    a.battle.anim_rate = legaia_engine_battle_vm::battle_anim_rate::AnimRate(rate);
                }
            }
            caster = self.cast_view(caster_slot);
            victim = self.cast_view(victim_slot);
            seat = self.cast_view(seat_slot);
        }
        if let Some((hit_arm, settle_arm, settle)) =
            ticks::gated_hit_arm(entry, if has_trampoline { body } else { None })
            && !capture_held
        {
            if phase_in == settle_arm && !self.chain_settled(settle, caster_slot, victim_slot, &ctx)
            {
                capture_held = true;
            }
            if phase_in == hit_arm && self.casting.module_hit_arm != Some(hit_arm) {
                self.casting.module_hit_arm = Some(hit_arm);
                // PROT 0945's Water Column draws its Venom ahead of the hit:
                // `rand() & 7 == 0` ORs bit `1` into the victim's `+0x16E`
                // (`0x801F7460..0x801F7480`), then the arm calls
                // `FUN_801DD4B0(0x30)`, so the damage rolls one draw later.
                if entry == 945
                    && body == Some(ticks::WATER_COLUMN_TICK)
                    && self.next_rand() & 7 == 0
                {
                    self.battle.status_effects.apply(
                        victim_slot,
                        legaia_engine_battle_vm::status_effects::StatusKind::Venom,
                    );
                }
                self.fold_pending_cast();
                // PROT 0951 `0x36`'s strike stores the victim's rate `2`
                // (`li v1,2; sb v1,0x21d(s1)` at `0x801F7494`), and the
                // reaction's cues run on it.
                if entry == 951
                    && body == Some(ticks::CHAOS_FLARE_TICK)
                    && let Some(v) = self.actors.get_mut(usize::from(victim_slot))
                {
                    v.battle.anim_rate = legaia_engine_battle_vm::battle_anim_rate::AnimRate(2);
                }
                caster = self.cast_view(caster_slot);
                victim = self.cast_view(victim_slot);
                seat = self.cast_view(seat_slot);
                (caster_orig, victim_orig, seat_orig) = (caster, victim, seat);
            }
        }
        if entry == 939 {
            if phase_in == 3 {
                self.casting.spore_gas_beat = ticks::SporeGasBeat::armed();
            }
            if phase_in == 4
                && let Some(hit) = self.casting.spore_gas_beat.tick()
            {
                self.spore_gas_hit(hit, caster_slot, victim_slot);
                self.casting
                    .module_debris
                    .push(legaia_engine_cast::cast_module_camera::spore_gas_hit_spawn());
            }
            caster = self.cast_view(caster_slot);
            victim = self.cast_view(victim_slot);
            seat = self.cast_view(seat_slot);
            (caster_orig, victim_orig, seat_orig) = (caster, victim, seat);
        }
        if let Some(arm) = chain.and_then(|c| c.arm(phase_in))
            && !capture_held
        {
            if arm.wrapper_site.is_some() && self.casting.module_hit_arm != Some(phase_in) {
                self.casting.module_hit_arm = Some(phase_in);
                // A split cast's hits are its own, on their beat
                // (PROT 0939's, PROT 0937's arm 7), and so are a hit-landing
                // director's (PROT 0958 / 0959, below): its sites roll and
                // write each hit, so the fold this arm opens owes the cast
                // nothing. Left to the director's own store further down the
                // pass, Megaton Press's arm 8 folded one generic roll ahead
                // of its site-0 hit, 76 on top of retail's 67.
                self.casting.module_skips_fold |= ticks::split_hits_for(
                    entry,
                    body.unwrap_or(legaia_engine_cast::cast_module_camera::SINGLE_BODY),
                )
                .is_some()
                    || director_lands_hits(entry);
                // A hit whose arm marks its victim stages no reaction
                // (`ticks::chain_hit_stages_no_reaction`, PROT 0956's `0x75`);
                // a victim the hit downs takes the dead branch's knockdown.
                let marks = ticks::chain_hit_stages_no_reaction(arm, &victim);
                if marks {
                    self.casting.module_fold_quiet = true;
                }
                // A status the arm rolls ahead of its damage call draws first
                // (PROT 0947's Neo Windhash Curse, `0x801F73E0`), so the
                // damage rolls one draw later.
                if let Some(roll) =
                    chain.and_then(|c| ticks::chain_pre_hit_roll(entry, c.body, spell_id))
                    && self.next_rand() & roll.mask == 0
                {
                    self.battle.status_effects.apply(victim_slot, roll.kind);
                }
                self.fold_pending_cast_at_rate(arm.hit_rate);
                if marks {
                    self.casting.module_fold_quiet = false;
                    let v = usize::from(victim_slot);
                    if self.actors.get(v).is_some_and(|a| a.battle.hp == 0) {
                        self.queue_battle_reaction(v, false);
                    }
                }
                // The arm runs on the seats the hit left (a dead victim
                // takes PROT 0956's reaction branch, not its turn-steal).
                caster = self.cast_view(caster_slot);
                victim = self.cast_view(victim_slot);
                seat = self.cast_view(seat_slot);
                (caster_orig, victim_orig, seat_orig) = (caster, victim, seat);
            }
            if let Some(settle) = arm.settle {
                let settled = self.chain_settled(settle, caster_slot, victim_slot, &ctx);
                self.casting.module_settle_ticks =
                    self.casting.module_settle_ticks.saturating_add(1);
                if settled
                    || self.casting.module_settle_ticks
                        > legaia_engine_cast::cast_fatal_decision::SETTLE_TICK_LIMIT
                {
                    self.casting.module_settle_ticks = 0;
                } else {
                    capture_held = true;
                }
            }
        }
        // The arm's streak-ribbon calls draw their `rand()` words inside the
        // arm, ahead of its rumble words (PROT 0934's `0x801F6EE4` before
        // `0x801F6F10`). PROT 0957's arm 5 calls after its scatter loop, so
        // its ring is dealt where the scatter's height reaches it.
        let pass_ribbons = capture_arm
            .as_ref()
            .and_then(|a| a.ribbons)
            .or(direction.and_then(|d| d.ribbons));
        if pass_ribbons.is_none() {
            self.battle.cast_ribbon_seeds = None;
        }
        if let Some(ring) = pass_ribbons.as_ref()
            && !capture_arm.as_ref().is_some_and(|a| a.ribbons_on_scatter_y)
            && self.module_tick_opens_pass()
        {
            self.deal_cast_ribbon_rands(ring);
        }
        // The arm's controller rumble stores, on the edge each sits on.
        let rumble_passed = direction.map_or(!capture_held, |d| !d.hold);
        self.apply_module_rumble(
            entry,
            body.unwrap_or(legaia_engine_cast::cast_module_camera::SINGLE_BODY),
            phase_in,
            rumble_passed,
            countdown_before,
        );
        run.camera_follow = direction.and_then(|d| d.follow);
        run.camera_end_frame = direction.is_some_and(|d| d.end_frame);
        // A capture director's framing-case calls (PROT 0962 `0xA2`): case 6
        // follows the caster on the live yaw ladder and depth, case 8 is the
        // post-action framing.
        if let Some(arm) = capture_arm {
            run.camera_end_frame |= arm.end_frame;
            if arm.follow_caster {
                let st = self.module_cam_with_yaw_base();
                let seats = self.module_cam_seats(caster_slot, victim_slot);
                run.camera_follow = Some(legaia_engine_cast::cast_module_camera::ModuleFollow {
                    seat: seats.caster,
                    yaw_base: st.yaw_base,
                    depth_raw: seats.depth_raw,
                    style: arm.follow_style,
                });
            }
        }
        run.camera_nudge = direction.and_then(|d| d.nudge);
        run.spawns = direction.map_or(&[], |d| d.spawns);
        run.arm_point = direction.and_then(|d| d.arm_point);
        run.vram_move = direction.and_then(|d| d.vram_move).or(capture_vram_move);
        run.caption = direction.and_then(|d| d.caption);
        run.fades = direction.map_or(&[], |d| d.fades);
        run.kills_fades = direction.is_some_and(|d| d.kills_fades);
        let held = direction.is_some_and(|d| d.hold) || capture_held;
        // A director that owns the phase of a module whose tick body is
        // unported - one that carries the whole phase chain
        // (`ModuleProfile::director_body`), or one that only arms the camera
        // - advances it on its pass and claims no tick.
        let director_body = profile.is_some_and(|p| p.director_body);
        let owns_chain =
            profile.is_some_and(|p| (!p.paces_band() || p.director_body) && p.owns_phase);
        if owns_chain && !held {
            ctx.phase = direction
                .and_then(|d| d.jump)
                .unwrap_or(ctx.phase.wrapping_add(1));
        }
        // The summon-creature modules' part spawns, read off each image by
        // arm: the calls on the pass that leaves the arm, and those that run
        // on every pass.
        if direction.is_some()
            && legaia_engine_cast::cast_module_camera::CREATURE_SPAWN_ENTRIES.contains(&entry)
        {
            use legaia_engine_cast::cast_module_camera::{SpawnTiming, creature_arm_spawns};
            if ctx.phase != phase_in {
                run.spawns = creature_arm_spawns(entry, phase_in, SpawnTiming::Pass);
            }
            run.every_spawns = creature_arm_spawns(entry, phase_in, SpawnTiming::Every);
        }
        let step = if held && !owns_chain {
            Some(ticks::CastTickStep::Busy)
        } else if held || owns_chain {
            None
        } else if has_trampoline {
            match (entry, body) {
                (958, Some(ticks::BLAZING_SLASH_TICK)) => {
                    Some(ticks::blazing_slash_tick(&mut ctx, &mut victim, None))
                }
                (952, Some(ticks::ASTRAL_SLASH_TICK)) => {
                    // The body has no damage site in any arm. Astral Slash's
                    // damage is the clip arm 2 stages on the caster (Gaza's
                    // archive entry `0x0A`: power bytes `0D 0E 0F 10`, beats
                    // `14 15 16 17`), rolled hit by hit by the melee kernel
                    // the anim tick calls (`FUN_80047430` -> `FUN_801EC3E4`,
                    // `World::tick_battle_hit_events`) and landed on the
                    // band's exit. The generic fold would roll a wrapper on
                    // top of that total.
                    self.casting.module_skips_fold = true;
                    // Arm 2's `ctx[+0x15] = 0xFF`: the kernel's apply gate
                    // reads the parked cursor, so the total lands on the
                    // clip's last beat.
                    if ctx.phase == ticks::ASTRAL_SLASH_STRIKE_PARK_ARM
                        && let Some(a) = self.actors.get_mut(usize::from(caster_slot))
                    {
                        a.battle.strike_index =
                            legaia_engine_battle_vm::battle_action::STRIKE_CURSOR_PARKED;
                    }
                    // Arms 0 and 1 poll `FUN_8004E2F0(ctx[+0x13],
                    // caster[+0x1DD])`; arm 0 draws one `rand()` first.
                    let arrived = self.battle_range_metric(caster_slot, victim_slot) == 0;
                    let arm0_rand = if ctx.phase == 0 {
                        self.next_rand() as u16
                    } else {
                        0
                    };
                    Some(ticks::astral_slash_tick(
                        &mut ctx,
                        &mut caster,
                        &mut victim,
                        arrived,
                        arm0_rand,
                    ))
                }
                (945, Some(ticks::WATER_COLUMN_TICK)) => Some(ticks::water_column_tick(
                    &mut ctx,
                    &mut caster,
                    &mut victim,
                    None,
                    0,
                )),
                // PROT 0945's other choreography: the band's widest stat
                // write, on the caster.
                (945, Some(ticks::ALL_STATS_SURGE_TICK)) => {
                    let agl_record = self.cast_record_agl(caster_slot);
                    Some(ticks::all_stats_surge_tick(
                        &mut ctx,
                        &mut caster,
                        agl_record,
                    ))
                }
                (960, Some(ticks::PLASMA_STRIKE_TICK)) => {
                    // The burst arm's `0x1C0` roll. Retail aims it at
                    // `0x801C9370[0]` - seat 0, not the derived victim - so
                    // it rides the victim view only when the two coincide;
                    // the arm's other writes (the close, the knockdown) are
                    // the victim's either way.
                    let hit = if ctx.phase == ticks::PLASMA_STRIKE_BURST_ARM && victim_slot == 0 {
                        ticks::damage_shape_for(960)
                            .and_then(|sh| self.capture_module_roll(sh, caster_slot, 0))
                    } else {
                        None
                    };
                    // The burst is the module's one wrapper call (`jal
                    // 0x801DD6B4` at `0x801F8168`), and its HP writes are the
                    // cast's whole outcome beside arm `0x0C`'s flurry
                    // landing: once it has landed here, the band's generic
                    // fold owes nothing. Folding as well rolled the
                    // `0x1C0` a second time through the catalog def.
                    if let Some(roll) = hit {
                        self.casting.module_skips_fold = true;
                        // The popup `FUN_801F44A0(roll, 0)` on the raw roll
                        // (`jal` at `0x801F817C`, `a0` the roll's low half)
                        // ahead of the clamp, the `+0x10` accumulate and the
                        // HP write: the burst's numeral over seat 0.
                        // REF: FUN_801F44A0
                        self.battle
                            .hit_fx
                            .push(legaia_engine_battle::battle_events::BattleHitFx {
                                target_slot: 0,
                                amount: roll.clamp(0, i32::from(u16::MAX)) as u16,
                                is_heal: false,
                                is_crit: false,
                            });
                    }
                    Some(ticks::plasma_strike_tick(
                        &mut ctx,
                        &mut caster,
                        &mut victim,
                        hit,
                    ))
                }
                // PROT 0942's Power Up reads the caster's own monster record
                // `+0x0E` and writes the AGL **base** half only.
                (942, Some(ticks::POWER_UP_TICK)) => {
                    let agl_record = self.cast_record_agl(caster_slot);
                    Some(ticks::power_up_tick(&mut ctx, &mut caster, agl_record))
                }
                // The three whole-row sweeps. Each rolls the module's own
                // baked power per hittable seat off this world's RNG cursor,
                // in seat order, so the draw order stays retail's; the two
                // 1-in-8 status draws Chaos Breath makes per seat come off
                // the same cursor.
                (938, Some(ticks::CHAOS_BREATH_TICK))
                | (938, Some(ticks::MYSTIC_CIRCLE_TICK))
                | (965, Some(ticks::DOOMSDAY_TICK)) => {
                    let body = body.unwrap_or_default();
                    let mut seats: Vec<ticks::CastActorState> = self.cast_seat_row();
                    let sweep_arm = ctx.phase;
                    let rolls = self.sweep_status_rolls(&ctx, &seats, body, caster_slot);
                    self.refresh_seat_spirit(&mut seats);
                    // The module's own baked power, rolled per seat: these
                    // three write `+0x14C` themselves, so they own the
                    // outcome and `fold_pending_cast` skips the generic fold
                    // for them (see `sweep_status_rolls`).
                    let take = |seat: u8| {
                        rolls
                            .iter()
                            .find(|(s, _, _)| *s == seat)
                            .map(|(_, d, _)| *d)
                            .unwrap_or(0)
                    };
                    let status = |seat: u8| {
                        rolls
                            .iter()
                            .find(|(s, _, _)| *s == seat)
                            .map(|(_, _, st)| *st)
                            .unwrap_or((1, 1))
                    };
                    let (step, hits) = match body {
                        ticks::CHAOS_BREATH_TICK => ticks::chaos_breath_tick(
                            &mut ctx,
                            &mut caster,
                            &mut seats,
                            take,
                            status,
                        ),
                        ticks::MYSTIC_CIRCLE_TICK => ticks::mystic_circle_tick(
                            &mut ctx,
                            &mut seats,
                            sweep_arm == ticks::MYSTIC_CIRCLE_SWEEP_ARM,
                            take,
                        ),
                        _ => ticks::doomsday_tick(
                            &mut ctx,
                            &mut seats,
                            sweep_arm == ticks::DOOMSDAY_SWEEP_ARM,
                            take,
                        ),
                    };
                    self.write_cast_seat_row(&seats);
                    run.aoe_hits = self.cast_hits_to_engine(hits.iter().map(|h| ticks::AoeHit {
                        seat: h.seat,
                        applied: h.applied as i32,
                    }));
                    Some(step)
                }
                (951, Some(ticks::CHAOS_FLARE_TICK)) => {
                    Some(ticks::chaos_flare_tick(&mut ctx, &mut victim, None))
                }
                (951, Some(ticks::SCYTHE_WIND_TICK)) => Some(ticks::scythe_wind_tick(
                    &mut ctx,
                    &mut caster,
                    &mut victim,
                    None,
                )),
                (952, Some(ticks::BLOODY_HORNS_TICK)) => Some(ticks::bloody_horns_tick(
                    &mut ctx,
                    &mut caster,
                    &mut victim,
                    None,
                )),
                // PROT 0955's six-spell cell. Its four status / buff bodies
                // write simulation state no damage fold can express, so they
                // run here and their outcome is reported back on the run.
                (955, Some(ticks::WHITE_SHIELD_TICK)) => {
                    // Retail reads the caster's own monster **record** at
                    // `0x801C9348[seat - 3]` rather than the live actor, so
                    // the buff is idempotent; the engine's nearest equivalent
                    // is the un-buffed defence split it seeded at battle load.
                    let record = (caster.udf_base, caster.ldf_base);
                    Some(ticks::white_shield_tick(&mut ctx, &mut caster, record))
                }
                (955, Some(ticks::KISS_OF_DEATH_TICK)) => {
                    // The body's one `rand()` is arm 4's, on the pass its
                    // gate opens (`0x801F8CBC`); the other arms draw none.
                    let roll =
                        (ctx.phase == ticks::KISS_OF_DEATH_ROLL_ARM).then(|| self.next_rand());
                    let (step, refund) = ticks::kiss_of_death_tick(&mut ctx, &mut victim, roll);
                    run.item_refund = refund;
                    // The hit leg throws its literal `1` over the victim:
                    // `FUN_801F44A0(1, seat)` at `0x801F8D6C`, behind the
                    // same Stone guard (`0x801F8D60`) the HP write takes.
                    // REF: FUN_801F44A0
                    if roll.is_some_and(|r| r & 1 == 0)
                        && victim.flags & ticks::FLAG_NON_TARGETABLE == 0
                    {
                        self.battle
                            .hit_fx
                            .push(legaia_engine_battle::battle_events::BattleHitFx {
                                target_slot: victim_slot,
                                amount: 1,
                                is_heal: false,
                                is_crit: false,
                            });
                        let c = &mut self.battle_ctx;
                        c.cast_readout_cursor = (c.cast_readout_cursor + 1) & 7;
                        c.readout_pushes = c.readout_pushes.wrapping_add(1);
                    }
                    // Only the odd (miss) leg raises the bar
                    // (`0x801F8CC8` branches the hit leg past the raise).
                    if roll.is_some_and(|r| r & 1 != 0) {
                        self.raise_module_message_bar(entry, ticks::KISS_OF_DEATH_MISS_LINE);
                    }
                    Some(step)
                }
                (955, Some(ticks::MELT_SPRAY_TICK)) => {
                    let debuff = ctx.phase == ticks::MELT_SPRAY_DEBUFF_ARM;
                    Some(ticks::melt_spray_tick(&mut ctx, &mut victim, debuff))
                }
                (955, Some(ticks::TERROR_SCREAM_TICK)) => {
                    let (step, refund) = ticks::terror_scream_tick(&mut ctx, &mut victim);
                    run.item_refund = refund;
                    Some(step)
                }
                (955, Some(ticks::POWER_CHARGE_TICK)) => {
                    Some(ticks::power_charge_tick(&mut ctx, &mut caster))
                }
                (955, Some(ticks::VOID_ACCESSORIES_TICK)) => {
                    // Arm 3's two draws (`0x801F6EB4`, `0x801F6F34`); the second
                    // only behind a filled slot.
                    let rolls = (ctx.phase == 3).then(|| {
                        let slot = self.next_rand();
                        let id = self.cast_victim_accessories(victim_slot)
                            [(slot % u32::from(ticks::ACCESSORY_SLOTS)) as usize];
                        (slot, if id != 0 { self.next_rand() } else { 1 })
                    });
                    let accessories = self.cast_victim_accessories(victim_slot);
                    let (step, outcome) =
                        ticks::void_accessories_tick(&mut ctx, &mut victim, accessories, rolls);
                    run.voided_accessory = outcome;
                    Some(step)
                }
                // PROT 0964's Element Change re-rolls the first monster
                // seat's element. `last_roll` is derived from what the record
                // holds now, which is what the previous commit wrote.
                (964, Some(ticks::ELEMENT_CHANGE_TICK)) => {
                    let mut seats: Vec<ticks::CastActorState> = self.cast_seat_row();
                    let last_roll = self.cast_element_change_last_roll();
                    // Arm 0 draws on demand, once and again while the draw
                    // repeats the last one (`0x801F89F0` / `0x801F8A3C`); a
                    // capture past arm 0 names the roll its arm accepted.
                    let (step, outcome) = match self.battle.element_roll_seed.take() {
                        Some(roll) if ctx.phase == 0 => {
                            ticks::element_change_tick(&mut ctx, &mut seats, u8::MAX, || {
                                u32::from(roll)
                            })
                        }
                        _ => ticks::element_change_tick(&mut ctx, &mut seats, last_roll, || {
                            self.next_rand()
                        }),
                    };
                    self.write_cast_seat_row(&seats);
                    if let Some(out) = outcome {
                        self.battle
                            .monster_ai_state
                            .set_counter(i32::from(out.roll));
                        self.apply_cast_element_change(out.element);
                        run.element_change = Some((out.element, out.group));
                    }
                    Some(step)
                }
                // --- fourteen arms ---
                // PROT 0940 / 0941 / 0943 / 0944 / 0950 / 0956 / 0962, ported
                // in `legaia_engine_cast::cast_arm_ticks`. Three of these bodies
                // wear the VA `0x801F6A04` in three different images, which is
                // why every arm below names its entry.
                (940, Some(arms::GLARE_DIVIDE_BLIND_TICK)) => {
                    // The body works on retail's first monster seat.
                    let first = self
                        .engine_slot_for_retail_pool(
                            legaia_engine_cast::cast_module_ticks::FIRST_MONSTER_SEAT,
                        )
                        .unwrap_or(legaia_engine_cast::cast_module_ticks::FIRST_MONSTER_SEAT);
                    let mut seat = self.cast_view(first);
                    let mut ext = self.cast_arm_ext_state(first);
                    let shield_arm = ctx.phase == arms::MYSTIC_SHIELD_ARM;
                    let step = arms::glare_divide_blind_tick(&mut ctx, &mut seat, &mut ext);
                    self.write_cast_view(first, &seat);
                    self.write_cast_arm_ext_state(first, &ext);
                    if shield_arm && !self.battle.mystic_shield_up() {
                        // `_DAT_8007BD84 = FUN_80021B04(..)` - the shield's
                        // sphere, seated with its texture scroller.
                        self.raise_mystic_shield();
                    }
                    Some(step)
                }
                (940, Some(arms::GLARE_DIVIDE_SPLIT_TICK)) => {
                    let ext = self.cast_arm_ext_state(caster_slot);
                    let saved = self.casting.module_split_saved_target.unwrap_or(0);
                    let (step, split) = arms::glare_divide_split_tick(
                        &mut ctx,
                        &mut caster,
                        &ext,
                        spell_id,
                        saved,
                        || self.next_rand(),
                    );
                    if let Some(sp) = split {
                        self.casting.module_split_saved_target = Some(sp.saved_caster_target);
                        self.apply_glare_divide_split(caster_slot, &sp);
                    }
                    // Arm 1's shake flag eases the caster toward white from
                    // its own colour word, which in retail is the neutral
                    // `0x20080200`. The engine's resting word is `0`, which
                    // under a raised flag reads as retail's zero - a body
                    // the tint pass blacks out and the draw tick skips - so
                    // the shake would start from black.
                    if caster.render_flag == arms::SPLIT_SHAKE_FLAG
                        && let Some(a) = self.actors.get_mut(usize::from(caster_slot))
                        && a.battle.render_color == 0
                    {
                        a.battle.render_color =
                            legaia_engine_battle_vm::battle_formulas::TINT_NEUTRAL;
                    }
                    Some(step)
                }
                (941, Some(arms::STEAL_TICK)) => {
                    let outcome = (ctx.phase == 1)
                        .then(|| self.roll_cast_steal(victim_slot))
                        .flatten();
                    let (step, taken) =
                        arms::steal_tick(&mut ctx, &mut caster, CAST_STEAL_RUN_CLIP, outcome);
                    if let Some(arms::StealOutcome::FromBag { item }) = taken {
                        let _removed = self.take_one_from_bag(item);
                    }
                    if let Some(outcome) = taken {
                        self.stash_cast_steal(caster_slot, outcome);
                        self.raise_cast_steal_caption(outcome);
                    }
                    Some(step)
                }
                (941, Some(arms::STEAL_SWEEP_TICK)) => {
                    let (step, hits) = self.run_cast_arm_sweep(
                        &mut ctx,
                        941,
                        arms::STEAL_SWEEP_TICK,
                        caster_slot,
                        &mut caster,
                    );
                    run.aoe_hits = hits;
                    Some(step)
                }
                (943, Some(arms::CURSE_SINGLE_TICK)) => {
                    Some(arms::curse_single_tick(&mut ctx, &mut caster, &mut victim))
                }
                (943, Some(arms::CURSE_MP_DRAIN_TICK)) => {
                    let mut seats: Vec<ticks::CastActorState> = self.cast_seat_row();
                    let mut exts: Vec<arms::CastArmExtState> = self.cast_arm_ext_row();
                    let (step, _drained) =
                        arms::curse_mp_drain_tick(&mut ctx, &mut seats, &mut exts);
                    self.write_cast_seat_row(&seats);
                    self.write_cast_arm_ext_row(&exts);
                    Some(step)
                }
                (944, Some(arms::GUILTY_CROSS_TICK)) => {
                    // Arm 0 rolled with its line; once it lands, the band's
                    // generic fold owes nothing.
                    let hit = if ctx.phase == 3 {
                        self.casting.module_cam.latched_hit.take()
                    } else {
                        None
                    };
                    if let Some(roll) = hit {
                        self.casting.module_skips_fold = true;
                        // Arm 3's popup, `FUN_801F44A0` on the latched roll
                        // (`jal` at `0x801F7198`) ahead of the clamp, the
                        // `+0x10` accumulate (`0x801F71E0`) and the HP write
                        // (`0x801F71FC`): the numeral over the victim and
                        // the readout's `DAMAGE` line.
                        self.battle
                            .hit_fx
                            .push(legaia_engine_battle::battle_events::BattleHitFx {
                                target_slot: victim_slot,
                                amount: roll.clamp(0, i32::from(u16::MAX)) as u16,
                                is_heal: false,
                                is_crit: false,
                            });
                    }
                    Some(arms::guilty_cross_tick(
                        &mut ctx,
                        &mut caster,
                        &mut victim,
                        hit,
                    ))
                }
                (944, Some(arms::GUILTY_CROSS_CURSE_TICK)) => {
                    let code = caster.target_code;
                    let mut seats: Vec<ticks::CastActorState> = self.cast_seat_row();
                    let (step, _marked) =
                        arms::guilty_cross_curse_tick(&mut ctx, code, &mut caster, &mut seats);
                    self.write_cast_seat_row(&seats);
                    Some(step)
                }
                (950, Some(arms::ROLLING_FLARE_TICK)) => Some(arms::rolling_flare_tick(
                    &mut ctx,
                    &mut caster,
                    &mut victim,
                    None,
                )),
                (950, Some(arms::ROLLING_FLARE_SWEEP_TICK)) => {
                    let sweep = ctx.phase
                        == arms::arm_sweep_arm(950, arms::ROLLING_FLARE_SWEEP_TICK).unwrap_or(0xFF);
                    let rolls = self.cast_arm_sweep_rolls(
                        &ctx,
                        950,
                        arms::ROLLING_FLARE_SWEEP_TICK,
                        caster_slot,
                    );
                    let mut seats: Vec<ticks::CastActorState> = self.cast_seat_row();
                    let take = |seat: u8| {
                        rolls
                            .iter()
                            .find(|(s, _)| *s == seat)
                            .map(|(_, d)| *d)
                            .unwrap_or(0)
                    };
                    let (step, hits) = arms::rolling_flare_sweep_tick(
                        &mut ctx,
                        &mut caster,
                        &mut seats,
                        sweep,
                        take,
                    );
                    self.write_cast_seat_row(&seats);
                    run.aoe_hits = self.cast_hits_to_engine(hits.iter().map(|h| ticks::AoeHit {
                        seat: h.seat,
                        applied: h.applied as i32,
                    }));
                    Some(step)
                }
                (956, Some(arms::WATER_HAZARD_TICK)) => {
                    let code = caster.target_code;
                    let mut seats: Vec<ticks::CastActorState> = self.cast_seat_row();
                    // Each hit seat's damage and then its status draw, seat by
                    // seat, as retail's sweep draws them, on either leg. The
                    // kernel names retail seats; the roll takes engine slots.
                    let shape = arms::arm_damage_shape_for(956, arms::WATER_HAZARD_TICK);
                    let map = self.cast_seat_map();
                    let (step, hits) = arms::water_hazard_tick(
                        &mut ctx,
                        r_caster as u8,
                        code,
                        &mut caster,
                        &mut seats,
                        |seat| {
                            let damage = map
                                .engine(seat)
                                .zip(shape)
                                .and_then(|(e, sh)| self.capture_module_roll(sh, caster_slot, e))
                                .unwrap_or(0);
                            (damage, self.next_rand())
                        },
                    );
                    self.write_cast_seat_row(&seats);
                    run.aoe_hits = self.cast_hits_to_engine(hits.iter().map(|h| ticks::AoeHit {
                        seat: h.seat,
                        applied: h.applied as i32,
                    }));
                    Some(step)
                }
                // `arrived = true` on all three: the band's approach leg
                // (`CasterStagePhase::Approach`, gated on
                // `capture_body_approaches`) walks the caster in and holds
                // the module until the metric reads zero, so by the time
                // this body runs its poll would read arrival.
                //
                // Retail's predicate is `FUN_8004E2F0(ctx[+0x13],
                // caster[+0x1DD]) == 0` - the **zero** side, not the non-zero
                // one: `sll v0,v0,0x10; bne v0,zero,<hold>` at
                // `0x801F7C0C` / `0x801F7D08` sends a non-zero metric to the
                // "stage the run clip and hold" arm, so the metric reads
                // "still out of reach" and zero is arrival. The engine has
                // that metric (`Self::battle_range_metric`, the port of the
                // same routine) and the 0x20 `FUN_80050BB8` calls the arrived
                // path makes are ported too
                // (`legaia_engine_battle_vm::battle_separation`, a pairwise
                // separation nudge - NOT the approach itself).
                //
                // The walk itself is the band's approach leg: the body's arm
                // `0` stages the walk entry and turns the caster onto the
                // victim, and the walk clip's own root motion carries it.
                (962, Some(arms::BLADE_BREATH_A_TICK)) => Some(arms::blade_breath_a_tick(
                    &mut ctx,
                    &mut caster,
                    &mut victim,
                    true,
                    None,
                )),
                (962, Some(arms::BLADE_BREATH_B_TICK)) => Some(arms::blade_breath_b_tick(
                    &mut ctx,
                    &mut caster,
                    &mut victim,
                    true,
                    None,
                )),
                (962, Some(arms::BLADE_BREATH_C_TICK)) => Some(arms::blade_breath_c_tick(
                    &mut ctx,
                    &mut caster,
                    &mut victim,
                    true,
                    None,
                )),
                // The phase-chain bodies (`cast_module_ticks::chain_bodies`):
                // PROT 0942 `0xAA`, 0956 `0x75`, 0959, 0960 `0xA6`, 0961,
                // 0963 and 0964 `0xB0..=0xB2`. Their hits are the fold's.
                (e, Some(b)) if ticks::chain_body_for(e, b).is_some() => {
                    let chain = ticks::chain_body_for(e, b).expect("guarded");
                    let selector = if e == 964 {
                        self.cast_element_change_last_roll()
                    } else {
                        0
                    };
                    use super::super::presentation::{PARALYZING_WAVE_MARK, PARALYZING_WAVE_TICK};
                    let marked_before = victim.flags & PARALYZING_WAVE_MARK != 0;
                    let t = ticks::run_chain_body(
                        chain,
                        &mut ctx,
                        &mut caster,
                        &mut victim,
                        &mut seat,
                        selector,
                    );
                    run.item_refund = t.refund;
                    if (e, b) == (956, PARALYZING_WAVE_TICK)
                        && !marked_before
                        && victim.flags & PARALYZING_WAVE_MARK != 0
                    {
                        self.raise_paralyzing_wave_caption(victim_slot);
                    }
                    Some(t.step)
                }
                // Every ported trampoline arm the band names. An arm whose
                // body has no port ticks nothing, which is exactly what
                // retail's fall-through does for an id the trampoline does
                // not name.
                _ => None,
            }
        } else {
            match entry {
                // --- player Seru 0903..0908 ---
                // The first six `0x801CF4EC` arms - the player Seru-magic tick
                // bodies (`legaia_engine_cast::seru_magic_ticks`). Unlike the
                // summon-creature ticks below, five of the six sweep or
                // retarget seats other than the caster's own three, so they
                // take the whole seat row and the run writes it back before
                // the caster / victim / summon views are refreshed from it.
                //
                // No damage roll is fed in: the engine folds a cast's HP
                // outcome once at `cast_spell_on_slots_prepaid`, so the bodies
                // below take `None`.
                //
                // PROT 0907's kill / confuse fork is the exception, because it
                // is not a damage roll at all - it writes the victim's HP to
                // zero or `+0x16E |= 0x380` and there is no magnitude for the
                // fold to carry. Its verdict is drawn here, once per cast
                // (`Self::nighto_verdict`), and held on the cast state for
                // every later frame, mirroring retail's arm-0 draw into the
                // module words `0x801F8534` / `0x801F853C`.
                903..=908 => {
                    // PROT 0904's ring sweep: advance the ray, then resolve
                    // the cone once for this tick (both borrow `self`, which
                    // the seat row below does not allow).
                    // Retail tests once a pass, four vsyncs of `0x40` apart: from
                    // `0x100` on, so the ray's first swing past the victim
                    // misses and the hit waits for the centre crossing at
                    // `0x800`, 32 vsyncs in. The pass ramps the word first
                    // (`0x801F7ADC..0x801F7AF0`) and builds its ray from the
                    // ramped word, so the test is on the word this tick's ramp
                    // reaches: read before it, the `0x800` crossing struck a
                    // pass late.
                    let cone_seats: Vec<u8> = if entry == 904
                        && ctx.phase == ticks_a::THEEDER_SWEEP_ARM
                        && ticks_a::theeder_pass_tested(ctx.ctx_6d8.wrapping_add(
                            ticks_a::theeder_ramp_per_tick(ticks_a::THEEDER_SWEEP_ARM),
                        )) {
                        use legaia_engine_cast::seru_magic_ticks::{
                            MONSTER_ROW_END, THEEDER_CONE_HALF_WIDTH,
                        };
                        // The ray the sweep arm tests this tick: from the
                        // beam root ahead of the summon seat to the tip the
                        // word `ctx+0x6D8` (after its own ramp) swings about
                        // the summon's facing.
                        let (mouth, tip) = self.theeder_ray(seat_slot, ctx.ctx_6d8);
                        let bearing = legaia_engine_battle_vm::battle_action::bearing_12bit_approx(
                            mouth[2], mouth[0], tip[2], tip[0],
                        );
                        self.seats_in_cone(
                            (mouth[0], mouth[2]),
                            bearing,
                            THEEDER_CONE_HALF_WIDTH,
                            // Retail's row `3..7`, in engine slots.
                            self.engine_slot_for_retail_pool(ticks::FIRST_MONSTER_SEAT)
                                .unwrap_or(ticks::FIRST_MONSTER_SEAT)
                                ..self
                                    .engine_slot_for_retail_pool(MONSTER_ROW_END)
                                    .unwrap_or(MONSTER_ROW_END),
                        )
                        .into_iter()
                        // ...and back into the kernel's row.
                        .map(|e| map.retail(e))
                        .collect()
                    } else {
                        Vec::new()
                    };
                    let nighto_outcome = if entry == 907 {
                        // The body holds no damage call, so a party's cast
                        // folds nothing: retail's Gobu Gobu keeps its 76 HP
                        // through a confused Nighto
                        // (`party_basic_attack_vs_gobu_gobu`, cast 0x85).
                        self.casting.module_skips_fold |= caster_slot < self.party.party_count;
                        self.nighto_verdict(caster_slot, victim_slot, spell_id)
                    } else {
                        ticks_a::NightoOutcome::ConfuseResisted
                    };
                    let who = ticks_a::SeruSeats {
                        caster: r_caster as u8,
                        victim: r_victim as u8,
                        summon: r_seat as u8,
                    };
                    let mut seats: Vec<ticks::CastActorState> = self.cast_seat_row();
                    // Carry the three views into the row so a stager that ran
                    // above this match is not thrown away.
                    for (slot, view) in [(r_caster, caster), (r_victim, victim), (r_seat, seat)] {
                        if let Some(s) = seats.get_mut(slot) {
                            *s = view;
                        }
                    }
                    let (step, hits) = match entry {
                        903 => ticks_a::gimard_tick(&mut ctx, &mut seats, who, None),
                        904 => {
                            // Arm 12's cone gate is the host's: retail reads
                            // the ring's rim bearing and every seat's, and the
                            // module only ever sees "in" or "out". Each seat
                            // the ray crosses takes the arm's own hit
                            // (`FUN_801DD0AC(0x11, 7, seat)` at `0x801F7D38`):
                            // a party cast rolls it here and folds nothing, so
                            // every monster the sweep reaches is struck, as
                            // retail's are (`theeder_summon_mid_cast`: seats 3,
                            // 5 and 4 at vsyncs 783, 791 and 819). A monster's
                            // cast of the id has no roll and keeps the fold.
                            let in_cone = cone_seats.clone();
                            let party_cast = caster_slot < self.party.party_count;
                            self.casting.module_skips_fold |= party_cast;
                            let mut rolled: Vec<(u8, u32)> = Vec::new();
                            // Arm 4 seats the creature between the caster and
                            // the victim, facing the victim (`0x801F6EEC`).
                            // No later arm writes the seat's position, so it
                            // stays pinned there for the rest of the cast.
                            if ctx.phase == ticks_a::THEEDER_RISE_ARM {
                                self.casting.module_theeder.seat =
                                    self.theeder_seat_for(caster_slot, victim_slot);
                            }
                            if let Some(seat) = self.casting.module_theeder.seat {
                                self.pin_theeder_seat(seat_slot, seat);
                            }
                            let geom = self.theeder_geom(seat_slot);
                            let mut fx = self.casting.module_theeder;
                            let attack_in = ctx.phase == ticks_a::THEEDER_ATTACK_ARM;
                            let mut struck = false;
                            let tick = ticks_a::theeder_tick(
                                &mut ctx,
                                &mut seats,
                                who,
                                geom,
                                &mut fx,
                                |seat| {
                                    if !in_cone.contains(&seat) {
                                        return None;
                                    }
                                    struck = true;
                                    if !party_cast {
                                        return Some(0);
                                    }
                                    let t = map.engine(seat)?;
                                    let roll = self.summon_module_roll(caster_slot, t, spell_id)?;
                                    rolled.push((t, roll));
                                    Some(roll as i32)
                                },
                            );
                            for (t, amount) in rolled {
                                self.battle.hit_fx.push(
                                    legaia_engine_battle::battle_events::BattleHitFx {
                                        target_slot: t,
                                        amount: amount.min(u32::from(u16::MAX)) as u16,
                                        is_heal: false,
                                        is_crit: false,
                                    },
                                );
                            }
                            // Each arm-12 hit sets the effect pool's sprite
                            // scale to `0x800` (`0x801F7DC8..0x801F7DD4`),
                            // which sizes the sparks its impact selector `2`
                            // sheds.
                            if struck {
                                self.effect_pool.head.sprite_scale =
                                    legaia_engine_cast::cast_module_ticks::THEEDER_HIT_SPRITE_SCALE;
                            }
                            self.casting.module_theeder = fx;
                            // Arm 7's shot is armed on the pass its clip wait
                            // lets through (`0x801F7274`).
                            if attack_in
                                && ctx.phase != ticks_a::THEEDER_ATTACK_ARM
                                && let Some(c) = self.casting.module_cam.creature
                            {
                                run.camera_shot = Some(
                                    legaia_engine_cast::cast_module_camera::theeder_attack_shot(c),
                                );
                                // ...and its two records at the mouth, on
                                // the creature's angles (`0x801F727C..`).
                                run.spawns =
                                    &legaia_engine_cast::cast_module_camera::THEEDER_ATTACK_SPAWNS;
                                run.arm_point = Some((
                                    ticks_a::theeder_mouth(c.x, c.z, c.facing),
                                    [0, c.facing as i16, 0],
                                ));
                            }
                            tick
                        }
                        905 => {
                            let restore = self.vera_restore(caster_slot, victim_slot, spell_id);
                            let (step, _) = ticks_a::vera_tick(&mut ctx, &mut seats, who, restore);
                            (step, Vec::new())
                        }
                        906 => ticks_a::gizam_tick(&mut ctx, &mut seats, who, |_| None),
                        907 => (
                            ticks_a::nighto_tick(
                                &mut ctx,
                                &mut seats,
                                who,
                                nighto_outcome,
                                self.casting.module_cam.countdown.0 < 0,
                            ),
                            Vec::new(),
                        ),
                        _ => {
                            // Arm 11's sites are the cast's second and last
                            // hits: three quarters of a roll on the victim,
                            // two thirds of one on each other seat
                            // (`0x801F7C14`, `0x801F7DF8`). The opener is arm
                            // 8's `apply_summon_pre_hits`, and the band folds
                            // nothing for a party's cast. A monster's cast of
                            // the id, which has no roll here, keeps the fold.
                            let finishing = ctx.phase == ticks_a::ZENOIR_FINISH_ARM
                                && caster_slot < self.party.party_count;
                            self.casting.module_skips_fold |= caster_slot < self.party.party_count;
                            // Arm 10's show loop: the seats within reach of
                            // the creature, which stands on the victim's body
                            // pair (`0x801F79E8..0x801F7A7C`), each seat's
                            // ground pair against it.
                            let reach: Vec<u8> = if ctx.phase == ticks_a::ZENOIR_BUMP_ARM {
                                let creature = map
                                    .engine(who.victim)
                                    .and_then(|v| self.live_body_pair(usize::from(v)));
                                (ticks::FIRST_MONSTER_SEAT..ticks_a::MONSTER_ROW_END)
                                    .filter(|&r| {
                                        let ground = map.engine(r).and_then(|t| {
                                            let a = self.actors.get(usize::from(t))?;
                                            Some((a.move_state.world_x, a.move_state.world_z))
                                        });
                                        matches!((creature, ground), (Some(c), Some(g))
                                            if ticks_a::zenoir_bump_in_reach(c, g))
                                    })
                                    .collect()
                            } else {
                                Vec::new()
                            };
                            let mut rolled: Vec<(u8, u32)> = Vec::new();
                            let out = ticks_a::zenoir_tick(
                                &mut ctx,
                                &mut seats,
                                who,
                                |r| reach.contains(&r),
                                |r| {
                                    if !finishing {
                                        return None;
                                    }
                                    let t = map.engine(r)?;
                                    let roll = self.summon_module_roll(caster_slot, t, spell_id)?;
                                    let site = if r == who.victim {
                                        ticks_a::SERU_HIT_SITES[4]
                                    } else {
                                        ticks_a::SERU_HIT_SITES[5]
                                    };
                                    rolled.push((t, roll * site.scale_num / site.scale_den));
                                    Some(roll as i32)
                                },
                            );
                            for (t, amount) in rolled {
                                self.battle.hit_fx.push(
                                    legaia_engine_battle::battle_events::BattleHitFx {
                                        target_slot: t,
                                        amount: amount.min(u32::from(u16::MAX)) as u16,
                                        is_heal: false,
                                        is_crit: false,
                                    },
                                );
                            }
                            out
                        }
                    };
                    self.write_cast_seat_row(&seats);
                    caster = seats.get(r_caster).copied().unwrap_or(caster);
                    victim = seats.get(r_victim).copied().unwrap_or(victim);
                    seat = seats.get(r_seat).copied().unwrap_or(seat);
                    run.aoe_hits = self.cast_hits_to_engine(hits.iter().map(|h| ticks::AoeHit {
                        seat: h.seat,
                        applied: h.applied as i32,
                    }));
                    Some(step)
                }
                // The summon band's own tick bodies - no trampoline, the
                // `0x801CF4EC` arm calls them directly.
                918 => ticks::kemaro_tick(&mut ctx, &mut victim, None),
                922 => Some(ticks::puera_tick(&mut ctx, &mut victim, None)),
                924 => {
                    // Arm 0's kill roll, drawn once per cast and held for
                    // arm 10 (the word `0x801F7C4C`).
                    let kill = self.ultimate_rave_verdict();
                    // The body rolls no wrapper: its one write is the kill's
                    // `sh zero, 0x14c`, and off that path the cast hits nothing.
                    self.casting.module_skips_fold = true;
                    Some(ticks::ultimate_rave_tick(
                        &mut ctx,
                        &mut caster,
                        &mut victim,
                        kill,
                    ))
                }
                925 => Some(ticks::spikefish_tick(&mut ctx, &mut caster)),
                927 => Some(ticks::juggernaut_tick(&mut ctx, &mut victim, None)),
                949 => Some(ticks::water_crystals_tick(&mut ctx, &mut victim, None)),
                // --- player Seru 0909..0913 ---
                // These five bodies read and write the whole actor table -
                // the summon seat, the caster and the enemy row - so they
                // take the table rather than the three locals above, and the
                // locals are refreshed from it afterwards.
                //
                // PROT 0909 is the one entry in this band whose move-VM
                // stager also advances `ctx[+0x279]` (its arms `0` and `1`).
                // Retail reaches the stager from the effect script and the
                // tick from `FUN_801F1ED4` - two call sites - while this seam
                // runs both in one call, so the tick is skipped on a frame
                // the stager already stepped the phase. `casting.module_phase`
                // is not written back until the end of this function, so it
                // still holds the phase this call started on.
                909..=913 => {
                    let stager_stepped = ctx.phase != self.casting.module_phase;
                    let mut seats: Vec<ticks::CastActorState> = self.cast_seat_row();
                    let summon = seat_slot;
                    // A frame the stager stepped the phase is still a busy
                    // one: the tick returns `1` from every arm but `0xFF`.
                    let step = if entry == 909 && stager_stepped {
                        Some(ticks::CastTickStep::Busy)
                    } else {
                        let (step, hits) = self.run_seru_b_tick(
                            entry,
                            &mut ctx,
                            &mut seats,
                            caster_slot,
                            summon,
                            victim_slot,
                        );
                        let hits = self.cast_hits_to_engine(hits);
                        run.aoe_hits.extend(hits);
                        Some(step)
                    };
                    if !(entry == 909 && stager_stepped) {
                        self.write_cast_seat_row(&seats);
                        // The three locals are written back below; take them
                        // from the table so this arm's writes survive.
                        caster = seats.get(r_caster).copied().unwrap_or(caster);
                        victim = seats.get(r_victim).copied().unwrap_or(victim);
                        seat = seats.get(r_seat).copied().unwrap_or(seat);
                    }
                    step
                }
                // The phase-chain bodies whose tick arm calls them directly
                // (`cast_module_ticks::chain_bodies`): PROT 0919, 0935, 0936,
                // 0937, 0939, 0947 and 0948. Their hits and heals are the
                // fold's.
                e if ticks::direct_chain_body(e).is_some() => {
                    let chain = ticks::direct_chain_body(e).expect("guarded");
                    let phase_before = ctx.phase;
                    let t = ticks::run_chain_body(
                        chain,
                        &mut ctx,
                        &mut caster,
                        &mut victim,
                        &mut seat,
                        0,
                    );
                    run.item_refund = t.refund;
                    // PROT 0919 (Spoon) arm 7 also runs the party cure ladder
                    // and its tier-4 AP doubling - the half of the arm the
                    // fold does not own (its heal is the fold's). Once, on
                    // the frame the arm lets the phase through.
                    if e == 919
                        && phase_before == legaia_engine_cast::seru_magic_ticks::SPOON_HEAL_ARM
                        && ctx.phase != phase_before
                    {
                        // The views carry this frame's chain writes; seat them
                        // in the row before the sweep reads it.
                        let mut seats: Vec<ticks::CastActorState> = self.cast_seat_row();
                        for (slot, view) in [(r_caster, caster), (r_victim, victim), (r_seat, seat)]
                        {
                            if let Some(s) = seats.get_mut(slot) {
                                *s = view;
                            }
                        }
                        let cleanse = self.cure_selector(
                            caster_slot,
                            spell_id,
                            legaia_engine_cast::seru_magic_ticks::ORB_CLEANSE_MIN_LEVEL,
                        );
                        legaia_engine_cast::seru_magic_ticks::spoon_cure_sweep(&mut seats, cleanse);
                        // ...and raises the roster panels the way Orb's arm 9 does
                        // (`FUN_801D8DE8(6, 0)` and `(0x4E + i, 0)`,
                        // `0x801F83E4..0x801F8450`).
                        self.battle.raise_module_roster_panels();
                        self.write_cast_seat_row(&seats);
                        caster = seats.get(r_caster).copied().unwrap_or(caster);
                        victim = seats.get(r_victim).copied().unwrap_or(victim);
                        seat = seats.get(r_seat).copied().unwrap_or(seat);
                    }
                    Some(t.step)
                }
                _ => None,
            }
        };
        if let Some(step) = step {
            run.busy = step == ticks::CastTickStep::Busy;
            run.tick_ported = true;
        } else if director_body {
            // The director is the body: busy until it reaches `0xFF`.
            run.tick_ported = true;
            run.busy = ctx.phase != ticks_a::SERU_B_DONE_PHASE;
        } else if let Some(arm) = capture_arm {
            // A capture director over a body with no port (a ported body, or
            // a holding arm, has already produced a step) owns its phase.
            run.tick_ported = true;
            run.busy = arm.next.is_some();
            if let Some(next) = arm.next {
                ctx.phase = next;
            }
        }
        // The finishing arm's `ctx[+0x6DA] = 0x780`, which the Done band's
        // case 6 frames from.
        if run.tick_ported
            && !run.busy
            && let Some(yaw) = legaia_engine_cast::cast_module_camera::capture_exit_yaw_base(
                entry,
                if has_trampoline {
                    body.unwrap_or_default()
                } else {
                    legaia_engine_cast::cast_module_camera::SINGLE_BODY
                },
            )
            && let Some(cam) = self.battle.camera.as_mut()
        {
            cam.set_action_yaw_base(yaw);
        }

        // Each view writes back only what the tick changed in it, folded onto
        // the slot's live state: the three views can name one actor (a
        // self-targeted cast's victim is its caster), and a whole-view write
        // of the second would undo the first's stores - PROT 0960's phase-5
        // stage of clip `0x0D` on Lu Delilas, read back as never staged,
        // held the band in `0x70` for good.
        for (slot, view, orig) in [
            (caster_slot, &caster, &caster_orig),
            (victim_slot, &victim, &victim_orig),
            (seat_slot, &seat, &seat_orig),
        ] {
            let mut live = self.cast_view(slot);
            live.fold_writes(orig, view);
            self.write_cast_view(slot, &live);
        }
        // A tick body that seats its creature and stages its clip on one pass
        // (PROT 0910's arm 2, `sb s8,0x1da` / `sb s8,0x1dc` at
        // `0x801F6DB0..0x801F6DB4` right behind the installer): the host seats
        // after this tick and the installer zeroes the clip bytes, so the store
        // waits with the director's for the seat there is
        // (`Self::apply_seru_seat_writes`).
        if self.casting.summon_actor_slot != Some(seat_slot)
            && (self.casting.summon_seat_owed.is_some()
                || self.casting.pending_summon_spawn.is_some())
            && seat.staged_anim != seat_orig.staged_anim
            && seat.staged_anim != 0
        {
            self.casting.module_cam.creature_stage = Some(seat.staged_anim);
            self.casting.module_cam.creature_restage = seat.restage & 1 != 0;
        }
        // PROT 0903's depth stores: arm 10's pass parks the walk-in's
        // `ctx[+0x6D0] = 0x800` (`0x801F7380`), and arm 11's pass - the hit -
        // re-runs the target-size framing `FUN_801F0348` (`jal` at
        // `0x801F75A0`) and, while the victim still stands, scales it by
        // `3/2` toward zero (`0x801F75A8..0x801F75DC`). Arm 12's case-8 calls
        // frame off that depth: a victim the breath killed at the `0x0C00`
        // floor, a living one a half again further out. PROT 0913 (Nova)
        // runs the same framing on arm 13's pass (`jal` at `0x801F7B6C`,
        // unscaled): arms 14 / 15 frame the victim at `d` and `(d/2, -d/2)`
        // and arm 16 lifts it `sin * d >> 14`, all off that depth.
        // PROT 0941's Steal halves the depth for the walk (`0x801F7434`) and
        // puts it back on the steal (`0x801F7B24`); each of the steal's two
        // passing arms rings a monster's own voice through the funnel by the
        // caster's id, `(4..=6, 0x43..=0x45, 0x90..=0x92)`.
        if entry == 941 && body == Some(arms::STEAL_TICK) && ctx.phase != phase_entry {
            match phase_entry {
                0 => {
                    self.battle.camera_frame_height /= 2;
                    self.battle_ctx.camera_frame_height /= 2;
                }
                1 => {
                    self.battle.camera_frame_height *= 2;
                    self.battle_ctx.camera_frame_height *= 2;
                }
                _ => {}
            }
            let id = self
                .actors
                .get(usize::from(caster_slot))
                .and_then(|a| a.battle_monster_id)
                .unwrap_or(0);
            let cue = match (phase_entry, id) {
                (1, 4..=6) => Some(0x1D),
                (1, 0x43..=0x45) => Some(0x137),
                (1, 0x90..=0x92) => Some(0x155),
                (2, 4..=6) => Some(0x1C),
                (2, 0x43..=0x45) => Some(0x138),
                (2, 0x90..=0x92) => Some(0x18B),
                _ => None,
            };
            if let Some(cue) = cue {
                let category = self.retail_actor_category(caster_slot);
                self.route_battle_cue(cue, category);
            }
            if phase_entry == 1 {
                self.steal_back_off(caster_slot, victim_slot);
            }
        }
        if matches!(entry, 903 | 913) && ctx.phase != phase_entry {
            let depth = match (entry, phase_entry) {
                (903, 10) => Some(legaia_engine_cast::cast_module_camera::GIMARD_WALK_DEPTH as i16),
                (903, 11) | (913, 0x0D) => {
                    let h = self.cast_target_frame_depth(caster_slot);
                    let alive = self
                        .actors
                        .get(usize::from(victim_slot))
                        .is_some_and(|a| a.battle.hp != 0);
                    Some(if alive && entry == 903 {
                        ((i32::from(h) * 3) / 2) as i16
                    } else {
                        h
                    })
                }
                _ => None,
            };
            if let Some(d) = depth {
                self.battle.camera_frame_height = d;
                self.battle_ctx.camera_frame_height = d;
            }
            // The same pass turns the victim onto the creature
            // (`jal 0x80019B28` at `0x801F756C`, `sh v0,0x46(s2)`), which is what
            // case 8's dead-target yaw reads through arm 12.
            if (entry, phase_entry) == (903, 11)
                && let Some((cz, cx)) = self
                    .actors
                    .get(usize::from(seat_slot))
                    .map(|c| (c.move_state.world_z, c.move_state.world_x))
                && let Some(v) = self.actors.get_mut(usize::from(victim_slot))
            {
                v.battle.facing_angle =
                    legaia_engine_battle_vm::battle_action::bearing_12bit_approx(
                        v.move_state.world_z,
                        v.move_state.world_x,
                        cz,
                        cx,
                    );
            }
        }
        // PROT 0903's `3/2` above reads the victim's HP after the hit
        // (`0x801F74F8`, `0x801F75A8`), but the band folds that hit behind this
        // pass: arm 12 finds a victim the breath killed and the depth stays on
        // the bare framing.
        if entry == 903
            && phase_entry == 12
            && self
                .actors
                .get(usize::from(victim_slot))
                .is_some_and(|a| a.battle.hp == 0)
        {
            let h = self.cast_target_frame_depth(caster_slot);
            self.battle.camera_frame_height = h;
            self.battle_ctx.camera_frame_height = h;
        }
        // The `0xFF` arm of eight player-Seru modules leaves the creature
        // seat's loop counter clear for the next cast's installer, which
        // seeds none (`sb zero,0x21b` / `sh zero,0x176`; PROT 0904
        // `0x801F80F0`, 0906 `0x801F76E8`, 0907 `0x801F7F68`, 0908
        // `0x801F82A4`, 0910 `0x801F7BDC`, 0911 `0x801F7FAC`, 0912
        // `0x801F831C`, 0913 `0x801F8608`).
        if phase_entry == legaia_engine_cast::cast_module_ticks::CHOREOGRAPHY_DONE_PHASE
            && SERU_EXIT_LOOP_CLEARS.contains(&entry)
            && let Some(a) = self.actors.get_mut(usize::from(seat_slot))
        {
            a.battle.hit_count_bound = 0;
            if let Some(p) = a.battle_animation.as_mut() {
                p.release_loop_window();
            }
        }
        // PROT 0903's settle arm breaks the creature up on every pass: `+0x1BA += 2 *
        // frame * scalar` on seat 7 (`0x801F76A4..0x801F76C4`, past the
        // countdown's jump-in like the rate store), the dissolve the settle
        // plays while the victim's knock-back runs out.
        if entry == 903
            && phase_in == ticks_a::GIMARD_SETTLE_ARM
            && let Some(a) = self.actors.get_mut(usize::from(seat_slot))
        {
            a.battle.shatter = a
                .battle
                .shatter
                .wrapping_add(ticks_a::GIMARD_SHATTER_STEP_PER_TICK);
        }
        // PROT 0925's outcome arm breaks the creature up on every pass,
        // held or not, ahead of its countdown gate.
        if entry == SPIKEFISH_ENTRY
            && phase_in == legaia_engine_cast::cast_module_ticks::SPIKEFISH_OUTCOME_ARM
            && let Some(a) = self.actors.get_mut(usize::from(seat_slot))
        {
            a.battle.shatter = a.battle.shatter.wrapping_add(
                legaia_engine_cast::cast_module_ticks::SPIKEFISH_SHATTER_STEP_PER_TICK,
            );
        }
        // PROT 0908's arm 12 shatters the monsters the finisher killed, on
        // every pass of the arm, held or not (the loop sits ahead of the
        // countdown gate the director runs). Both writes land after the
        // views' fold-back above, which would otherwise restore the word
        // from a view read before them.
        if entry == 908 && phase_in == ticks_a::ZENOIR_SHATTER_ARM {
            let latched = self.casting.module_cam.victim_slot.unwrap_or(victim_slot);
            self.zenoir_shatter(latched);
        }
        // PROT 0964's strike `0xFF` arm flushes the pool (`0x801F8854`) and
        // shows the caster again (`sb zero,0x21c(s7)` at `0x801F88B4`).
        if entry == 964
            && phase_entry == legaia_engine_cast::cast_module_ticks::CHOREOGRAPHY_DONE_PHASE
            && (0xB0..=0xB2).contains(&spell_id)
        {
            self.apply_capture_seat_writes(
                caster_slot,
                legaia_engine_cast::cast_module_camera::CaptureSeatWrite {
                    render_flag: Some(0),
                    ..Default::default()
                },
            );
            self.casting.flush_module_part_pool();
        }
        // PROT 0942 `0xAA`'s party swirl, every pass of its arms 2..=5.
        if entry == 942 && spell_id == 0xAA && capture_arm.is_some() {
            self.dark_typhoon_party_swirl(caster_slot, phase_in, !capture_held);
        }
        // A capture arm's streak-ribbon calls are per pass: this tick's, or
        // none.
        self.battle.cast_ribbons = pass_ribbons;
        self.casting.module_ribbons_on_scatter_y =
            capture_arm.as_ref().is_some_and(|a| a.ribbons_on_scatter_y);
        if std::mem::take(&mut self.casting.module_storm_dealt) {
            self.deal_storm_ribbons(false);
            self.casting.module_ribbons_on_scatter_y = false;
        }
        // A capture director's seat stores (PROT 0959's stand-ins, heights
        // and hides), after the fold-back for the same reason.
        if let Some(arm) = capture_arm {
            if arm.element_stream
                && let Some(readef) = self.tables.readef_dat.clone()
            {
                let roll = self.battle.monster_ai_state.counter().clamp(0, 2) as u8;
                crate::battle_sideband_textures::write_readef_group_textures(
                    &readef,
                    legaia_actor_formats::summon_readef::element_strike_group(roll),
                    &mut self.battle.vram_loads,
                );
            }
            if arm.flushes_pool {
                self.casting.flush_module_part_pool();
            }
            if let Some(w) = arm.part_write
                && let Some(scene) = self.casting.active_summon.as_mut()
            {
                for part in scene
                    .parts
                    .iter_mut()
                    .filter(|p| !p.finished && p.tag.is_some_and(|t| w.records.contains(&t)))
                {
                    let x = w.x.unwrap_or(part.state.world_x.wrapping_add(w.dx));
                    let y = w.y.unwrap_or(part.state.world_y.wrapping_add(w.dy));
                    part.poke(0x14, x as u16);
                    part.poke(0x16, y as u16);
                    if let Some(c) = w.colours {
                        part.state.set_actor_u32(0xA0, c);
                        part.state.set_actor_u32(0xA4, c);
                    }
                    if w.spin_84_shl != 0 {
                        part.state.anim_84 =
                            part.state.anim_84.wrapping_shl(u32::from(w.spin_84_shl));
                    }
                    if let Some(r) = w.level_rate {
                        part.state.tween_src_x = r;
                    }
                }
            }
            if arm.hides_creature {
                self.despawn_summon_actor();
            }
            if arm.seats_creature
                && let Some(c) = self.actors.get(usize::from(caster_slot))
            {
                let m = &c.move_state;
                let at = [m.world_x, m.world_y, m.world_z];
                self.casting.module_cam.creature =
                    Some(legaia_engine_cast::cast_module_camera::ModuleSeat {
                        x: at[0],
                        y: at[1],
                        z: at[2],
                        facing: c.battle.facing_angle & 0xFFF,
                    });
                self.casting.pending_summon_spawn = Some((spell_id, at));
            }
            if !arm.others_write.is_empty() && !capture_held {
                let monsters = self.party.party_count..self.actors.len() as u8;
                for slot in 0..self.party.party_count {
                    if !arm.others_live_only || self.actors[usize::from(slot)].battle.hp > 0 {
                        self.apply_capture_seat_writes(slot, arm.others_write);
                    }
                }
                for slot in monsters {
                    if self.actors[usize::from(slot)].battle.hp > 0 {
                        self.apply_capture_seat_writes(slot, arm.others_write);
                    }
                }
            }
            self.apply_capture_seat_writes(caster_slot, arm.caster_write);
            self.apply_capture_seat_writes(victim_slot, arm.victim_write);
            if capture_held {
                self.casting.module_held_moves =
                    Some(legaia_engine_battle_loop::cast_fx_state::ModulePassMoves {
                        phase: phase_in,
                        seats: [
                            Self::seat_move_of(caster_slot, &arm.caster_write),
                            Self::seat_move_of(victim_slot, &arm.victim_write),
                        ],
                        focus_z: arm.focus_z,
                    });
            }
            if !arm.party_write.is_empty() {
                for slot in 0..self.party.party_count {
                    let a = &self.actors[usize::from(slot)];
                    if (!arm.party_live_only || a.battle.hp > 0)
                        && !(arm.party_skip_stoned && self.battle.actor_is_petrified(slot))
                        && !(arm.party_skip_victim && slot == victim_slot)
                    {
                        self.apply_capture_seat_writes(slot, arm.party_write);
                    }
                }
            }
            // PROT 0938's breath pushes the whole party row back, a seat at a
            // time along its bearing from the caster, on every pass of arm 1.
            if let Some(units) = arm.party_shove {
                let (cx, cz) = self
                    .actors
                    .get(usize::from(caster_slot))
                    .map_or((0, 0), |a| (a.move_state.world_x, a.move_state.world_z));
                for slot in 0..self.party.party_count {
                    let Some(a) = self.actors.get_mut(usize::from(slot)) else {
                        continue;
                    };
                    let heading = legaia_engine_battle_vm::battle_action::bearing_12bit_approx(
                        cz,
                        cx,
                        a.move_state.world_z,
                        a.move_state.world_x,
                    )
                    .wrapping_add(0x800)
                        & 0xFFF;
                    let (sin, cos) = legaia_engine_system::trig::trig12(heading);
                    // Retail's `2 * step` a frame is `2` a vsync here.
                    let by = i32::from(units);
                    let dx = ((i32::from(sin) * by) >> 12) as i16;
                    let dz = ((i32::from(cos) * by) >> 12) as i16;
                    a.move_state.world_x = a.move_state.world_x.wrapping_add(dx);
                    a.move_state.world_z = a.move_state.world_z.wrapping_add(dz);
                    if let Some((bx, bz)) = a.battle.seat {
                        a.battle.seat = Some((bx.wrapping_add(dx), bz.wrapping_add(dz)));
                    }
                }
            }
            if let Some(level) = arm.parts_level
                && let Some(scene) = self.casting.active_summon.as_mut()
            {
                for part in &mut scene.parts {
                    part.state.field_78 = level;
                }
            }
            // PROT 0965 arm `0x0B` lines the party up for its sweep and its
            // `0xFF` arm puts each seat back (`0x801F76E4..0x801F7790`,
            // `0x801F7A30..0x801F7A7C`).
            if arm.party_line_up {
                self.line_up_party_for_sweep();
            }
            if entry == 965 && phase_in == ticks::CHOREOGRAPHY_DONE_PHASE {
                self.restore_lined_up_party();
            }
            if arm.backdrop_on {
                self.battle.backdrop_cue = 0x800;
            }
            // A director that lands the module's wrapper hits itself (PROT
            // 0959's three sites, arm `0x0F`'s many times over): each is
            // `FUN_801DD6B4(power, ctx[+0x13], 0)` aimed at `0x801C9370[0]`,
            // its popup pushed with the raw roll (`jal 0x801F44A0` ahead of
            // the clamp), then the shape-A clamp and the HP write. The fold
            // owes the cast nothing.
            if director_lands_hits(entry) {
                self.casting.module_skips_fold = true;
            }
            // A director that paces a split cast's hits names each as it
            // lands (PROT 0937's arm 7); the band rolls and applies it.
            if let Some(hit) = arm.split_hit
                && let Some(hits) = ticks::split_hits_for(
                    entry,
                    body.unwrap_or(legaia_engine_cast::cast_module_camera::SINGLE_BODY),
                )
            {
                self.land_split_hit(hits, hit, caster_slot, victim_slot);
                // Hits short of the last clear the readout flag
                // `0x8007B64C` beside their `HP - 1` cap: the cluster opens
                // on the last.
                self.battle.combo_cluster_held = hit < hits.spares_below;
            }
            if let Some(site) = arm.hit_site
                && let Some(shape) = legaia_engine_cast::cast_module_ticks::damage_shape_for(entry)
                && let Some(roll) =
                    self.capture_module_roll_site(shape, usize::from(site), caster_slot, 0)
            {
                // The shape-A clamp and the HP / bar writes. Arms 7 and `0x0D`
                // go through the safe-clamp hit arm the fold lands a cast with.
                // Only arm 7 stages a reaction (`+0x1F1` at `0x801F7278`): the
                // body's other `+0x1DA` stores are arm `0x0A`, `0x0F` and `0x11`,
                // so arm `0x0D` lands on a victim still parked in its get-up
                // clip, and arm `0x0F`'s barrage restages nothing.
                // PROT 0958's six sites are all the bare clamp: each arm
                // stages the victim's knockdown itself beside its call.
                if entry == 959 && site < 2 {
                    self.apply_effect_child_hit(0, roll.max(0));
                    if site == 0 && roll > 0 {
                        let survives = self.actors.first().is_some_and(|a| a.battle.hp > 0);
                        self.queue_battle_reaction(0, survives);
                    }
                } else {
                    let orig = self.cast_view(0);
                    let mut view = orig;
                    ticks::apply_hit_floor_zero(&mut view, roll);
                    let mut live = self.cast_view(0);
                    live.fold_writes(&orig, &view);
                    self.write_cast_view(0, &live);
                }
                self.battle
                    .hit_fx
                    .push(legaia_engine_battle::battle_events::BattleHitFx {
                        target_slot: 0,
                        amount: roll.clamp(0, i32::from(u16::MAX)) as u16,
                        is_heal: false,
                        is_crit: false,
                    });
            }
            // PROT 0959's hits clear the readout flag and its arm `0x0F`
            // raises it as it passes: the cluster opens on that pass.
            if entry == 959 {
                self.battle.combo_cluster_held = phase_in < 0x0F || (phase_in == 0x0F && arm.hold);
            }
            // PROT 0960's Plasma Strike burst clears it beside its HP write
            // (`sb zero,-0x49B4` at `0x801F81BC`) and arm `0x0F` raises it
            // as it passes (`li v0,0x78` / `sb` at `0x801F84B8..0x801F84BC`):
            // the burst's numeral goes up alone and the cluster opens on that
            // pass. No earlier arm raises it, and the flurry's banked hits
            // write neither the flag nor HP.
            if entry == 960
                && body == Some(legaia_engine_cast::cast_module_camera::PLASMA_STRIKE_BODY)
            {
                self.battle.combo_cluster_held = phase_in < 0x0F || (phase_in == 0x0F && arm.hold);
            }
            // PROT 0958's clear the flag the same way and its arm 23 raises
            // it as it passes (`0x801F8ABC..0x801F8ACC`).
            if entry == 958 {
                let readout = legaia_engine_cast::cast_module_camera::BLAZING_SLASH_READOUT_ARM;
                self.battle.combo_cluster_held =
                    phase_in < readout || (phase_in == readout && arm.hold);
            }
            if arm.backdrop_off {
                self.battle.backdrop_cue =
                    legaia_engine_battle_vm::battle_ground_grid::BACKDROP_CUE_FULL;
            }
            if let Some(id) = arm.cue {
                self.dispatch_battle_cue(id);
            }
        }
        // The hit-tint stores: colour word, blend and impact selector.
        self.apply_module_impacts(
            entry,
            body.unwrap_or(legaia_engine_cast::cast_module_camera::SINGLE_BODY),
            phase_in,
            ctx.phase != phase_in,
            countdown_before,
            caster_slot,
            victim_slot,
            seat_slot,
        );
        self.apply_cast_kill_results(
            entry,
            phase_in,
            ctx.phase != phase_in,
            caster_slot,
            victim_slot,
        );
        if let Some(scale) =
            legaia_engine_cast::cast_module_ticks::effect_sprite_scale_store(entry, phase_in)
        {
            self.effect_pool.head.sprite_scale = scale;
        }
        if entry == 908 {
            self.apply_summon_pre_hits(
                entry,
                phase_in,
                ctx.phase != phase_in,
                caster_slot,
                spell_id,
            );
        }
        // The summon-creature directors' HP and colour-word stores, after
        // the views' fold-back like PROT 0908's.
        if direction.is_some() && (914..=934).contains(&entry) {
            self.apply_summon_body_writes(
                entry,
                phase_in,
                ctx.phase != phase_in,
                victim_slot,
                seat_slot,
            );
            if entry == 930 {
                self.apply_horn_writes(
                    phase_in,
                    ctx.phase != phase_in,
                    seat_slot,
                    usize::from(caster_slot),
                );
            }
            if entry == 915 {
                self.apply_mushura_writes(phase_in, victim_slot);
                self.apply_mushura_run(
                    phase_in,
                    ctx.phase != phase_in,
                    ctx.monster_count,
                    victim_slot,
                    seat_slot,
                );
            }
            if entry == 929 {
                self.apply_mule_writes(
                    phase_in,
                    ctx.phase != phase_in,
                    ctx.monster_count,
                    caster_slot,
                    spell_id,
                );
            }
            if entry == 928 {
                self.apply_palma_writes(phase_in, ctx.phase != phase_in);
            }
            if entry == 914 && ctx.phase != phase_in {
                self.apply_gola_gola_writes(phase_in, ctx.monster_count, caster_slot, seat_slot);
            }
            if entry == 919 {
                self.apply_spoon_writes(
                    phase_in,
                    ctx.phase != phase_in,
                    ctx.monster_count,
                    seat_slot,
                );
            }
            if entry == 921 {
                self.apply_iota_writes(
                    phase_in,
                    ctx.phase != phase_in,
                    ctx.monster_count,
                    seat_slot,
                );
            }
            if entry == SPIKEFISH_ENTRY {
                self.apply_spikefish_writes(
                    phase_in,
                    ctx.phase != phase_in,
                    ctx.monster_count,
                    seat_slot,
                );
            }
            if entry == 924 {
                self.apply_rave_writes(
                    phase_in,
                    ctx.phase != phase_in,
                    ctx.monster_count,
                    victim_slot,
                    seat_slot,
                );
            }
            if entry == legaia_engine_cast::cast_module_camera::SLIPPERY_ENTRY {
                self.apply_slippery_writes(phase_in, ctx.phase != phase_in, seat_slot);
            }
            if entry == 931 && phase_in >= 4 {
                // The tail every pass runs (`0x801F89A0..0x801F89F0`): arm 4 saved
                // channels 6 and 22 of the creature's model table, and from clip
                // 4 on channel 6 draws object 22.
                let clip = self
                    .actors
                    .get(usize::from(seat_slot))
                    .map_or(0, |a| a.battle.current_anim);
                self.casting.creature_channel_object = (clip >= 4).then_some((6, 22));
            }
            if entry == 931 {
                self.apply_jedo_writes(
                    phase_in,
                    ctx.phase != phase_in,
                    ctx.monster_count,
                    seat_slot,
                );
            }
            if entry == 932 {
                self.apply_meta_writes(
                    phase_in,
                    ctx.phase != phase_in,
                    ctx.monster_count,
                    seat_slot,
                );
            }
            if entry == 933 {
                self.apply_terra_writes(
                    phase_in,
                    ctx.phase != phase_in,
                    ctx.monster_count,
                    seat_slot,
                );
            }
            if entry == 934 {
                self.apply_ozma_writes(
                    phase_in,
                    ctx.phase != phase_in,
                    ctx.monster_count,
                    seat_slot,
                    caster_slot,
                    spell_id,
                );
            }
            if entry == 923 {
                self.apply_gilium_writes(phase_in, ctx.phase != phase_in);
            }
            self.apply_summon_pre_hits(
                entry,
                phase_in,
                ctx.phase != phase_in,
                caster_slot,
                spell_id,
            );
        }
        // PROT 0948's beam: arm 2 zeroes the builder's counter as it passes,
        // and arm 3 calls the builder (`jal 0x801F726C` at `0x801F6EF4`)
        // ahead of its own gate, so it draws on every tick the arm runs.
        self.casting.module_beam_live = false;
        if entry == legaia_engine_cast::cast_module_ticks::CROSS_BEAM_ENTRY {
            if phase_in == 2 && ctx.phase != phase_in {
                self.casting.module_beam_counter = 0;
            }
            if phase_in == 3 {
                self.casting.module_beam_counter +=
                    legaia_engine_cast::cast_module_ticks::CROSS_BEAM_COUNTER_PER_TICK;
                self.casting.module_beam_live = true;
            }
        }
        // PROT 0920's rain words, written by its arms around the director's
        // gates: arm 0 frees the records and sets the grey colour, arm 5's
        // exit seeds the budget, arm 6 drains it `2 * delta` a pass and pins
        // it at `4` on its exit, arm 9 ramps the colour towards red a step
        // a vsync and pins it on its exit, and `0xFF` clears the budget.
        if entry == legaia_engine_cast::cast_module_camera::SLIPPERY_ENTRY {
            self.casting
                .slippery_rain_words(phase_in, ctx.phase != phase_in);
        }
        // The arm passed: its re-arm of the countdown word.
        if let Some(a) = arm_countdown
            && !capture_held
            && ctx.phase != phase_in
        {
            a.pass(&mut self.casting.module_cam.countdown);
        }
        // PROT 0925's (the Spikefish flute's) outcome arm: on an ordinary
        // fight it stages the party's flee, and on every fight its round
        // tail moves the turn cursor by the living party
        // ([`legaia_engine_cast::cast_module_ticks::spikefish_round_tail`]).
        if entry == SPIKEFISH_ENTRY
            && phase_in == legaia_engine_cast::cast_module_ticks::SPIKEFISH_OUTCOME_ARM
            && ctx.phase != phase_in
        {
            self.spikefish_outcome(&mut ctx);
        }
        if entry == 911 {
            self.orb_arm_writes(phase_in, ctx.phase);
        }
        self.casting
            .late_creature_stager_words(entry, phase_in, ctx.phase != phase_in);
        if entry == 928
            && let Some(v) = legaia_engine_cast::cast_module_camera::palma_stage_byte(
                phase_in,
                self.casting.module_cam.countdown.0,
            )
        {
            ctx.ctx_278 = v;
        }
        if entry == 923
            && ctx.phase != phase_in
            && let Some(v) = legaia_engine_cast::cast_module_camera::gilium_stage_byte(phase_in)
        {
            ctx.ctx_278 = v;
        }
        if entry == 917
            && let Some(v) = legaia_engine_cast::cast_module_camera::barra_stage_byte(
                phase_in,
                ctx.phase != phase_in,
                self.casting.module_cam.countdown.0,
            )
        {
            ctx.ctx_278 = v;
        }
        self.casting.module_ctx_278 = ctx.ctx_278;
        self.casting.module_ring_angle = ctx.ctx_6d8;
        self.casting.module_phase = ctx.phase;
        // The turn-steal arms bump `ctx[+0x1A]`; it is a context byte, so it
        // has to travel back out of the view.
        self.battle_ctx.turn_cursor = ctx.turn_cursor;
        run.phase = ctx.phase;
        run.ctx_278 = ctx.ctx_278;
        // PROT 0955's two arms that reach outside the battle actor: the
        // refunded item goes back in the bag (retail's `FUN_800421D4`), and
        // the voided accessory is cleared out of the character record and
        // handed back (retail's record write plus `FUN_80042558`).
        if let Some(item) = run.item_refund {
            let _ = self.party.inventory.add(item, 1);
        }
        if let Some(out) = run.voided_accessory
            && let Some(id) = out.voided
        {
            let rslot = self.party_roster_slot(victim_slot as usize);
            if let Some(rec) = self.party.roster.members.get_mut(rslot) {
                let mut eq = rec.equipment();
                if let Some(slot) = eq.slots.get_mut(ACCESSORY_EQUIP_SLOT_0 + out.slot as usize) {
                    *slot = 0;
                }
                rec.set_equipment(eq);
            }
            let _ = self.party.inventory.add(id, 1);
            self.refresh_party_ability_bits();
        }
        // The arm's calls into the shared presentation helpers (fades,
        // washes, CLUT copies), decoded off the module image. A guarded site
        // reads the countdown word as this pass opened on it.
        self.casting.module_pres.word_in =
            legaia_engine_cast::cast_module_camera::summon_holds::countdown_word(entry)
                .map(|va| (va, countdown_before));
        self.fire_cast_presentation(
            entry,
            if has_trampoline { body } else { None },
            (phase_entry != phase_in).then_some(phase_entry),
            phase_in,
            ctx.phase,
            run.tick_ported && !run.busy,
        );
        // PROT 0955's Void Accessories composes its line at run time, the
        // strip's `0xC2`-escape template or the module's `Nothing happened.`
        // (`0x801F6F94`, `0x801F7048`); the decoder sees both sites.
        if let Some(out) = run.voided_accessory {
            self.raise_cast_void_caption(out.voided);
        }
        if entry == 929 {
            self.mule_backdrop_follow(phase_in, ctx.phase != phase_in);
        }
        Some(run)
    }
}

impl World {
    /// PROT 0941 Steal's arm-1 way out (`0x801F75F4..0x801F76E4`): the caster
    /// turned to `FUN_80019B28(victim, caster)` - its back to the victim -
    /// and stepped along that heading by its playing clip's stride
    /// (`*(+0x22C)+0x4C` entry `+0xC`) x the frame step `0x1F800393` x
    /// `+0x21D`, `>> 15` per axis (`+0x34` on the sine table `0x8007B81C`,
    /// `+0x38` on the cosine one `0x8007B7F8`), until the range poll
    /// `FUN_8004E2F0` reads it out of reach. One pass, however many steps; a step comes ahead of each poll.
    // PORT: FUN_801F730C (PROT 0941 `0x51` arm 1's back-off, `0x801F75F4..0x801F76E4`)
    fn steal_back_off(&mut self, caster_slot: u8, victim_slot: u8) {
        /// Steps before giving up on a stride that never leaves reach.
        const MAX_STEPS: u32 = 0x400;
        let (c, v) = (usize::from(caster_slot), usize::from(victim_slot));
        if c >= self.actors.len() || v >= self.actors.len() {
            return;
        }
        let (vx, vz) = self.battle_seat_of(v);
        let (cx, cz) = (
            self.actors[c].move_state.world_x,
            self.actors[c].move_state.world_z,
        );
        let heading =
            legaia_engine_battle_vm::battle_action::bearing_12bit_approx(vz, vx, cz, cx) & 0xFFF;
        self.actors[c].battle.facing_angle = heading;
        let Some((speed, scale)) = self.battle_playing_root_motion(c) else {
            return;
        };
        let (sin, cos) = legaia_engine_system::trig::trig12(heading);
        let (dx, dz) = legaia_engine_battle_vm::battle_action::motion::root_motion_step(
            sin,
            cos,
            speed,
            self.battle_frame_step().max(1),
            scale,
        );
        // A step, then the poll (`beq v0,zero,0x801F7620` at `0x801F76E4`).
        for _ in 0..MAX_STEPS {
            let ms = &mut self.actors[c].move_state;
            ms.world_x = ms.world_x.wrapping_add(dx as i16);
            ms.world_z = ms.world_z.wrapping_add(dz as i16);
            if self.battle_range_metric(caster_slot, victim_slot) != 0 {
                break;
            }
        }
    }

    /// One hit of a capture body's split cast ([`ticks::SplitHits`]): the
    /// wrapper roll at the shared site, its share pushed as the popup, the
    /// clamp, the HP and bar writes, the hit's reaction and the turn onto the
    /// caster. `None` when the victim is down or Stoned (no roll is drawn);
    /// otherwise whether the hit downed it.
    fn land_split_hit(
        &mut self,
        hits: &legaia_engine_cast::cast_module_ticks::SplitHits,
        hit: u8,
        caster_slot: u8,
        victim_slot: u8,
    ) -> Option<bool> {
        let v = usize::from(victim_slot);
        let hp = self.actors.get(v)?.battle.hp;
        if hp == 0 || self.battle.actor_is_petrified(victim_slot) {
            return None;
        }
        let roll = self.capture_module_roll_site(hits.shape, 0, caster_slot, victim_slot)?;
        let share = roll.max(0) / hits.divisor;
        let landed = hits.landed(hit, roll, hp);
        // The HP and bar writes of the hit arm; the body picks its own clip.
        let staged = self.actors[v].battle.queued_anim;
        self.apply_effect_child_hit(v, landed);
        self.actors[v].battle.queued_anim = staged;
        self.battle
            .hit_fx
            .push(legaia_engine_battle::battle_events::BattleHitFx {
                target_slot: victim_slot,
                amount: share.clamp(0, i32::from(u16::MAX)) as u16,
                is_heal: false,
                is_crit: false,
            });
        let dead = self.actors[v].battle.hp == 0;
        let index = hits
            .reactions
            .get(usize::from(hit.saturating_sub(1)))
            .copied()
            .unwrap_or(2);
        if let Some(clip) = self
            .battle_reaction_map(v)
            .map(|m| m[if dead { 2 } else { index }])
        {
            self.commit_battle_reaction_entry(v, clip);
        }
        self.turn_victim_to_caster(victim_slot, caster_slot);
        Some(dead)
    }

    /// PROT 0939's hit `hit` (`1..=4`, [`ticks::SPORE_GAS_HITS`]), then its
    /// Rot roll behind the record's two immunity bits (`+0x6BC & 0x11000000`).
    /// A victim the hit downs ends the count.
    fn spore_gas_hit(&mut self, hit: u8, caster_slot: u8, victim_slot: u8) {
        use legaia_engine_cast::cast_module_ticks as ticks;
        const IMMUNE: u32 = 0x1100_0000;
        let v = usize::from(victim_slot);
        let Some(dead) = self.land_split_hit(&ticks::SPORE_GAS_HITS, hit, caster_slot, victim_slot)
        else {
            return;
        };
        let party = victim_slot < self.party.party_count;
        let immune = party
            && self
                .party
                .character_ability_bits
                .get(v)
                .is_some_and(|&b| b & IMMUNE != 0);
        if hit < 4
            && !immune
            && let Some(limb) = ticks::spore_gas_rot(hit, self.next_rand())
        {
            self.battle.status_effects.apply(
                victim_slot,
                legaia_engine_battle_vm::status_effects::StatusKind::Rot,
            );
            self.battle.status_effects.set_rot_limb(victim_slot, limb);
        }
        if dead {
            self.casting.spore_gas_beat.hits = 4;
        }
    }

    /// [`Self::fold_pending_cast`] for a chain arm that stores a rate on each
    /// seat it strikes, beside the reaction ([`ticks::ChainArm::hit_rate`]). The
    /// clip commit that follows decays anything off normal to `4`.
    fn fold_pending_cast_at_rate(&mut self, rate: Option<u8>) {
        let struck: Vec<u8> = match (&self.casting.pending_cast, rate) {
            (Some(pc), Some(_)) => pc.targets.clone(),
            _ => Vec::new(),
        };
        self.fold_pending_cast();
        for s in struck {
            if let (Some(a), Some(rate)) = (self.actors.get_mut(usize::from(s)), rate) {
                a.battle.anim_rate = legaia_engine_battle_vm::battle_anim_rate::AnimRate(rate);
            }
        }
    }

    /// PROT 0925's outcome arm (arm 8, `FUN_801F6A00`). With the no-escape
    /// byte `ctx[+0x287]` clear (`0x801F7764`) the arm stages the party's
    /// flee - the same staging, camera cut and HP floor the run band's
    /// granted escape uses (`World::stage_party_flee`; the HP floor is
    /// `0x801F7964..0x801F7984`) - and hands the action SM state `0x65` with
    /// `ctx[+0x6D8] = 0x3C` and the escape outcome, which the step applies
    /// after its write-back. On every fight it then runs the round tail.
    ///
    /// PORT: FUN_801F6A00 (arm 8: flee staging + round tail; the record-side
    /// HP / MP write-back is the battle teardown's)
    /// The target-size framing `FUN_801F0348` for a cast by `caster_slot`:
    /// the `ctx[+0x6D0]` depth its target's (or, for a monster caster, its
    /// own) size class gives.
    ///
    /// REF: FUN_801F0348
    fn cast_target_frame_depth(&self, caster_slot: u8) -> i16 {
        let target = self
            .actors
            .get(usize::from(caster_slot))
            .map_or(8, |a| a.battle.active_target);
        let size_of = |slot: u8| {
            self.actors
                .get(usize::from(slot))
                .and_then(|a| a.battle_monster_id)
                .and_then(|id| self.tables.monster_catalog.get(id))
                .map_or(0, |d| d.size_class)
        };
        legaia_engine_battle_vm::battle_formulas::camera_height_for_frame(
            caster_slot,
            target,
            self.party.party_count,
            size_of,
        )
    }

    fn spikefish_outcome(
        &mut self,
        ctx: &mut legaia_engine_cast::cast_module_ticks::CastModuleCtx,
    ) {
        let party_n = usize::from(
            self.party
                .party_count
                .min(legaia_engine_cast::cast_module_ticks::FIRST_MONSTER_SEAT),
        )
        .min(self.actors.len());
        if !self.battle.no_escape {
            self.stage_party_flee();
            for a in self.actors.iter_mut().take(party_n) {
                if a.battle.hp == 0 {
                    a.battle.hp = 1;
                    a.battle.liveness = 1;
                }
            }
            self.casting.module_flee = true;
        }
        let mut party: Vec<_> = (0..party_n as u8)
            .map(|s| self.cast_actor_state(s))
            .collect();
        let refunds = legaia_engine_cast::cast_module_ticks::spikefish_round_tail(ctx, &mut party);
        for (a, view) in self.actors.iter_mut().zip(&party) {
            a.battle.init_key = view.init_key;
        }
        for item in refunds {
            let _ = self.party.inventory.add(item, 1);
        }
    }
}

/// `true` for a capture director that lands the module's wrapper hits itself
/// (PROT 0958's six sites, PROT 0959's three): each is `FUN_801DD6B4` at its
/// own call site, so no generic fold may roll the cast as well.
fn director_lands_hits(entry: u32) -> bool {
    legaia_engine_cast::cast_module_ticks::damage_shape_for(entry).is_some()
        && matches!(entry, 958 | 959)
}
