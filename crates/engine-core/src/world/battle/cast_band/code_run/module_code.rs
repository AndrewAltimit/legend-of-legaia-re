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
        // PROT 0954's body is ported whole - camera, records, wheel and
        // outcomes - and drives the band on its own
        // (`world::battle::fatal_decision`).
        if entry == legaia_engine_cast::cast_fatal_decision::FATAL_DECISION_ENTRY {
            return Some(self.run_fatal_decision(entry));
        }
        self.casting.module_running_entry = Some(entry);
        let mut ctx = self.cast_module_ctx();
        // The phase the tick enters on, before a state-touching stager below
        // steps it (PROT 0909's arm 0 is its stager's).
        let phase_entry = ctx.phase;
        let caster_slot = self.battle_ctx.active_actor;
        let victim_slot = self
            .actors
            .get(caster_slot as usize)
            .map(|a| a.battle.active_target)
            .unwrap_or(0);
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
            e if Self::runs_part_stager_calls(e) => self.run_part_stager_calls(e),
            922 => self.run_puera_stager_calls(&mut ctx),
            927 => self.run_juggernaut_stager_calls(),
            923 => ticks::gilium_stager(&mut ctx, arm),
            949 => ticks::water_crystals_stager(&mut victim, arm),
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
        // The module countdown this pass opens on (PROT 0922's seat writes
        // read it).
        let countdown_before = self.casting.module_cam.countdown.0;
        // PROT 0916's arm-0 kill roll, drawn once per cast ahead of its
        // director.
        if entry == legaia_engine_cast::cast_module_camera::ALURU_ENTRY {
            self.draw_aluru_verdict(caster_slot, victim_slot, spell_id);
        }
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
                let d = direct(&mut st, ctx.phase, seats);
                st.sideband_busy = held;
                self.store_module_cam(st);
                d
            })
        };
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
        if entry == 927
            && let Some(d) = direction
        {
            self.apply_juggernaut_writes(ctx.phase, !d.hold, ctx.monster_count);
        }
        if entry == 918
            && let Some(d) = direction
        {
            self.apply_kemaro_writes(ctx.phase, !d.hold, victim_slot);
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
        let mut capture_held = false;
        let mut capture_arm = None;
        let phase_in = ctx.phase;
        // A capture-class body's camera arms, on the phase it is about to
        // run (they make no gate of their own: the body's port does).
        if let Some(direct) = legaia_engine_cast::cast_module_camera::capture_camera_director(
            entry,
            body.unwrap_or(legaia_engine_cast::cast_module_camera::SINGLE_BODY),
        ) {
            let seats = self.module_cam_seats(caster_slot, victim_slot);
            let mut st = self.module_cam_with_yaw_base();
            let arm = direct(&mut st, ctx.phase, seats);
            self.store_module_cam(st);
            if let Some(v) = arm.latch
                && let Some(i) = self.caster_latch_index(caster_slot)
            {
                self.battle.monster_ai_state.dat[i] = v;
            }
            if arm.skips_fold {
                self.casting.module_skips_fold = true;
            }
            run.camera_shot = arm.shot;
            run.capture_drift = arm.drift;
            capture_held = arm.hold;
            capture_arm = Some(arm);
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
        if let Some(arm) = chain.and_then(|c| c.arm(phase_in))
            && !capture_held
        {
            if arm.wrapper_site.is_some() && self.casting.module_hit_arm != Some(phase_in) {
                self.casting.module_hit_arm = Some(phase_in);
                self.fold_pending_cast();
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
        run.camera_follow = direction.and_then(|d| d.follow);
        run.camera_end_frame = direction.is_some_and(|d| d.end_frame);
        run.camera_nudge = direction.and_then(|d| d.nudge);
        run.spawns = direction.map_or(&[], |d| d.spawns);
        run.arm_point = direction.and_then(|d| d.arm_point);
        run.vram_move = direction.and_then(|d| d.vram_move);
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
                (945, Some(ticks::WATER_COLUMN_TICK)) => {
                    Some(ticks::water_column_tick(&mut ctx, &mut victim, None, 0))
                }
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
                    if hit.is_some() {
                        self.casting.module_skips_fold = true;
                    }
                    Some(ticks::plasma_strike_tick(
                        &mut ctx,
                        &mut caster,
                        &mut victim,
                        hit,
                    ))
                }
                (957, Some(ticks::SUMMON_EFFECT_TICK_B)) => {
                    Some(ticks::summon_effect_tick_b(&mut ctx, &mut victim))
                }
                (957, Some(ticks::SUMMON_EFFECT_TICK_A)) => {
                    Some(ticks::summon_effect_tick_a(&mut ctx, &mut victim, None))
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
                (951, Some(ticks::SCYTHE_WIND_TICK)) => {
                    Some(ticks::scythe_wind_tick(&mut ctx, &mut victim, None))
                }
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
                    let roll = Some(self.next_rand());
                    let (step, refund) = ticks::kiss_of_death_tick(&mut ctx, &mut victim, roll);
                    run.item_refund = refund;
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
                    let rolls = Some((self.next_rand(), self.next_rand()));
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
                    let mut rolls: Vec<u32> = Vec::new();
                    if ctx.phase == 0 {
                        for _ in 0..=ticks::ELEMENT_CHANGE_MAX_REROLLS {
                            rolls.push(self.next_rand());
                        }
                    }
                    let mut cursor = rolls.into_iter();
                    let (step, outcome) =
                        ticks::element_change_tick(&mut ctx, &mut seats, last_roll, || {
                            cursor.next().unwrap_or(0)
                        });
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
                    if shield_arm && !self.mystic_shield_up() {
                        // `_DAT_8007BD84 = FUN_80021B04(..)` - the shield's
                        // sphere, seated with its texture scroller.
                        self.raise_mystic_shield();
                    }
                    Some(step)
                }
                (940, Some(arms::GLARE_DIVIDE_SPLIT_TICK)) => {
                    let ext = self.cast_arm_ext_state(caster_slot);
                    let saved = self.casting.module_split_saved_target.unwrap_or(0);
                    let roll = (ctx.phase == 2).then(|| self.next_rand());
                    let (step, split) = arms::glare_divide_split_tick(
                        &mut ctx,
                        &mut caster,
                        &ext,
                        spell_id,
                        saved,
                        roll,
                    );
                    if let Some(sp) = split {
                        self.casting.module_split_saved_target = Some(sp.saved_caster_target);
                        self.apply_glare_divide_split(caster_slot, &sp);
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
                (944, Some(arms::GUILTY_CROSS_TICK)) => Some(arms::guilty_cross_tick(
                    &mut ctx,
                    &mut caster,
                    &mut victim,
                    None,
                )),
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
                    let rolls =
                        self.cast_arm_sweep_rolls(&ctx, 956, arms::WATER_HAZARD_TICK, caster_slot);
                    let status: Vec<(u8, u32)> = if ctx.phase == 2 {
                        rolls.iter().map(|(s, _)| (*s, self.next_rand())).collect()
                    } else {
                        Vec::new()
                    };
                    let mut seats: Vec<ticks::CastActorState> = self.cast_seat_row();
                    let take = |seat: u8| {
                        rolls
                            .iter()
                            .find(|(s, _)| *s == seat)
                            .map(|(_, d)| *d)
                            .unwrap_or(0)
                    };
                    let st = |seat: u8| {
                        status
                            .iter()
                            .find(|(s, _)| *s == seat)
                            .map(|(_, r)| *r)
                            .unwrap_or(1)
                    };
                    let (step, hits) = arms::water_hazard_tick(
                        &mut ctx,
                        r_caster as u8,
                        code,
                        &mut caster,
                        &mut seats,
                        take,
                        st,
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
                    let t = ticks::run_chain_body(
                        chain,
                        &mut ctx,
                        &mut caster,
                        &mut victim,
                        &mut seat,
                        selector,
                    );
                    run.item_refund = t.refund;
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
                    let cone_seats: Vec<u8> = if entry == 904
                        && ctx.phase == ticks_a::THEEDER_SWEEP_ARM
                    {
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
                            // module only ever sees "in" or "out". The damage
                            // magnitude stays the fold's, so an in-cone seat
                            // gets a zero roll and the arm's presentation half
                            // (render flag, reaction bits) runs.
                            let in_cone = cone_seats.clone();
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
                            let tick = ticks_a::theeder_tick(
                                &mut ctx,
                                &mut seats,
                                who,
                                geom,
                                &mut fx,
                                |seat| in_cone.contains(&seat).then_some(0),
                            );
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
                        _ => ticks_a::zenoir_tick(&mut ctx, &mut seats, who, |_| None),
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
                self.apply_horn_writes(phase_in, ctx.phase != phase_in, seat_slot);
            }
            if entry == 915 {
                self.apply_mushura_writes(phase_in, victim_slot);
            }
            if entry == 929 {
                self.apply_mule_writes(phase_in, ctx.phase != phase_in);
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
            self.slippery_rain_words(phase_in, ctx.phase != phase_in);
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
        self.late_creature_stager_words(entry, phase_in, ctx.phase != phase_in);
        if entry == 928
            && let Some(v) = legaia_engine_cast::cast_module_camera::palma_stage_byte(
                phase_in,
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
        // washes, CLUT copies), decoded off the module image.
        self.fire_cast_presentation(
            entry,
            if has_trampoline { body } else { None },
            (phase_entry != phase_in).then_some(phase_entry),
            phase_in,
            ctx.phase,
            run.tick_ported && !run.busy,
        );
        Some(run)
    }
}

impl World {
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
