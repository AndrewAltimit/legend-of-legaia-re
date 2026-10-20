//! Which pad drive a capture's surface needs, and running it from the round
//! prompt.

use super::*;

impl RetailBattle {
    /// Monster indices that read `0` HP on a capture of a **party** action in
    /// flight: the swing's victims, which [`run_engine_battle`] can seed at
    /// `1` HP so the replay makes the kill rather than ending the fight at
    /// its first wipe gate.
    /// Whether the capture's yaw is an idle-orbit clock reading rather than a
    /// framing: a flow byte the battle tick's orbit store runs on
    /// ([`ORBIT_FLOWS`]), or a pre-seed capture whose far framing has landed
    /// ([`SpanGate::Landed`]) - case 9 passes `_DAT_8007B792` straight
    /// through and nothing in `0x0A` writes it, so it holds wherever the
    /// orbit left it.
    pub fn orbit_owns_yaw(&self) -> bool {
        ORBIT_FLOWS.contains(&self.flow) || self.span_gate == SpanGate::Landed
    }

    ///
    /// A party cast replayed from its dispatch ([`SeedPlan::Cast`]) has the
    /// same victims: a capture past the spell's kill (the Done band's hold,
    /// `shiny_refactor_gimard_levelup`) reads the target already at `0` and
    /// faded, and a replay onto that corpse lands no damage, so the frame
    /// lacks the `DAMAGE` readout and the death retail's frame follows.
    ///
    /// Only a capture past the cast band (the Done band, `0x50` and above)
    /// counts its dead as the cast's: one taken mid-cast reads monsters an
    /// earlier action already killed at `0` too (`gilium_summon_mid_cast`),
    /// and reviving those lands nothing before the phase.
    pub fn action_victims(&self) -> Vec<usize> {
        let past_cast = self.seed_plan() == SeedPlan::Cast
            && self.action_state
                >= legaia_engine_battle_vm::battle_action::ActionState::DoneCleanup.as_byte();
        if !matches!(self.seed_plan(), SeedPlan::Action { seat, .. } if seat < 3) && !past_cast {
            return Vec::new();
        }
        self.monsters
            .iter()
            .enumerate()
            .filter_map(|(m, c)| c.filter(|c| c.hp == 0 && c.hp_max != 0).map(|_| m))
            .collect()
    }

    /// Whether the image child takes the frame the display showed - the
    /// drive's tick less [`Self::display_lag`] - rather than the RAM's
    /// (`LEGAIA_CAPTURE_DISPLAY_LAG`; the child keeps the gate's own frame
    /// when the drive's state is younger than the lag). Measured per span
    /// gate over the corpus:
    ///
    /// * a party seat's [`SpanGate::Age`] and any [`SpanGate::CaptureFade`]
    ///   take it - every such frame rose (`battle_vahn_tri_somersault_super`
    ///   `0.716` to `0.824`, `nivora_duel_mid_blazing_slash` to `0.996`);
    /// * a [`SpanGate::Landed`] capture keeps the RAM's frame: retail sat
    ///   parked on the CD wait with its glide long landed, while the
    ///   engine's gate is the tick its own glide lands, so the frames before
    ///   it are mid-glide (`evil_medallion_rage_battle` `0.913` to `0.395`);
    /// * a monster's [`SpanGate::Age`] keeps it: Tail Fire's rays run on the
    ///   engine's own clock and both `battle_gimard_tail_fire_*` frames fell;
    /// * a [`SpanGate::DoneHold`] keeps it (`noa_levelup_fight_pre` fell).
    pub fn display_retake(&self) -> bool {
        self.display_lag > 0
            && match self.seed_plan() {
                SeedPlan::Action { seat, .. } => match self.span_gate {
                    SpanGate::Age { .. } => seat < 3,
                    SpanGate::CaptureFade { .. } => true,
                    _ => false,
                },
                _ => false,
            }
    }

    /// Whether the image child's camera is aligned to the pose the
    /// displayed frame was drawn at ([`Self::displayed_cam_live`]) rather
    /// than the RAM's. Every action drive but a [`SpanGate::DoneHold`]:
    /// the Done band's captures part on where the engine's replay left
    /// the bodies, and walking the camera back moved `noa_levelup_fight_pre`
    /// off retail's. Elsewhere it rose wherever it moved - with the frame
    /// itself left at the RAM's instant too (`battle_gimard_tail_fire_b`
    /// `0.745` to `0.772`).
    pub fn displayed_camera(&self) -> bool {
        matches!(self.seed_plan(), SeedPlan::Action { .. })
            && !matches!(self.span_gate, SpanGate::DoneHold { .. })
    }

    /// [`Self::clip_cursors`] on engine slots: the party's pool slots
    /// `0..party_count` as they are, the monster row `3..` packed behind
    /// the party, as the engine seats it.
    pub fn engine_clip_cursors(&self) -> Vec<legaia_engine_capture::clip_cursors::ClipCursorSeed> {
        let pc = self.party_count.clamp(1, 3);
        self.clip_cursors
            .iter()
            // While an action is in flight (ahead of the Done band
            // `0x50..=0x5A`), the acting seat's clip and its target's reaction
            // run with it, and the seed already placed it where the
            // capture's stood.
            //
            // Not yet, though, on the seed's own passes: the seed `0x0C` and
            // an Attack arm `0x14` whose body has not run (the yaw counter
            // still on the seed's `0x200` rung, [`tween_passes_back`]) have
            // staged no clip - `0x14` stages the approach - so both bodies
            // still sway on the idle loops the replay reaches on its own
            // clock (`super_queue_replace_*`).
            //
            // The capture-class band `0x6F..=0x71` is the exception for a
            // body it leaves on its idle loop (clip `0`): the slot-B module
            // there runs its own countdown and stages no clip on either seat,
            // so that cursor is time since the loop began -
            // `zeto_big_wave_mid_cast`'s caster idles through PROT 0946's band
            // in a close-up the loop's sway fills. Elsewhere an idle seat in
            // flight may have just come off the action's own clip.
            //
            // So is a party target already on its reaction (the
            // `+0x1EF..+0x1F3` family `2, 3, 4, 5, 0x0B`): the reaction
            // started on the hit, which the replay lands on its own clock,
            // and the displayed frame is two game frames older than the RAM -
            // a knockdown carries the whole body in that time
            // (`battle_gimard_tail_fire_b`'s Vahn). A cursor below zero is a
            // displayed frame from before the hit, left alone.
            .filter(|&&(slot, clip, cursor)| {
                let staged = !(self.action_state == 0x0C
                    || self.action_state == 0x14 && self.walk_yaw_base == 0x200);
                let in_flight =
                    self.action_state != 0 && staged && !(0x50..=0x5A).contains(&self.action_state);
                let capture_band_idle = clip == 0 && (0x6F..=0x71).contains(&self.action_state);
                let party_reaction = slot < 3
                    && slot == self.target_code
                    && matches!(clip, 2..=5 | 0x0B)
                    && cursor >= 0;
                !in_flight
                    || capture_band_idle
                    || party_reaction
                    || (slot != self.active_actor && slot != self.target_code)
            })
            .filter_map(|&(slot, clip, cursor)| {
                let slot = if slot < 3 {
                    (slot < pc).then_some(slot)?
                } else {
                    let m = slot - 3;
                    (m < self.monster_count).then_some(pc + m)?
                };
                Some(legaia_engine_capture::clip_cursors::ClipCursorSeed { slot, clip, cursor })
            })
            .collect()
    }

    /// The party seats ahead of `seat` whose committed category this round
    /// is Spirit (`+0x1DE = 4`), as a bitmask: a menu capture's commit log
    /// shows each such member's `Spirit` (`delilas_che_hammer_gaps_v3`'s
    /// Vahn and Noa), which a drive committing a plain Attack draws as
    /// `Attack` and a target.
    pub fn spirit_seats_ahead(&self, seat: u8) -> u8 {
        self.party_categories
            .iter()
            .take(usize::from(seat))
            .enumerate()
            .filter(|&(_, &c)| c == 4)
            .fold(0, |m, (s, _)| m | 1 << s)
    }

    /// The pad drive that reaches this capture, when its plan has one.
    pub fn battle_drive(&self) -> Option<BattleDrive> {
        match self.seed_plan() {
            SeedPlan::Menu { flow, seat } => Some(BattleDrive::Menu {
                flow,
                seat,
                spirit: self.spirit_seats_ahead(seat),
            }),
            SeedPlan::Action { seat, state } => Some(BattleDrive::Action {
                seat,
                state,
                category: self.queued_category,
                queued: self.queued_action,
                spare: !self.action_victims().is_empty(),
                absorbed: if seat < 3 { self.absorbed_seru } else { 0 },
                end: self.span_gate,
                style: Some(self.cam_style),
                steer: ActionSteer {
                    // A party killing blow on its victim, or a monster
                    // action (a cast or a strike) on the party seat it
                    // aimed at: the monster's pick is a draw, and a run on
                    // another member frames another approach.
                    target: ((seat < 3
                        && self.target_code >= 3
                        && self
                            .action_victims()
                            .contains(&usize::from(self.target_code - 3)))
                        || (seat >= 3
                            && matches!(self.queued_category, 2 | 3)
                            && self.target_code < 3)
                        // A monster cast on itself (Cort's Ultra Charge,
                        // `+0x1DD == ctx[+0x13]`): retail's cast-begin skips
                        // the facing store for a self target (`beq v0,t2` at
                        // `0x801E4350`), and the module frames from the
                        // heading the caster keeps.
                        || (seat >= 3 && self.queued_category == 2 && self.target_code == seat))
                        .then_some(self.target_code),
                    yaw: Some(self.walk_yaw_base),
                    coin: Some(self.track_coin),
                    message: self.timed_message,
                    // An empty content word is the counter swap's only on
                    // an attack: a Spirit action never opens the target
                    // plaque, so its word reads zero with no swap behind it
                    // (`nivora_duel_pre_megaton_press`, Gala's Spirit hold),
                    // and steering the counter HUD there raised a readout
                    // bar under the AP bar retail draws alone.
                    plate_cleared: self.target_plate_cleared
                        && seat < 3
                        && self.queued_category == 3,
                    arts: seat < 3 && self.queued_category == 3 && self.arts_queue,
                    gauge: self
                        .acting_gauge
                        .filter(|&(live, base)| {
                            seat < 3 && self.queued_category == 3 && self.arts_queue && live > base
                        })
                        .map(|(live, _)| live),
                    clip: (seat < 3
                        // Gate on the direction swing clips 0x0C..=0x0F only:
                        // retail can sit on the dynamic art slot 0x10 / 0x11
                        // while the engine holds the swing's own clip, which
                        // the gate could then never match
                        // (battle_melee_hit_spark: retail 0x11, engine 0x0E).
                        && ((0x0C..=0x0F).contains(&self.caster_clip)
                            // ...and on the idle clip in the return state
                            // `0x20`, whose first hold waits while the
                            // attacker's `+0x1D9 != 0` (`0x801E54EC`): a
                            // capture reading `0` there is past the last
                            // swing, and the acting actor's idle commit is
                            // what its accumulator counts from. Ungated, the
                            // first tick the engine's accumulator reached the
                            // value was inside the last art clip
                            // (`player_steal_skeleton_banner`: the frame
                            // showed the Somersault still landing, retail the
                            // steal caption over an idle Vahn).
                            || (self.caster_clip == 0 && state == 0x20))
                        && matches!(self.span_gate, SpanGate::Age { .. }))
                    .then_some(self.caster_clip),
                    aim: (seat < 3
                        && self.queued_category == 3
                        && (3..8).contains(&self.target_code))
                    .then(|| self.target_code - 3),
                    queue: (seat < 3 && self.queued_category == 3 && self.arts_queue)
                        .then_some(self.committed_queue),
                    cursor: (seat < 3
                        && self.queued_category == 3
                        && self.arts_queue
                        && matches!(state, 0x1E | 0x1F)
                        // Only on the dynamic art slots, the clips the
                        // `clip` gate leaves open: an idle `0` or a swing is
                        // placed by the age and the clip already.
                        && matches!(self.caster_clip, 0x10 | 0x11)
                        && matches!(self.span_gate, SpanGate::Age { .. }))
                    .then_some(self.strike_cursor),
                    // A party swing captured after its victim's death commit:
                    // the close-up accumulator restarts on every idle cycle
                    // of the attacker (its natural-end re-commit), so the
                    // age alone matches the first cycle after the swing,
                    // before the knockdown has ended. The defeat fade dates
                    // the death (`player_steal_skeleton_banner`: lane
                    // `0xF0`, 34 vsyncs into the fade, with the idle on its
                    // second cycle).
                    fading: self
                        .target_fade_lane
                        .filter(|_| {
                            seat < 3
                                && self.target_code >= 3
                                && matches!(self.span_gate, SpanGate::Age { .. })
                        })
                        .map(|lane| (self.target_code, lane)),
                    // The caption is the steal roll's outcome, a draw: a
                    // stream whose kill rolls no steal shows another frame.
                    spoils: seat < 3 && self.spoils_caption,
                },
            }),
            SeedPlan::Opening => Some(BattleDrive::Opening {
                swept: matches!(self.flow, 0x0C | 0x14),
                entry: self.entry_counter,
            }),
            _ => None,
        }
    }
}

/// Run `drive` from the round prompt through the pad path. The ticks it took,
/// or `None` when the phase was never reached (or the fight ended first).
pub(super) fn run_drive(
    session: &mut NativeSession,
    director: &mut crate::retail_compare::RecordingDirector,
    drive: BattleDrive,
    age_short: &mut Option<u16>,
) -> Result<Option<u32>> {
    drive.prime(&mut session.host.world);
    let mut reached = None;
    let mut held = 0;
    // The engine accumulator on the last tick an `Age` phase's state held
    // short of its age.
    let mut aged = None;
    for t in 0..drive.budget() {
        let world = &session.host.world;
        if reached.is_none() {
            if drive.in_aged_state(world) {
                aged = Some(active_clip_age(world));
            } else if let Some(a) = aged.take() {
                // The state ended before it was as old as retail's: the
                // re-run samples its last tick.
                *age_short = Some(a.min(u32::from(u16::MAX)) as u16);
                break;
            }
        }
        if drive.reached(world) {
            reached.get_or_insert(t);
            if held >= drive.hold_ticks() {
                break;
            }
            held += 1;
        } else if reached.is_some() {
            // The surface closed under the hold: sample where it stands.
            break;
        }
        if world.mode != SceneMode::Battle {
            break;
        }
        if std::env::var_os("LEGAIA_RC_POS_TRACE").is_some() && world.mode == SceneMode::Battle {
            eprintln!(
                "[pos] t={t} act={} st=0x{:02X} {}",
                world.battle_ctx.active_actor,
                world.battle_ctx.action_state,
                (0..world.actors.len().min(4))
                    .map(|i| {
                        let a = &world.actors[i];
                        format!(
                            "| {i}: ({}, {}) b{:?} f{} c{:#04X} tgt{} ",
                            a.move_state.world_x,
                            a.move_state.world_z,
                            a.battle.seat,
                            a.battle.facing_angle & 0xFFF,
                            world.battle_current_anim(i),
                            a.battle.active_target
                        )
                    })
                    .collect::<String>()
            );
        }
        if std::env::var_os("LEGAIA_RC_DRIVE_TRACE").is_some() {
            let hp: Vec<u16> = world.actors.iter().take(8).map(|a| a.battle.hp).collect();
            eprintln!(
                "[rc] t={t} mode={:?} flow={:?} cmd={} act={} st=0x{:02X} hp={hp:?} cam={:?} depth={} acc={:?} plaque_dy={} tint={:?}",
                world.mode,
                world.battle.flow,
                world.battle.command.is_some(),
                world.battle_ctx.active_actor,
                world.battle_ctx.action_state,
                world.battle.camera.as_ref().map(|c| c.phase()),
                world.battle.camera_frame_height as u16,
                world
                    .battle
                    .camera
                    .as_ref()
                    .map(|c| (c.close_up_accum(), c.is_gliding())),
                legaia_engine_battle_hud::battle_action_plaque_dy(world),
                world
                    .actors
                    .iter()
                    .take(8)
                    .map(|a| (a.battle.render_flag, a.battle.render_color))
                    .collect::<Vec<_>>()
            );
            let anims: Vec<(u8, u32, bool)> = (0..world.actors.len().min(8))
                .map(|i| {
                    (
                        world.battle_current_anim(i),
                        world.actors[i].battle.damage_accum,
                        world.battle_on_knockdown(i),
                    )
                })
                .collect();
            let pose = world.battle.camera.as_ref().map(|c| {
                let p = c.framing_pose();
                (p.pitch as i32, p.yaw as i32, p.tr.map(|v| v as i32))
            });
            eprintln!(
                "[rc] t={t} anims={anims:?} pose={pose:?} mod={} cd={} tgt={} pos={:?}",
                world.casting.module_phase,
                world.casting.module_cam.countdown.0,
                world
                    .actors
                    .get(usize::from(world.battle_ctx.active_actor))
                    .map_or(0, |a| a.battle.active_target),
                world
                    .actors
                    .iter()
                    .take(8)
                    .map(|a| (a.move_state.world_x, a.move_state.world_z))
                    .collect::<Vec<_>>()
            );
            eprintln!(
                "[rc] t={t} yaw_base={:?} style={} y/facing={:?}",
                world.battle.camera.as_ref().map(|c| c.action_yaw_base()),
                world.battle_ctx.camera_variant,
                world
                    .actors
                    .iter()
                    .take(8)
                    .map(|a| (a.move_state.world_y, a.battle.facing_angle & 0xFFF))
                    .collect::<Vec<_>>()
            );
            eprintln!(
                "[rc] t={t} clips={:?}",
                world
                    .actors
                    .iter()
                    .take(8)
                    .map(|a| a.battle_animation.as_ref().map(|p| (
                        p.action_id(),
                        p.cursor_sixteenths(),
                        p.frame_count(),
                        p.finished(),
                        a.battle.anim_rate.get()
                    )))
                    .collect::<Vec<_>>()
            );
        }
        let pad = if reached.is_some() {
            0
        } else {
            drive.pad_word_at(world, u64::from(t))
        };
        session.host.world.input.set_pad(pad);
        drive.steer(&mut session.host.world);
        session.tick()?;
        session.fog_render_tick();
        session.host.route_bgm_events(director)?;
    }
    session.host.world.input.set_pad(0);
    Ok(reached)
}
