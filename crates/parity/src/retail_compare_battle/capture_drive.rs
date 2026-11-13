//! Which pad drive a capture's surface needs, and running it from the round
//! prompt.

use super::*;

/// The rung the Attack arm stands the yaw counter `ctx[+0x6DA]` on as the
/// seed enters `0x14` (`0x801E2F20`).
const ATTACK_YAW_RUNG: u16 = 0x200;

/// The rung the seed pass stands the counter on (`0x801E2CF8`), which a Spirit
/// band action climbs from with no store of its own.
const SEED_YAW_RUNG: u16 = 0x800;
/// The shortest walk [`RetailBattle::party_approach`] places: under a few
/// passes the replay's own arrival is as near as the estimate.
const PARTY_WALK_MIN_VSYNCS: u32 = 8;

impl RetailBattle {
    /// The hits a party **melee** capture's action had landed, in push order
    /// ([`RetailBattle::action_hit_sequence`]), for the replay to deal again
    /// at retail's amounts (`legaia_engine_capture::melee_hits`), and the
    /// same for a monster's cast. Empty for any other plan: a party cast's
    /// hits come from its module, not the melee kernel.
    ///
    /// The total is `DAT_8007BD14` on the action's target, read only when
    /// the ring holds pushes (so it is this action's).
    pub fn melee_hit_seeds(&self) -> legaia_engine_capture::melee_hits::MeleeHitSeed {
        match self.seed_plan() {
            SeedPlan::Action { seat, .. }
                if seat < 3
                    && self.queued_category == 3
                    && !self.action_hit_sequence.is_empty() =>
            {
                legaia_engine_capture::melee_hits::MeleeHitSeed {
                    hits: self.action_hit_sequence.clone(),
                    total: (self.target_code >= 3 && self.combo_total > 0)
                        .then_some((self.target_code, self.combo_total)),
                    own_clip: (self.clip_pushes > 0)
                        .then_some((self.caster_clip, self.clip_pushes)),
                }
            }
            // A monster's cast (`+0x1DE == 2`) pushes its hits on the same
            // ring; the replay's cast rolls them on its own stream
            // (`battle_gimard_tail_fire_*`: retail's `16` on Vahn, the replay
            // `14` / `17`).
            SeedPlan::Action { seat, .. }
                if seat >= 3
                    && self.queued_category == 2
                    && !self.action_hit_sequence.is_empty() =>
            {
                legaia_engine_capture::melee_hits::MeleeHitSeed {
                    hits: self.action_hit_sequence.clone(),
                    total: None,
                    own_clip: None,
                }
            }
            _ => Default::default(),
        }
    }

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

    /// Party seats a monster's wiping blow left at `0` HP, on a capture past
    /// the end signal whose results sequencer frames that monster
    /// ([`SeedPlan::Action`] on a monster seat, a [`SpanGate`] past the
    /// signal, every seated member down): the party twin of
    /// [`Self::action_victims`]. Seeded at `0`, the replay's first `0x5A`
    /// wipe gate ends the fight on whichever party seat acts first, so the
    /// sequencer's pose actor is that member and never the monster retail
    /// frames (`battle_jou_party_wipe_results_retail`: seat `5`, the wipe's
    /// Spirit round). Seeded at `1`, with the party on Spirit
    /// ([`BattleDrive::Action`]'s `spare`), the monster's replayed action
    /// makes the wipe. Empty on any other capture.
    pub fn party_wipe_victims(&self) -> Vec<usize> {
        if !matches!(self.seed_plan(), SeedPlan::Action { seat, .. } if seat >= 3)
            || !self.span_gate.is_end()
        {
            return Vec::new();
        }
        let seated: Vec<usize> = self
            .party
            .iter()
            .enumerate()
            .filter_map(|(s, c)| c.filter(|c| c.hp_max != 0).map(|c| (s, c)))
            .map(|(s, c)| if c.hp == 0 { s } else { usize::MAX })
            .collect();
        if seated.is_empty() || seated.contains(&usize::MAX) {
            return Vec::new();
        }
        seated
    }

    /// `c`'s HP before the action in flight landed its hits on pool slot
    /// `slot` ([`Self::action_hits`]): the replay plays that action from its
    /// prompt and deals them again, so a bar seeded with the capture's HP
    /// would read them twice (`battle_gimard_tail_fire_a` showed Vahn's
    /// `154` less a second Tail Fire). A seat the action left at `0` keeps
    /// its HP, which [`Self::action_victims`] owns, and the sum is clamped to
    /// the bar's maximum.
    pub fn pre_action_hp(&self, slot: u8, c: &Combatant) -> u16 {
        if c.hp == 0 {
            return c.hp;
        }
        let dealt: u32 = self
            .action_hits
            .iter()
            .filter(|&&(s, _)| s == slot)
            .map(|&(_, v)| u32::from(v))
            .sum();
        (u32::from(c.hp) + dealt).min(u32::from(c.hp_max.max(c.hp))) as u16
    }

    /// Whether a monster's captured Attack stands past its self-heal gate.
    /// Monsters `0x47`, `0x48`, `0x68`, `0x69` and `0x6A` cast `0x52` from the
    /// picker's switch at HP `<= max / 3` with the latch `DAT_801C8FE0[i + 4]`
    /// clear (`0x801EAD38`). The round's setup picks ahead of its prompt, so a
    /// strike on those words was picked on more HP, and a harvest probe cut it
    /// at the prompt after (`battle_map03_heavy_grude_strike`: `832` of `2498`,
    /// latch `0`). The live game cannot stand here. The replay picks on the
    /// seeded HP, and on every stream casts the heal the capture never ran, so
    /// the seed raises the latch.
    pub fn heal_gate_passed(&self) -> bool {
        let Some(m) = usize::from(self.active_actor).checked_sub(3) else {
            return false;
        };
        self.queued_category == 3
            && self
                .monster_ids
                .get(m)
                .is_some_and(|id| matches!(id, 0x47 | 0x48 | 0x68 | 0x69 | 0x6A))
            && self
                .monsters
                .get(m)
                .copied()
                .flatten()
                .is_some_and(|c| c.hp <= c.hp_max / 3)
    }

    /// Whether a monster's captured cast stands with its latch
    /// `DAT_801C8FE0[i + 4]` raised. The body reads the word at its arm 0, and
    /// the replay re-enters the cast there: PROT 0953 forks on it
    /// (`0x801F6B04`), so a clear word takes the charge, which holds another
    /// stage and has no strike (`battle_tunnelc_xain_terio_punch_mid_cast`
    /// holds `1` with its phase still `0`).
    pub fn caster_latch_raised(&self) -> bool {
        self.active_actor >= 3 && self.queued_category == 2 && self.caster_latch != 0
    }

    /// Whether the capture is a monster strike gated on a swing clip
    /// (`0x1E` on a clip's accumulator, a swing clip committed), whose
    /// strike stream the drive can stand on the capture's
    /// ([`Self::strike_stream_steer`]).
    pub fn strike_stream_steerable(&self) -> bool {
        matches!(self.seed_plan(), SeedPlan::Action { seat, .. } if seat >= 3)
            && self.queued_category == 3
            && self.action_state == 0x1E
            && (self.caster_clip > 1 || self.strike_swing_pending())
            && matches!(self.span_gate, SpanGate::Age { .. })
    }

    /// Whether the capture's first swing is staged and not yet committed, the
    /// attacker still on the idle or walk clip it arrived on (`+0x1D9 <= 1`):
    /// the strike-loop pass staged the byte at `+0x1DF` (`0x801E3734`) and
    /// found another behind it, or it would have left for `0x1F` on that same
    /// pass (`0x801E3998`). A stream the replay rolled shorter leaves the loop
    /// on its first pass and never holds retail's state.
    pub fn strike_swing_pending(&self) -> bool {
        self.caster_clip <= 1
            && self.strike_cursor > 0
            && self
                .committed_queue
                .get(usize::from(self.strike_cursor))
                .is_some_and(|&b| b != 0)
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
    /// * a monster's [`SpanGate::Age`] in the magic band takes it only where
    ///   the gate's frame is another shot than the display showed
    ///   ([`Self::retake_on_shot_only`]);
    /// * a [`SpanGate::Landed`] capture keeps the RAM's frame: retail sat
    ///   parked on the CD wait with its glide long landed, while the
    ///   engine's gate is the tick its own glide lands, so the frames before
    ///   it are mid-glide (`evil_medallion_rage_battle` `0.913` to `0.395`);
    /// * a [`SpanGate::DoneHold`] keeps it (`noa_levelup_fight_pre` fell);
    /// * a capture past a walk-in the replay skips
    ///   ([`Self::walk_in_shot_aligned`]) keeps the gate's frame: the replay
    ///   runs that walk's arms in a tick or two, so the lag back from its
    ///   gate lands on `0x70`'s first tick, ahead of the shot retail's
    ///   display shows walking, and the shot is aligned onto the displayed
    ///   frame's pose instead (`koru_blade_breath_mid_cast`).
    pub fn display_retake(&self) -> bool {
        self.display_lag > 0
            && match self.seed_plan() {
                SeedPlan::Action { seat, .. } => match self.span_gate {
                    SpanGate::Age { .. } => seat < 3 || self.retake_on_shot_only(),
                    SpanGate::CaptureFade { .. } => !self.walk_in_shot_aligned(),
                    SpanGate::Results { .. } => self.results_opened_inside_lag(),
                    _ => false,
                },
                _ => false,
            }
    }

    /// A results-frame capture taken within the display lag of the
    /// sequencer's results open ([`SpanGate::Results`] with its hold `gp+0xA54`
    /// under [`Self::display_lag`]): the RAM has opened the results (the loss
    /// or spoils window, the HUD dismissed) while the display still shows a
    /// frame from before it, the party HUD up and the target plaque over it.
    /// The image child then retakes across the state's entry
    /// ([`Self::retake_across_band_entry`]). `battle_jou_party_wipe_results_retail`
    /// holds `3` against a lag of `6`.
    /// The span gate on the engine's clock. A results gate's hold is
    /// retail's `gp+0xA54`, which the results frame zeroes and then advances
    /// by its own pass's vsync delta (`0x8004F738..0x8004F76C`): the results
    /// pass already reads the frame step. The engine's sequence opens the
    /// results at hold `0` and counts a vsync a tick after it, so retail's
    /// hold `h` is the engine's `h - step`, and the results pass itself the
    /// engine's opening tick (`battle_jou_gala_finisher_results_retail`, hold
    /// `3` at step `3`: the RAM's camera still on the load window's
    /// stand-off). A harness mapping; every other gate passes through.
    pub fn engine_span_gate(&self) -> SpanGate {
        match self.span_gate {
            SpanGate::Results { hold } => SpanGate::Results {
                hold: hold.saturating_sub(u16::from(self.frame_step.max(1))),
            },
            g => g,
        }
    }

    pub fn results_opened_inside_lag(&self) -> bool {
        matches!(self.span_gate, SpanGate::Results { hold } if hold < self.display_lag)
    }

    /// A monster's magic-band capture ([`SpanGate::Age`] on a monster seat,
    /// category `2`, state `0x2A..=0x2E`): the cast-effect driver's shot
    /// first frames the camera on the first `0x2B` pass (`jal 0x801DC0A0` at
    /// `0x801E4834`; `0x29`'s own call is overridden by the case 6 its pass
    /// ends on, `0x801E474C`), so a capture two passes into `0x2B` holds the
    /// shot while its display shows the case-6 framing before it. The image
    /// child retakes only when its own camera armed the shot after the
    /// retake's tick (`LEGAIA_CAPTURE_RETAKE_SHOT`). A shot that lands on its
    /// arming pass keeps nothing of the framing before it, so the replay's
    /// own case 6 is the only reading of the displayed frame; a shot still
    /// walking holds retail's table, and the retake stands on the pose walked
    /// back along it. Elsewhere a monster's gate keeps the RAM's frame:
    /// retaken on every monster `Age` gate the strikes split both ways
    /// (`battle_jouind_gilium_lv3_strike` `0.850` to `0.953`,
    /// `battle_vozz_gizam_strike` `0.964` to `0.921`).
    pub fn retake_on_shot_only(&self) -> bool {
        self.monster_magic_band_age()
    }

    /// A strike loop on its clip age, a monster's or a party seat's. `0x1E`
    /// arms case 7 on every pass (`li a1,0x7` at `0x801E36EC`), whoever
    /// strikes, so the table describes the pass it is read on and the walk
    /// back along it is retail's own displayed pose. A party strike's walk
    /// back chases endpoints both bodies and the attacker's heading moved
    /// (`camera_align::walker_end_shift`); retail films of three party
    /// strikes put the displayed frame two passes back, where their poses
    /// read `image` `.992` (`battle_jou_party_gala_chain_mid`), `.996`
    /// (`battle_garmel_party_noa_lizard_tail_mid`) and `.985`
    /// (`battle_ropeway_party_gala_flying_knee_late`), against `.746`,
    /// `.911` and `.927` on the replay's own glide aligned onto agreeing
    /// endpoints.
    /// The alignment otherwise takes only the components whose endpoint the
    /// replay's framing agrees with, and the replay's case 7 reads the body
    /// pairs a pass newer than the arm did: `battle_korb2_aluru_strike`'s
    /// endpoint `(54, 60)` is the midpoint on the attacker's body at
    /// `(131, 146)`, the RAM's reads `(176, 173)`. The camera channel still
    /// reads the law. The same stand over every battle state loses badly
    /// (`cort_evolved_final_crisis_mid_cast` `1.000` to `.344`): those shots
    /// were not armed by the pass the table describes.
    ///
    /// Scythe Wind's arms `3..=5` are the same shape: each held pass re-aims
    /// the shot (`scythe_wind_camera`'s sweep, `0x801F7C04` / `0x801F7D30` /
    /// `0x801F8008`), so the table describes its pass, and the replay's sweep
    /// reads its own countdown and heading. `battle_jou_nova_v3_scythe_wind_mid_cast`
    /// reads `.691` on the replay's pose, with Noa near the camera `60` px off,
    /// and `.942` here; every other cast in the corpus's band falls.
    pub fn stand_on_displayed_pose(&self) -> bool {
        let strike = matches!(
            (self.seed_plan(), self.span_gate),
            (SeedPlan::Action { state: 0x1E, .. }, SpanGate::Age { .. })
        ) && self.queued_category == 3;
        let scythe_sweep = self.active_actor >= 3
            && self.action_state == 0x70
            && self.queued_action == 0x5B
            && (3..=5).contains(&self.module_phase);
        strike || scythe_sweep
    }

    /// The strike's turn on the RAM's pass: the heading `+0x46` the pass
    /// stored less the one its framing arm read, on a strike-loop capture.
    ///
    /// State `0x1E` arms case 7 (`0x801E36E8`) before it re-faces the attacker
    /// (`0x801E371C`), and case 7's yaw is `ctx[+0x6DA] + 0x900 - heading`,
    /// half a turn on for an odd style `ctx[+0xD]`: the table's endpoint and
    /// the counter name the heading the arm read exactly. Retail films of
    /// three strikes read it so on every pass (`battle_jou_party_gala_chain_mid`
    /// `129` under a stored `157`; `battle_jou_party_vahn_chain_late` `3857`
    /// under `3853`; `battle_ropeway_party_gala_flying_knee_late` `807` under
    /// `804`). `None` off the strike loop, without a captured heading, or for
    /// a turn wider than [`STRIKE_ARM_TURN_MAX`] (another framing's table).
    pub fn strike_arm_turn(&self) -> Option<f32> {
        if self.action_state != 0x1E {
            return None;
        }
        let facing = i32::from(
            self.facing
                .get(usize::from(self.active_actor))
                .copied()
                .flatten()?,
        );
        let end = (self.cam_tween.1.yaw.round() as i32).rem_euclid(4096);
        let arm = (i32::from(self.walk_yaw_base)
            + super::camera_align::CASE7_YAW_BASE
            + 0x800 * i32::from(self.cam_style & 1)
            - end)
            .rem_euclid(4096);
        let turn = (facing - arm + 2048).rem_euclid(4096) - 2048;
        (turn.abs() <= STRIKE_ARM_TURN_MAX).then_some(turn as f32)
    }

    /// A monster's magic-band capture: [`SpanGate::Age`] on a monster seat,
    /// category `2`, state `0x2A..=0x2E`.
    pub fn monster_magic_band_age(&self) -> bool {
        matches!(
            (self.seed_plan(), self.span_gate),
            (SeedPlan::Action { seat, .. }, SpanGate::Age { .. }) if seat >= 3
        ) && self.queued_category == 2
            && legaia_engine_battle_vm::battle_cam_script::SPELL_CAM_STATES
                .contains(&self.action_state)
    }

    /// A monster's Spirit / Item band capture (a class-0 Heal or Hyper Heal,
    /// which the action seed's class-byte discriminator routes to `0x3C`):
    /// [`SpanGate::Age`] on a monster seat, category `2`, state `0x3D..=0x40`.
    pub fn monster_spirit_band_age(&self) -> bool {
        matches!(
            (self.seed_plan(), self.span_gate),
            (SeedPlan::Action { seat, .. }, SpanGate::Age { .. }) if seat >= 3
        ) && self.queued_category == 2
            && super::SPIRIT_STAGE_STATES.contains(&self.action_state)
    }

    /// The yaw the frame before a turning case `0xE` shot stood on. Case `0xE`
    /// aims the live yaw `step * 4` on from wherever it stands, every pass
    /// (`0x801DCB18`), and its accumulator counts the passes it has run, so
    /// the RAM's yaw walks back onto the case-6 yaw it took over: the one
    /// retail's frame holds where the replay's own case 6 carries the yaw
    /// history of its own rounds. A step-`4` capture displays one pass
    /// earlier still, a case-6 step off this yaw.
    pub fn retake_turning_yaw(&self) -> Option<u16> {
        if !self.retake_on_shot_only() || self.action_state != 0x2B || self.queued_shot != 0x0E {
            return None;
        }
        let SpanGate::Age { accum } = self.span_gate else {
            return None;
        };
        let (step, _) = self.step_run;
        let step = u16::from(step.max(1));
        let (live, _) = self.cam_tween;
        Some((live.yaw as i32 - i32::from(accum / (8 * step) * step * 4)).rem_euclid(4096) as u16)
    }

    /// The pose the displayed frame of a turning shot stood on, for a capture
    /// on the shot's own pass (`0x2B` at one pass of accumulator): two passes
    /// back along the case-6 walk the shot cut off.
    ///
    /// The shot closes each component in one frame, so the table's steps name
    /// the pass before it (`N - 1`). That pass ran no arm: the `0x2A`
    /// terminator calls neither `FUN_801DC0A0` nor `FUN_801D5854`
    /// (`0x801E4774..0x801E479C`), so it walked on the increment of the pass
    /// before it, `N - 2`, the one the display shows, which armed case 6
    /// (`ceil(gap / 0xC)`) and walked in the same pass. Both moves share that
    /// increment, and the increment is the gap's, so [`stale_pass_gap`]
    /// inverts them. Yaw takes case 6's endpoint from the counter
    /// `ctx[+0x6DA]`, less the prologue's drift over the passes since.
    ///
    /// The table's steps are unsigned. Pitch and TR y take the sign that
    /// stands nearer case 6's endpoint. TR z and the focus come from the
    /// framing of the action before ([`Self::previous_actor_bearing`]): the
    /// sign is inferred, and `None` where no earlier actor this round names
    /// one (the far framing comes from elsewhere).
    pub fn retake_turning_pose(&self) -> Option<legaia_engine_camera::projection::BattleCamPose> {
        let yaw_n1 = self.retake_turning_yaw()?;
        let SpanGate::Age { accum } = self.span_gate else {
            return None;
        };
        let (step, _) = self.step_run;
        if self.cam_style != 0 || accum != 8 * u16::from(step) {
            return None;
        }
        let [_, pass_n, pass_n1, pass_n2] = [0, 1, 2, 3].map(|i| i32::from(self.recent_steps[i]));
        let (s1, s2) = (pass_n1, pass_n2);
        let drift = |s: i32| (s * 4 / 3).max(1);
        let (live, end) = self.cam_tween;
        let [pitch_step, _, _, _, try_step, trz_step, fx_step, _, fz_step] =
            self.cam_tween_steps.map(i32::from);
        let back = |v_n1: i32, end6: i32| {
            let d = v_n1 - end6;
            end6 + d.signum() * stale_pass_gap(d.abs(), s1, s2)
        };
        // Pitch and TR y: the side of the shot's endpoint case 6 stands on.
        let near = |shot_end: i32, step: i32, end6: i32| {
            let (a, b) = (shot_end + step, shot_end - step);
            if (a - end6).abs() <= (b - end6).abs() {
                a
            } else {
                b
            }
        };
        let wrap_i = |a: f32| {
            let a = a as i32 & 0xFFF;
            if a >= 0x800 { a - 0x1000 } else { a }
        };
        let tr_y6 = legaia_engine_battle_vm::battle_cam_script::ACTION_TR_Y as i32;
        let pitch = back(near(wrap_i(end.pitch), pitch_step, 0), 0);
        let tr_y = back(near(end.tr[1] as i32, try_step, tr_y6), tr_y6);
        // Yaw: case 6's endpoint on the pass that armed it.
        let pool = usize::from(self.active_actor);
        let facing = i32::from((*self.facing.get(pool)?)?);
        let counter = i32::from(self.walk_yaw_base) - drift(pass_n) - drift(pass_n1);
        let end6 = (counter - facing).rem_euclid(4096);
        let gap = (end6 - i32::from(yaw_n1) + 2048).rem_euclid(4096) - 2048;
        let yaw = (end6 - gap.signum() * stale_pass_gap(gap.abs(), s1, s2)).rem_euclid(4096);
        // TR z and the focus: from the framing of the action before.
        let [fx, fz] = live_pair(live.focus);
        let (sx, sz) = self.previous_actor_bearing([fx, fz], [fx_step, fz_step])?;
        let tr_z = end.tr[2] as i32 - stale_pass_gap(trz_step, s1, s2);
        Some(legaia_engine_camera::projection::BattleCamPose {
            pitch: pitch.rem_euclid(4096) as f32,
            yaw: yaw as f32,
            tr: [0.0, tr_y as f32, tr_z as f32],
            focus: [
                (fx + sx * stale_pass_gap(fx_step, s1, s2)) as f32,
                0.0,
                (fz + sz * stale_pass_gap(fz_step, s1, s2)) as f32,
            ],
        })
    }

    /// The signs `(x, z)` from `focus` towards the earlier actor of this round
    /// whose bearing best fits the focus record's unsigned steps: case 6 walks
    /// off the framing of the action before, in a straight line per component
    /// ratio. `None` on a round's first action, or with no spent seat.
    fn previous_actor_bearing(&self, focus: [i32; 2], steps: [i32; 2]) -> Option<(i32, i32)> {
        if self.turn_cursor == 0 {
            return None;
        }
        let party = 0..usize::from(self.party_count.clamp(1, 3));
        let monsters = 3..3 + usize::from(self.monster_count);
        let fit = party
            .chain(monsters)
            .filter(|&s| s != usize::from(self.active_actor))
            .filter(|&s| self.init_keys.get(s).copied().flatten() == Some(0))
            .filter_map(|s| {
                let [x, z] = (*self.ground.get(s)?)?;
                let v = [i32::from(x) - focus[0], i32::from(z) - focus[1]];
                let len = f64::from(v[0]).hypot(f64::from(v[1]));
                (len > 0.0).then(|| {
                    let cos = f64::from(v[0].abs() * steps[0] + v[1].abs() * steps[1]) / len;
                    (cos, v)
                })
            })
            .max_by(|a, b| a.0.total_cmp(&b.0))?;
        Some((fit.1[0].signum(), fit.1[1].signum()))
    }

    /// The pose the displayed frame stands on when a capture holds a one-frame
    /// shot on its second pass (`0x2B`, accumulator `2 * 8 * step`). The shot's
    /// arming pass is `N - 1` and it has landed, so the RAM holds nothing of
    /// the frame before it; the display's `N - 2` is the `0x2A` terminator
    /// pass, on the case-6 walk. That walk is law from three things the RAM
    /// keeps: where it ended, the steps of the passes it ran, and the earlier
    /// actor it set out from.
    ///
    /// The passes are the seed `0x0C` (which arms case 6 on the counter's
    /// `0x800` rung), `0x28`, the wait `0x29` draining its `0x14` timer by each
    /// pass's step, and the terminator, which re-arms nothing
    /// ([`case6_walk_passes`]). Each armed pass moves
    /// `ceil(gap / 0xC) * step`, the depth and focus closing on the caster.
    ///
    /// The start is the earlier actor's framing ([`Self::walk_start`]). The yaw
    /// of a turning shot comes off the shot ([`Self::retake_turning_yaw`]);
    /// elsewhere it walks from that actor's case-6 yaw with the caster's
    /// heading taken at home for the seed's arm, an inference.
    ///
    /// `pin` is what the RAM cannot say ([`displayed_walk_pin`]).
    pub fn retake_stale_walk_pose(
        &self,
        pin: Option<DisplayedWalkPin>,
    ) -> Option<legaia_engine_camera::projection::BattleCamPose> {
        if !self.retake_on_shot_only() || self.action_state != 0x2B || self.cam_style != 0 {
            return None;
        }
        let SpanGate::Age { accum } = self.span_gate else {
            return None;
        };
        let (step, _) = self.step_run;
        let shot_passes = if accum == 16 * u16::from(step) {
            2
        } else if accum == 8 * u16::from(step) {
            1
        } else {
            return None;
        };
        let (live, end) = self.cam_tween;
        let turning = self.retake_turning_yaw();
        let landed =
            live.tr == end.tr && live.focus[0] == end.focus[0] && live.focus[2] == end.focus[2];
        if !landed {
            return None;
        }
        let passes = case6_walk_passes(&self.ring_steps, shot_passes);
        let start = self.walk_start(pin)?;
        let caster = usize::from(self.active_actor);
        let [gx, gz] = (*self.ground.get(caster)?)?;
        let tr_z_end = i32::from(self.cam_depth) * 8 / 5;
        let walk = |from: i32, to: i32| case6_walk(from, to, &passes);
        let yaw = match (turning, pin.and_then(|p| p.yaw)) {
            (Some(y), _) => i32::from(y),
            (None, Some(y)) => i32::from(y),
            (None, None) => {
                let facing = i32::from((*self.facing.get(caster)?)?);
                case6_yaw_walk(start.yaw, facing, &passes)
            }
        };
        Some(legaia_engine_camera::projection::BattleCamPose {
            pitch: walk(start.pitch, 0).rem_euclid(4096) as f32,
            yaw: yaw as f32,
            tr: [
                0.0,
                walk(
                    start.tr_y,
                    i32::from(legaia_engine_battle_vm::battle_cam_script::ACTION_TR_Y as i16),
                ) as f32,
                walk(start.tr_z, tr_z_end) as f32,
            ],
            focus: [
                walk(start.focus[0], i32::from(gx)) as f32,
                0.0,
                walk(start.focus[1], i32::from(gz)) as f32,
            ],
        })
    }

    /// Where the case-6 walk of this capture's actor set out: the framing the
    /// earlier actor's `0x5A` armed, or the prompt's far framing on a round's
    /// first actor. The selector `FUN_801DABA4` leaves no battle order behind,
    /// only spent keys, but the gauge panel's raise names the last party
    /// member to act on a Spirit ([`RetailBattle::gauge_actor`]); the style
    /// byte `ctx[+0xD]` of that action is gone, so the style comes from a pin,
    /// or else the plain one. A capture whose byte names no member falls back
    /// on the last spent party seat. A party seat's Spirit frames it at the party's
    /// depth `0x800`. `None` where the earlier action is one whose framing the
    /// RAM does not name.
    fn walk_start(&self, pin: Option<DisplayedWalkPin>) -> Option<WalkStart> {
        let party = 0..usize::from(self.party_count.clamp(1, 3));
        let monsters = 3..3 + usize::from(self.monster_count);
        let spent = |s: &usize| {
            *s != usize::from(self.active_actor)
                && self.init_keys.get(*s).copied().flatten() == Some(0)
        };
        if self.turn_cursor == 0 {
            let mut bbox: Option<legaia_engine_battle_vm::battle_cam_script::FormationBox> = None;
            for s in party.chain(monsters) {
                let [x, z] = (*self.ground.get(s)?)?;
                let (x, z) = (f32::from(x), f32::from(z));
                let b =
                    bbox.get_or_insert(legaia_engine_battle_vm::battle_cam_script::FormationBox {
                        min: [x, z],
                        max: [x, z],
                    });
                b.min = [b.min[0].min(x), b.min[1].min(z)];
                b.max = [b.max[0].max(x), b.max[1].max(z)];
            }
            let b = bbox?;
            let raw = legaia_engine_battle_vm::battle_cam_script::menu_raw_z(Some(b));
            // The prompt's orbit is a clock the RAM does not date: only a turning
            // shot, or a capture whose yaw is pinned, takes this start.
            if pin.and_then(|p| p.yaw).is_none() {
                self.retake_turning_yaw()?;
            }
            return Some(WalkStart {
                pitch: 32,
                yaw: 0,
                tr_y: 1280,
                tr_z: raw * 8 / 5,
                focus: [
                    ((b.min[0] + b.max[0]) * 0.5) as i32,
                    ((b.min[1] + b.max[1]) * 0.5) as i32,
                ],
            });
        }
        let seat = match self.gauge_actor {
            Some(s) if s != self.active_actor => usize::from(s),
            _ => party.clone().rfind(spent)?,
        };
        // Style `2` raises the pitch `0x80` and drops TR y `0x100`.
        let (pitch, tr_y) = match pin.map(|p| p.style) {
            Some(2) => (128, 1024),
            _ => (0, 1280),
        };
        if self.seat_category.get(seat).copied().flatten() != Some(4) {
            return None;
        }
        let [x, z] = (*self.ground.get(seat)?)?;
        let facing = i32::from((*self.facing.get(seat)?)?);
        Some(WalkStart {
            pitch,
            yaw: (SPIRIT_END_COUNTER - facing).rem_euclid(4096),
            tr_y,
            tr_z: 0x800 * 8 / 5,
            focus: [i32::from(x), i32::from(z)],
        })
    }

    /// A monster's capture-band gate ([`SpanGate::CaptureFade`]): captured
    /// within the display lag of the band's entry, its displayed frame is a
    /// pass before the band's first arm, whose parts retail had not spawned
    /// on screen (`battle_map01_lippian_kiss_of_death_mid_cast`: PROT 0955's
    /// shell parts stand on the gate's frame and on none of retail's).
    pub fn retake_across_band_entry(&self) -> bool {
        matches!(
            (self.seed_plan(), self.span_gate),
            (SeedPlan::Action { seat, .. }, SpanGate::CaptureFade { .. }) if seat >= 3
        ) || self.results_opened_inside_lag()
    }

    /// The display lag in the image child's own ticks: two of its passes,
    /// for the shot retake ([`Self::retake_on_shot_only`]). A pad drive runs
    /// the engine's default battle frame step unless the drive seeds
    /// retail's ([`Self::frame_step_seed`]), so where retail ran another
    /// step its [`Self::display_lag`] (two of retail's passes, in vsyncs) is
    /// not two of the child's passes: Barra's Windcutter ran step `3`, and
    /// six ticks back from the gate is the child's `0x29` exit pass, a pass
    /// before the frame retail's display held.
    ///
    /// A seed held on the accumulator ([`Self::step_run_accum_hold`]) runs
    /// only the run's passes on its step: of the two frames behind the gate,
    /// one past the run's start keeps the ordinary step.
    pub fn drive_display_lag(&self) -> u16 {
        let ordinary = self.child_ordinary_step();
        let Some((step, _, _, hold)) = self.frame_step_seed() else {
            return DISPLAY_LAG_PASSES * u16::from(ordinary.max(1));
        };
        let seeded = hold.map_or(usize::MAX, |_| self.step_run.1);
        (0..usize::from(DISPLAY_LAG_PASSES))
            .map(|k| {
                // The frame `k + 1` passes behind the gate: the engine opens a
                // seeded frame on each run pass after the first.
                if k + 1 < seeded {
                    u16::from(step.max(1))
                } else {
                    u16::from(ordinary.max(1))
                }
            })
            .sum()
    }

    /// Whether the image child's camera is aligned to the pose the
    /// displayed frame was drawn at ([`Self::displayed_cam_live`]) rather
    /// than the RAM's. Every action drive but a [`SpanGate::DoneHold`]:
    /// the Done band's captures part on where the engine's replay left
    /// the bodies, and walking the camera back moved `noa_levelup_fight_pre`
    /// off retail's. Elsewhere it rose wherever it moved - with the frame
    /// itself left at the RAM's instant too (`battle_gimard_tail_fire_b`
    /// `0.745` to `0.772`). A command-menu drive takes it too: the menu's
    /// close-up is still easing in on most captures, and on the displayed
    /// pose `cort_evolved_battle_first_menu` reads `0.988` against `0.947`,
    /// `koru_strip_forced` `0.945` against `0.904`, `battle_gaza2_prompt`
    /// `0.998` against `0.984`, with no menu capture falling.
    ///
    /// A Done-band capture takes the displayed pose only where the engine's
    /// glide heads for retail's framing on every component
    /// ([`Self::displayed_camera_on_agreement`]): it then walks retail's own
    /// step table, and the displayed pose is one it passes through.
    pub fn displayed_camera(&self) -> bool {
        matches!(
            self.seed_plan(),
            SeedPlan::Action { .. } | SeedPlan::Menu { .. }
        ) && !matches!(self.span_gate, SpanGate::DoneHold { .. })
    }

    /// The Done band's conditional twin of [`Self::displayed_camera`]: the
    /// image child takes the displayed pose when its glide's endpoint
    /// agrees with retail's on every component
    /// (`BattleCamera::glide_endpoint_matches`), and the RAM's otherwise.
    /// `zora_glare_petrify_post` (the caster's case 6, every endpoint
    /// exact) reads `0.862` on the RAM's pose and `0.996` on the displayed
    /// one; `noa_levelup_fight_pre`'s case 8 lands on a target pushed
    /// elsewhere and keeps the RAM's.
    pub fn displayed_camera_on_agreement(&self) -> bool {
        matches!(self.seed_plan(), SeedPlan::Action { .. })
            && matches!(self.span_gate, SpanGate::DoneHold { .. })
    }

    /// Whether a replayed cast's camera takes retail's pose as its glide
    /// origin (`BattleCamera::align_glide_origin`), as a pad drive's does
    /// ([`Self::displayed_camera`]): a capture in the summon invoke `0x32`,
    /// which re-arms `FUN_801D5854(actor, 6)` every pass (`0x801E4984`) on
    /// the ease `0x29`'s route arm opened onto the summon close-up's depth.
    /// That ease sets out from whatever the fight framed before the cast -
    /// the round's history, which the replay, seeded at the dispatch, does
    /// not share (`flute_lippian_midcast`: retail's pitch `10` closing on
    /// `0` from above, the replay's glide having set out at `0`).
    ///
    /// The headless seed takes retail's live pose, which the `camera`
    /// channel scores. The image child takes the pose the **displayed**
    /// frame was drawn at - the live pose walked back two game frames along
    /// the step table ([`Self::displayed_cam_live`]): retail's RAM is one
    /// pass into the ease, and the frame on screen is older than it. Aligned
    /// to the RAM's pose the frame lands on the wrong side of it
    /// (`flute_spikefish_midcast` `.910` to `.810`); to the displayed one
    /// both flute captures rise, once the walk-back steps onto the `0x29`
    /// table the route arm replaced ([`summon_route_prior_end`]). The
    /// summon close-up keeps the replay's own pose; a module shot still
    /// walking is aligned on its own law ([`Self::module_cam_aligned`]).
    pub fn cast_glide_aligned(&self) -> bool {
        matches!(self.seed_plan(), SeedPlan::Cast) && self.action_state == 0x32
    }

    /// A capture-band `0x70` capture inside a module shot still walking:
    /// retail's endpoints and the vsyncs its table has left
    /// (`ceil(gap / increment)` on the slowest component). A `--physical`
    /// cast film opens on the state's own instant and stands the camera on
    /// the state's live pose; its module shot was built by the replay's own
    /// earlier pass, from wherever the replay's camera stood, so the film
    /// re-arms it onto retail's table from the seated pose
    /// (`LEGAIA_SEAT_MODULE_SHOT`). Harness seat: a still takes its frame on
    /// the gate, before the table walks.
    pub fn module_shot_seat(
        &self,
    ) -> Option<(legaia_engine_camera::projection::BattleCamPose, u16)> {
        if self.action_state != 0x70 || self.module_phase == 0 || self.cam_landed {
            return None;
        }
        let (live, end) = self.cam_tween;
        let wrap = |d: f32| (d + 2048.0).rem_euclid(4096.0) - 2048.0;
        let gaps = [
            wrap(end.pitch - live.pitch),
            wrap(end.yaw - live.yaw),
            0.0,
            end.tr[0] - live.tr[0],
            end.tr[1] - live.tr[1],
            end.tr[2] - live.tr[2],
            end.focus[0] - live.focus[0],
            end.focus[1] - live.focus[1],
            end.focus[2] - live.focus[2],
        ];
        let left = (0..9)
            .filter(|&k| k != 2 && self.cam_tween_steps[k] > 0)
            .map(|k| {
                let inc = f32::from(self.cam_tween_steps[k]);
                (gaps[k].abs() / inc).ceil() as u16
            })
            .max()
            .filter(|&n| n > 0)?;
        Some((end, left))
    }

    /// Whether a replayed cast's **module shot** takes retail's pose
    /// (`BattleCamera::align_module_shot`): a capture in the summon band's
    /// sustain / return (`0x35` / `0x36`), standing on an arm that polls the
    /// creature stream (`cast_module_camera::creature_poll_arm`), with
    /// retail's shot still walking. The shot armed ahead of the poll walks
    /// on through retail's read, which the replay's resident stream never
    /// waits on, so its progress at the capture is the read's length
    /// (`vera_summon_mid_cast`, `72` vsyncs into PROT 0905's arm-1 shot:
    /// `image` `.985` to `.996`). The headless seed takes the RAM's live
    /// pose; the image child the pose walked back to the displayed frame
    /// along the shot's own table ([`Self::displayed_module_cam_live`]).
    /// Elsewhere the module's own clocks place its shot: aligned on PROT
    /// 0930's side-band wait, `horn_summon_mid_cast` read `.976` against
    /// `.988`.
    ///
    /// The same holds for a capture-class shot armed past a walk-in the
    /// replay does not run ([`Self::walk_in_shot_aligned`]).
    pub fn module_cam_aligned(&self) -> bool {
        let entry = u32::from(self.queued_action).wrapping_sub(0x81) + 903;
        (matches!(self.seed_plan(), SeedPlan::Cast)
            && (0x81..=0xA0).contains(&self.queued_action)
            && matches!(self.action_state, 0x35 | 0x36)
            && legaia_engine_cast::cast_module_camera::creature_poll_arm(entry)
                == Some(self.module_phase)
            && !self.cam_landed)
            || self.walk_in_shot_aligned()
    }

    /// A `0x70` capture of a body whose walk-in frames the caster through
    /// case 6 every pass (PROT 0962's charge bodies,
    /// `cast_module_camera::capture_approach_follows`), saved past the walk
    /// with its arm-2 shot still walking. The arming routine takes the
    /// shortest arc from the live camera (`FUN_80021248`), so the shot sets
    /// out from wherever the walk-in's framing had swung: in a retail film
    /// of the walk (`koru_pre_thunder_needle`, `36` vsyncs of arm 1 from
    /// Koru's seat) the half-turned case-6 framing lands on yaw `2168` and
    /// the shot leaves from there. The replay stands the caster on the
    /// capture's walked ground, already in reach, so its arms 0..2 run in
    /// one tick and the shot leaves from the unturned yaw instead; the
    /// shot's own table places it (`koru_blade_breath_mid_cast`, two frames
    /// into the arm-2 shot).
    pub fn walk_in_shot_aligned(&self) -> bool {
        self.action_state == 0x70
            && self.module_phase >= 2
            && !self.cam_landed
            && (0xA2..=0xA4).contains(&self.queued_action)
            && legaia_engine_cast::cast_module_camera::capture_approach_follows(
                962,
                legaia_engine_cast::cast_module_ticks::capture_tick_body(962, self.queued_action),
            )
    }

    /// A `0x70` capture inside a walk-in body's walk arm (arm 1, the case-6
    /// follow re-armed every pass, `capture_approach_follows`), its follow
    /// still walking: the headless channel's live camera takes retail's
    /// (`BattleCamera::align_module_shot`). The follow's endpoint is the
    /// RAM's; where it set out from is the replay's own history.
    /// `battle_tunnelc_xain_bloody_horns_mid_cast` sits one pass into Bloody
    /// Horns' walk, the engine's follow at yaw `3237` heading for `3653`
    /// where retail's stood at `2986` heading for `3658` (`camera` `.878` to
    /// `1.000`). The image child keeps the displayed pose its
    /// `LEGAIA_BATTLE_CAM_ALIGN` seats: walking the follow's table back
    /// stops on the last re-arm, not on the frame on screen.
    pub fn walk_arm_follow_aligned(&self) -> bool {
        self.action_state == 0x70
            && self.module_phase == 1
            && !self.cam_landed
            && legaia_engine_cast::cast_module_ticks::capture_body_for_action(self.queued_action)
                .is_some_and(|(entry, body)| {
                    legaia_engine_cast::cast_module_camera::capture_approach_follows(
                        entry,
                        Some(body),
                    )
                })
    }

    /// Every captured combatant's live pair and heading by engine slot - the
    /// ground the capture holds, not the seed's (which [`Self::undrift`]
    /// may have moved) - for the phase's ground seat
    /// ([`legaia_engine_capture::ground_snap`]).
    pub fn engine_ground_snaps(&self) -> Vec<legaia_engine_capture::ground_snap::GroundSnap> {
        self.ground_snaps(false, false, false)
    }

    /// Whether the capture sits on a monster strike's arrival pass: `0x1E`
    /// just stored by the poll `0x19`, no strike-loop pass run (cursor
    /// `ctx[+0x15] == 0`), on the walk clip ([`Self::walk_clock_arrival`])
    /// or with the staged idle already committed (accumulator `0`). The
    /// table is the poll's case-6 arm and the counter the walk's last
    /// reading.
    pub fn arrival_pass(&self) -> bool {
        self.walk_clock_arrival()
            || matches!(self.seed_plan(), SeedPlan::Action { seat, state: 0x1E } if seat >= 3)
                && self.queued_category == 3
                && self.strike_cursor == 0
                && self.span_gate == SpanGate::Age { accum: 0 }
    }

    /// [`Self::engine_ground_snaps`] with the acting seat on the pair its
    /// pass began on ([`Self::arm_ground`]), for the framing endpoint an
    /// [`Self::arrival_pass`] capture's table was armed against. The poll's
    /// case 6 frames the attacker's live pair before the pass's anim tick
    /// steps it back off the arrival (the walk clip's cut, `0x80047950`):
    /// `battle_geremi_spikefish_strike`'s table ends on `(-188, 321)`, its
    /// ring slot `1`, where the RAM holds `(-219, 417)`. `None` elsewhere.
    pub fn arrival_arm_snaps(&self) -> Option<Vec<legaia_engine_capture::ground_snap::GroundSnap>> {
        if !self.arrival_pass() {
            return None;
        }
        let pool = usize::from(self.active_actor);
        let arm = self.arm_ground?;
        let pc = usize::from(self.party_count.clamp(1, 3));
        let slot = u8::try_from(pc + pool.checked_sub(3)?).ok()?;
        let mut snaps = self.engine_ground_snaps();
        let s = snaps.iter_mut().find(|s| s.slot == slot)?;
        s.x = arm[0];
        s.z = arm[1];
        // Case 6 frames the live pair itself, so no body-pair seat.
        s.body = None;
        // The arm also read the heading the pass before left: the pass
        // re-faces the attacker after its framing call, so `+0x46` already
        // holds the new bearing. Case 6's yaw is `counter - heading`, plus
        // the half turn on style bit 0, so the table and the counter name
        // the heading it read (`battle_dolk_gimard_strike` `1950` where the
        // RAM holds `2019`).
        let half = if self.cam_style & 1 != 0 { 0x800 } else { 0 };
        let read = (i32::from(self.walk_yaw_base) + half - self.cam_tween.1.yaw as i32) & 0xFFF;
        s.facing = u16::try_from(read).ok();
        Some(snaps)
    }

    /// [`Self::engine_ground_snaps`] for the image child, whose frame is the
    /// **displayed** one. On the action seed's own pass - `0x14` with the
    /// yaw counter still on the seed's `0x200` rung ([`tween_passes_back`]) -
    /// each combatant stands on the pair the position ring holds for that
    /// frame ([`Self::displayed_ground`]), a body pair moved with it: the
    /// seed recentres the formation (`jal 0x801EFE44` at `0x801E2D44`) on
    /// the RAM's pass, after the frame on screen was built.
    /// `super_queue_replace_*` hold Vahn `(+36, +45)` and Gobu Gobu
    /// `(+36, +52)` off the pairs that frame drew them on (`.938` to
    /// `.996`).
    ///
    /// Elsewhere the RAM's pairs stay for a body moving on its own clip's
    /// root motion, which is drawn from the replay's pose at the displayed
    /// tick: `battle_melee_hit_spark`'s Vahn (`+5` a frame on Somersault)
    /// reads better on the RAM's body pair than on the ring's.
    ///
    /// A struck monster stood on its body pair (a reaction clip `2..=5` /
    /// `0x0B`, not the acting seat) is the exception: the hit's push carries
    /// it, so its pose centroid holds across the two frames and the body
    /// pair the screen drew is the RAM's moved by the ring's delta.
    /// `battle_melee_hit_spark`'s Gimard is pushed `+11` a frame: his RAM
    /// body pair `(-2, 492)` stands at `(-2, 470)` on the displayed frame
    /// (`image` `.814` to `.847`).
    ///
    /// So is a monster still walking in the strike loop
    /// ([`Self::walk_clock_capture`]): its root motion carries it `speed *
    /// scale / 8` a vsync, and the frame on screen drew it the ring's pair
    /// back along the walk (`battle_korb2_warhalf_strike`'s Warhalf).
    pub fn engine_image_ground_snaps(&self) -> Vec<legaia_engine_capture::ground_snap::GroundSnap> {
        self.ground_snaps(
            self.action_state == 0x14 && self.walk_yaw_base == 0x200,
            true,
            self.walk_clock_capture(),
        )
    }

    /// The acting party seat's live pair on the displayed frame, for the
    /// image child to stand the body on **after** the camera is aligned
    /// (`LEGAIA_BATTLE_DISPLAYED_SEAT`), when the seat committed a swing
    /// between the displayed frame and the RAM's in the strike loop
    /// ([`Self::displayed_recommit`]). The commit's range step
    /// (`FUN_8004AD80`, `0x8004BF7C..0x8004C118`) moves the live pair in one
    /// pass, after the frame on screen was built, so that frame drew the
    /// body on the ring's pair. The camera stays aligned on the RAM's ground,
    /// as for every other action drive: retail's framing read the body pair
    /// of an earlier pass, which the RAM's ground reproduces
    /// (`rim_elm_gimard_victory`: Vahn drawn at `(12, -217)` on the `0x0F`
    /// swing's cursor `144`, stepped to `(3, -154)` by the `0x0E` commit).
    pub fn engine_image_displayed_seats(
        &self,
    ) -> Vec<legaia_engine_capture::ground_snap::GroundSnap> {
        // A monster whose strike clip committed inside the display lag drew
        // the clip before it, on the ring's pair
        // ([`Self::engine_image_displayed_clips`]).
        // A displayed frame that is the walk's last pass drew the walk's
        // heading ([`RetailBattle::displayed_strike_heading`]).
        if let Some((seed, then)) = self.displayed_strike_clip() {
            let then = then.or_else(|| {
                self.displayed_ground
                    .get(usize::from(self.active_actor))
                    .copied()
                    .flatten()
            });
            if let Some(then) = then {
                return vec![legaia_engine_capture::ground_snap::GroundSnap {
                    slot: seed.slot,
                    x: then[0],
                    z: then[1],
                    facing: self.displayed_strike_heading,
                    body: None,
                }];
            }
        }
        let pool = usize::from(self.active_actor);
        // A monster on the clip the frame on screen showed, which moved
        // inside the lag all the same: the arrival's idle stage committed on
        // the displayed pass and the loop's passes since backed it off its
        // target (`battle_jou_cort_181_strike`: ring `(-201, 530)` displayed,
        // `(-206, 546)` live, idle record on slots `0..=2`).
        if let Some(seat) = self.displayed_monster_strike_seat() {
            return vec![seat];
        }
        if pool >= 3
            || pool >= usize::from(self.party_count.clamp(1, 3))
            || !(0x1E..=0x20).contains(&self.action_state)
            || self.span_gate.is_end()
            || !self.displayed_recommit.get(pool).copied().unwrap_or(false)
        {
            return Vec::new();
        }
        let Some(Some(then)) = self.displayed_ground.get(pool) else {
            return Vec::new();
        };
        vec![legaia_engine_capture::ground_snap::GroundSnap {
            slot: pool as u8,
            x: then[0],
            z: then[1],
            facing: None,
            body: None,
        }]
    }

    /// [`Self::engine_image_displayed_seats`]' seat for a monster strike
    /// whose displayed clip is the RAM's (no commit inside the lag, not a
    /// walk on the clock) but whose live pair moved inside the lag: the
    /// displayed frame's ring pair.
    fn displayed_monster_strike_seat(
        &self,
    ) -> Option<legaia_engine_capture::ground_snap::GroundSnap> {
        let pool = usize::from(self.active_actor);
        let strike = matches!(self.seed_plan(), SeedPlan::Action { state: 0x1E, .. })
            && self.queued_category == 3
            && matches!(self.span_gate, SpanGate::Age { .. });
        if pool < 3
            || !strike
            || self.walk_clock_capture()
            || self.displayed_strike_clip().is_some()
        {
            return None;
        }
        let m = pool - 3;
        if m >= usize::from(self.monster_count) {
            return None;
        }
        let then = (*self.displayed_ground.get(pool)?)?;
        let live = self.captured.get(pool).copied().flatten()?.0;
        (then != live).then(|| legaia_engine_capture::ground_snap::GroundSnap {
            slot: self.party_count.clamp(1, 3) + m as u8,
            x: then[0],
            z: then[1],
            facing: None,
            body: None,
        })
    }

    /// The acting monster's clip on the **displayed** frame, for the image
    /// child to pose it on (`LEGAIA_SEAT_DISPLAYED_CLIPS`,
    /// `World::seat_displayed_battle_clip`): slot two of the history rings
    /// ([`Self::displayed_clips`]), in the strike loop of a monster's
    /// Attack gated on its clip age.
    ///
    /// Such a gate keeps its own frame (the hit rays and the rest of the
    /// shot run on the engine's clock), so the child draws the attacker on
    /// the clip the RAM holds. Two captures show another one:
    ///
    /// - **A commit inside the display lag.** On the strike loop's first
    ///   pass (`0x1E` at accumulator `0`) the RAM holds the strike entry at
    ///   the commit's cursor `0`, while the frame on screen predates the
    ///   commit: the rings show the arrival's idle stage committed one pass
    ///   before the strike, at cursor `0` on the displayed slot and `24` on
    ///   the next (`battle_noaru_gilium_lv2_strike`: entry `0` at `0`, then
    ///   `24`, then the strike entry `8` at `0` on the live slot; the same
    ///   on every first-pass monster strike in the library). The walk back
    ///   from the RAM's cursor cannot see across a commit, so the ring
    ///   names the clip. Posed on the idle's `24` instead, the Gilium reads
    ///   `.840` against `.902` on its `0`.
    /// - **A walk the engine has already ended.** A capture still on its
    ///   walk ([`Self::walk_clock_capture`]) whose replay reached the clock
    ///   on the strike clip (`battle_map02_mad_ogre_strike`'s Mad Ogre on
    ///   entry `7` at cursor `16`, retail's on its walk at `24`): the
    ///   cursor seat lands only on the clip the body plays.
    /// - **A walked-in charge cast** ([`Self::walk_in_shot_aligned`]), whose
    ///   image child keeps the gate's frame: the module's arm committed the
    ///   wind-up after the frame on screen was built, which still shows the
    ///   walk-in's run clip (`koru_blade_breath_mid_cast`: entry `5` at `0`
    ///   live, the run entry `1` at `136` displayed; `.943` to `.976`).
    pub fn engine_image_displayed_clips(
        &self,
    ) -> Vec<legaia_engine_capture::clip_cursors::ClipCursorSeed> {
        self.displayed_strike_clip()
            .map(|(seed, _)| vec![seed])
            .unwrap_or_default()
    }

    /// [`Self::engine_image_displayed_clips`]' seed, with the displayed
    /// frame's live pair of the seat when the clip committed inside the
    /// lag (a walker's ring pair is the ground snap's already).
    fn displayed_strike_clip(
        &self,
    ) -> Option<(
        legaia_engine_capture::clip_cursors::ClipCursorSeed,
        Option<[i16; 2]>,
    )> {
        let pool = usize::from(self.active_actor);
        let strike = matches!(self.seed_plan(), SeedPlan::Action { state: 0x1E, .. })
            && self.queued_category == 3
            && matches!(self.span_gate, SpanGate::Age { .. });
        // A walked-in cast whose image child keeps the gate's frame
        // ([`Self::walk_in_shot_aligned`]): the body's wind-up commits on
        // the arm the RAM is in, after the frame on screen was built, so
        // the display still shows the run clip the walk-in played.
        let walked_cast = self.walk_in_shot_aligned();
        // A walk-in body one pass into its walk arm
        // ([`Self::walk_arm_follow_aligned`]) whose walk clip committed
        // inside the display lag: arm 0 queues the walk with the `+0x1DC`
        // bump (PROT 0952's `sb v1,0x1da(s2)` / `sb v0,0x1dc(s2)` at
        // `0x801F72C8..0x801F72D0`), which commits it mid-clip on the next
        // anim tick, after the frame on screen was built, so that frame
        // still shows the caster's idle on the ring's pair. Only a commit
        // the rings show counts (`committed` below).
        let walk_arm = self.walk_arm_follow_aligned();
        // A monster cast's accumulator gate: the frame the gate stops on
        // carries the clip the RAM's accumulator pairs with, two passes past
        // the keyframe the screen shows, and the cast clip is a one-shot the
        // cursor seat leaves alone.
        //
        // A monster's Spirit / Item band (a class-0 Heal or Hyper Heal,
        // `0x3D..=0x40`) is the same clock: its cast clip commits at `0x3D`
        // and plays through `0x3E` on the gate's accumulator, so the RAM's
        // cursor is the display lag past the keyframe on screen
        // (`battle_ropeway_orb_heal_mid_cast`: cursor ring `40, 24, 12`, the
        // frame shows `12`).
        let magic_band = self.monster_magic_band_age() || self.monster_spirit_band_age();
        // A monster's capture-band module arm (`0x70`, past the walk-in
        // cases above) poses the caster on the displayed slot too: the
        // retake's lag is `2 * frame_step` vsyncs, and on a pass the history
        // stores as the clamp `0x2BC` the frame step reads `3` where the pass
        // ran `4`, so the replay's frame stands a pass-share short of the
        // screen while the rings name the keyframe it showed
        // (`cort_evolved_card_doomsday_mid_cast`: cursor ring `128, 96, 64`
        // on clip `7`, `32` a pass; the retake drew `80`, the ring's `64`
        // reads `image` `.894` to `.997`).
        let band_cast = self.action_state == 0x70 && !walk_arm && !walked_cast;
        let magic_band = magic_band || band_cast;
        if pool < 3 || !(strike || walked_cast || walk_arm || magic_band) {
            return None;
        }
        let (clip, cursor) = (*self.displayed_clips.get(pool)?)?;
        let committed = clip != self.caster_clip;
        if walk_arm && !committed {
            return None;
        }
        if !committed && !magic_band && (walked_cast || !self.walk_clock_capture()) {
            return None;
        }
        let m = pool - 3;
        if m >= usize::from(self.monster_count) {
            return None;
        }
        let slot = self.party_count.clamp(1, 3) + m as u8;
        let then = if committed {
            Some((*self.displayed_ground.get(pool)?)?)
        } else {
            None
        };
        Some((
            legaia_engine_capture::clip_cursors::ClipCursorSeed {
                slot,
                clip,
                cursor: i32::from(cursor),
            },
            then,
        ))
    }

    /// `displayed` stands every combatant on the displayed frame's pair,
    /// `displayed_struck` only a struck monster stood on its body pair,
    /// `displayed_walker` the acting seat.
    fn ground_snaps(
        &self,
        displayed: bool,
        displayed_struck: bool,
        displayed_walker: bool,
    ) -> Vec<legaia_engine_capture::ground_snap::GroundSnap> {
        let pc = usize::from(self.party_count.clamp(1, 3));
        self.captured
            .iter()
            .enumerate()
            .filter_map(|(pool, c)| {
                let (mut live, mut body, clip, _) = (*c)?;
                let slot = if pool < 3 {
                    (pool < pc).then_some(pool)?
                } else {
                    let m = pool - 3;
                    (m < usize::from(self.monster_count)).then_some(pc + m)?
                };
                let active = pool == usize::from(self.active_actor);
                let struck = !active && pool >= 3 && matches!(clip, 2..=5 | 0x0B);
                if (displayed || (displayed_struck && struck) || (displayed_walker && active))
                    && let Some(Some(then)) = self.displayed_ground.get(pool)
                {
                    let d = [then[0].wrapping_sub(live[0]), then[1].wrapping_sub(live[1])];
                    body = [body[0].wrapping_add(d[0]), body[1].wrapping_add(d[1])];
                    live = *then;
                }
                Some(legaia_engine_capture::ground_snap::GroundSnap {
                    slot: u8::try_from(slot).ok()?,
                    x: live[0],
                    z: live[1],
                    facing: self.facing.get(pool).copied().flatten(),
                    body: (if active {
                        (0x1E..=0x20).contains(&self.action_state)
                    } else {
                        struck
                    })
                    .then_some(body),
                })
            })
            .collect()
    }

    /// [`Self::clip_cursors`] on engine slots: the party's pool slots
    /// `0..party_count` as they are, the monster row `3..` packed behind
    /// the party, as the engine seats it.
    /// A monster parked in the approach poll with its walk clip dead: a live
    /// approach in `0x19` always plays its Move clip, so `+0x1D9 == 0` there
    /// is the park.
    fn parked_approach(&self) -> bool {
        self.active_actor >= 3
            && self.action_state == 0x19
            && self.queued_category == 3
            && self.caster_clip == 0
    }

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
            //
            // A monster target is seated on one reaction only: the get-up a
            // killed monster rises on for the absorb, with a Seru staged in
            // `ctx[+0x269]` (the commit's death arm, `0x8004B688..0x8004B6A0`).
            // Its knockdown ran on the replay's clock too, and the replay's
            // may still be falling where retail's has ended - the world
            // carries it on to the get-up (`World::seat_battle_clip_cursor`,
            // `rim_elm_gimard_seru_capture_before`). A monster's other
            // reactions stay on the replay's clock: seated on its knockdown,
            // `rim_elm_gimard_victory`'s Gimard read `.888` to `.883`.
            .filter(|&&(slot, clip, cursor)| {
                let staged = !(self.action_state == 0x0C
                    || self.action_state == 0x14 && self.walk_yaw_base == 0x200);
                // `0xFD` / `0xFF` (idle hold, round boundary) are where the
                // action SM sits through the command flow: no action runs,
                // so the commanding member's idle loop is a clock like any
                // other seat's (`delilas_che_hammer_gaps_v3`'s Che).
                // `0x0A` / `0x0B` are the setup passes ahead of the seed `0x0C`:
                // the acting seat has staged nothing yet, so its idle loop is a
                // clock too (`rikuroa_caruban_pre_fire_breath`'s Caruban, a
                // close-up its sway fills, reads `.675` on the replay's cursor
                // and `.938` on retail's).
                let in_flight = !matches!(self.action_state, 0 | 0x0A | 0x0B | 0xFD | 0xFF)
                    && staged
                    && !(0x50..=0x5A).contains(&self.action_state);
                let capture_band_idle = clip == 0 && (0x6F..=0x71).contains(&self.action_state);
                // So is a seat idling through a summon's invoke and freeze
                // (`0x32..=0x34`): the band stages no clip on the victim
                // before the module's hit, so its idle cursor is time since
                // the loop began - a clock the replay reaches on its own
                // (`gimard_summon_start`'s Gobu Gobu).
                let summon_band_idle = clip == 0 && (0x32..=0x34).contains(&self.action_state);
                let target_reaction = slot == self.target_code
                    && cursor >= 0
                    && if slot < 3 {
                        matches!(clip, 2..=5 | 0x0B)
                    } else {
                        clip != 0 && self.absorbed_seru != 0
                    };
                // So is a monster parked in the approach poll on its idle
                // ([`ActionSteer::park`]): the flinch's stale bit turned its
                // walk into the idle loop, which has run on its own clock
                // since (`battle_gaza2_park_0x19_target_vahn`'s Gaza).
                let parked_idle = clip == 0 && slot == self.active_actor && self.parked_approach();
                // ... and a monster still on its walk in the strike loop
                // ([`Self::walk_clock_capture`]): the walk loops on its own
                // clock, re-committing at each cycle end, and the frame on
                // screen shows the cycle two game frames back.
                let walking = clip == 1 && slot == self.active_actor && self.walk_clock_capture();
                // ... and a monster melee's target still idling ahead of the
                // hit: its loop is a clock of its own until the strike lands
                // and stages its reaction (`battle_korb2_warhalf_strike`'s
                // Vahn, walked onto at idle cursor `0` on the displayed
                // frame). A target idling in the RAM was not struck inside
                // the display lag either, so the walk back holds (a wrap of
                // the loop included).
                // A party strike's monster target is the same clock: nothing
                // the strike loop runs touches its loop before the hit
                // (`battle_ropeway_party_noa_sonic_javelin_open`'s Orb, whose
                // idle bob stands it a sphere's height off retail's). Only
                // ahead of the first swing's commit, though: that commit
                // steps the attacker along the range law to the target's
                // body pair (`FUN_8004AD80`, `0x8004BF7C..0x8004C118`), which
                // the replay's own idle phase decided, and a loop re-seated
                // after it stands the body where the step never measured
                // (`minigame_muscle_dome_retail_red_piura_vahn_strike`
                // `.987` to `.967`).
                let melee_target_idle = clip == 0
                    && slot == self.target_code
                    && self.queued_category == 3
                    && (self.active_actor >= 3 || self.caster_clip == 0)
                    && (0x14..=0x1E).contains(&self.action_state);
                // ... and a monster cast's party target still idling ahead of
                // the hit, the same clock: the cast band stages nothing on the
                // victim before its hit (`battle_nilboa_aluru_v2_aluru_cannon_mid_cast`'s
                // Vahn, a close-up whose sway the replay's own cursor missed).
                let cast_target_idle = clip == 0
                    && slot == self.target_code
                    && slot < 3
                    && self.active_actor >= 3
                    && self.queued_category == 2
                    && (0x28..=0x2F).contains(&self.action_state);
                // ... and a monster on its idle in the strike loop, with the
                // first swing staged behind it: the idle runs from the end of
                // the walk or close-in, whose passes the replay's clock does
                // not share (`battle_jou_cort_181_strike`).
                let strike_idle = clip == 0
                    && slot == self.active_actor
                    && self.active_actor >= 3
                    && self.action_state == 0x1E;
                !in_flight
                    || capture_band_idle
                    || strike_idle
                    || parked_idle
                    || walking
                    || melee_target_idle
                    || cast_target_idle
                    || summon_band_idle
                    || target_reaction
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

    /// The command surface the **displayed** frame shows, when it is not the
    /// one the RAM holds: a capture taken on the pass that opened the arts
    /// entry `0x50` ([`Self::hud_glides_unseen`] - its own spawns, the
    /// `Auto` / `Command` chips leaving and the directional chips arriving,
    /// all younger than the display lag) still shows the `Auto | Command`
    /// prompt `0x78` the player pressed Right on, at rest. Only under Select
    /// Attack `0`: the other options reach `0x50` straight from the ring.
    pub fn displayed_menu_flow(&self) -> Option<BattleFlowState> {
        match self.seed_plan() {
            SeedPlan::Menu {
                flow: BattleFlowState::ArtsCommandEntry,
                ..
            } if self.hud_glides_unseen && self.select_attack == 0 => {
                Some(BattleFlowState::AttackModePrompt)
            }
            _ => None,
        }
    }

    /// [`Self::battle_drive`] for the image child: a menu drive walks to the
    /// surface the displayed frame shows ([`Self::displayed_menu_flow`]),
    /// and an opening under the entry sweep is taken at the counter the
    /// displayed frame was built on - the RAM's less the double buffer's own
    /// lag ([`displayed_lag_vsyncs`]), since the frame driver advances
    /// `gp+0x330` by the frame step a pass. The battle's entry fade-in rides
    /// the same passes (`gaza2_card_battle_init`: the block at `25`, `102`
    /// on screen at step `3`, two passes back); `s5_tetsu_battle` was saved
    /// after the flip, and its screen is one pass back.
    pub fn image_drive(&self) -> Option<BattleDrive> {
        match (self.battle_drive(), self.displayed_menu_flow()) {
            (Some(BattleDrive::Menu { seat, spirit, .. }), Some(flow)) => Some(BattleDrive::Menu {
                flow,
                seat,
                spirit,
                pick: 0,
            }),
            (Some(BattleDrive::Opening { swept, entry }), _) if (0x80..=0xC0).contains(&entry) => {
                let lag = u8::try_from(self.display_phase_lag).unwrap_or(u8::MAX);
                Some(BattleDrive::Opening {
                    swept,
                    entry: entry.saturating_sub(lag).max(0x80),
                })
            }
            (drive, _) => drive,
        }
    }

    /// The engine slot whose action a driven action capture holds, for the
    /// re-seat at that action's seed (`BattleState::capture_reseat`): the
    /// drive's rounds walk the formation where the capture's history never
    /// did, and the strike framings aim at where the action stands. `None`
    /// on every other plan, and on a capture past the end signal.
    pub fn reseat_engine_seat(&self, party_count: u8) -> Option<u8> {
        match self.seed_plan() {
            SeedPlan::Action { seat, .. } if self.run_state != 0xFE => Some(
                legaia_engine_capture::battle_drive::engine_seat(seat, party_count.clamp(1, 3)),
            ),
            _ => None,
        }
    }

    /// The yaw counter `ctx[+0x6DA]` a driven monster-melee capture's
    /// attacker reached its target on, for the re-seat's approach start
    /// (`BattleState::capture_approach_yaw`).
    ///
    /// A monster's counter after the Attack arm has one rung, the `0x200`
    /// the arm stores (`0x801E2F20`): the swing-clip re-seed is a party
    /// seat's (`FUN_8004E13C`, gated on `ctx[+0x13] < 3`). From there it
    /// counts SM passes, `max(1, 4 * step / 3)` each (`0x801E29E4`), through
    /// the walk, the strike loop's first pass and its clips. The close-up
    /// accumulator (`ctx[+0x87C]`, `8 * step` a framing call) dates the clip
    /// the capture sits in, so the walk ended that many passes before the
    /// capture, plus the arrival pass and the strike loop's first; a
    /// capture still on the walk clip arrived inside its accumulator's
    /// window. Strike-loop
    /// states `0x1E..=0x20` of an Attack only; `None` elsewhere. Beside it,
    /// the engine slot of the capture's target `+0x1DD` - only a walk onto
    /// that body is the captured one - and both bodies' captured live pairs
    /// (not the seeds', which [`Self::undrift`] moves).
    pub fn capture_approach_yaw(
        &self,
    ) -> Option<legaia_engine_battle_loop::battle_state::CaptureApproach> {
        let SeedPlan::Action { seat, state } = self.seed_plan() else {
            return None;
        };
        if seat < 3 {
            return self.party_approach(state);
        }
        let SpanGate::Age { accum } = self.span_gate else {
            return None;
        };
        if self.queued_category != 3 || !(0x1E..=0x20).contains(&state) {
            return None;
        }
        let step = if self.step_run.1 >= 16 {
            self.step_run.0
        } else {
            2
        }
        .max(1);
        let inc = legaia_engine_battle_vm::battle_cam_script::action_yaw_pass_increment(step);
        // A capture still on the walk clip in the strike loop
        // ([`Self::walk_clock_capture`]): the accumulator counts from the
        // walk's last loop re-commit, which says nothing of the arrival. The
        // loop's framing (case 7, `0x801E36E8`) re-arms every pass, so the
        // table's yaw increment says how many passes have run
        // ([`strike_passes_off_table`]).
        let passes = if self.arrival_pass() {
            // Saved on the arrival pass itself: the poll `0x19` stored `0x1E`
            // and armed case 6 there (`0x801E35B4..0x801E35C8`), so the
            // table is the approach's, not case 7's ladder, and the counter
            // is the walk's last reading.
            0
        } else if self.walk_clock_capture() {
            strike_passes_off_table(self.cam_tween_steps[1], step)
        } else {
            accum / (8 * u16::from(step)) + 2
        };
        let target = legaia_engine_capture::battle_drive::engine_seat(
            self.target_code,
            self.party_count.clamp(1, 3),
        );
        let strike = self
            .captured
            .get(usize::from(self.active_actor))?
            .as_ref()?
            .0;
        let target_ground = self
            .captured
            .get(usize::from(self.target_code))?
            .as_ref()?
            .0;
        self.walk_yaw_base
            .checked_sub(passes * inc)
            .filter(|&y| y > 0x200)
            .map(
                |arrive| legaia_engine_battle_loop::battle_state::CaptureApproach {
                    arrive,
                    target,
                    strike,
                    target_ground,
                },
            )
    }

    /// [`Self::capture_approach_yaw`] for a party Attack captured in its
    /// Done band (`0x50` / `0x51`), once a first run has measured the
    /// string ([`Self::party_combo_vsyncs`]).
    ///
    /// A party seat's counter also climbs from the Attack arm's `0x200`
    /// when none of its entries can re-seed it (`swing_reseed_entry`), so
    /// `counter - 0x200` units are the action's whole span: the walk, then
    /// the string. The string's clips run as long on both sides - a
    /// PCSX-Redux replay of `cort_evolved_card_pre_doomsday` lands every
    /// clip of Gala's combo within a pass of the engine's - but at retail's
    /// coarser step, so the span is turned into vsyncs at the ring's rate
    /// ([`Self::ring_steps`]) and the replay's string taken off it. What is
    /// left is the walk, which retail ran from where state `0x0C`'s recentre
    /// stood the attacker (that replay: Gala from `(-985, -2047)` onto Cort
    /// on the origin, 21 passes, counter `512 -> 596`); the replay, seated
    /// near its end-of-string pair, arrived in a pass or two.
    ///
    /// The arrival reading is in the engine's own counter at step `2` (one
    /// a vsync). `strike` / `target_ground` are the captured end pairs, the
    /// walk's ray as the string's pushes left it; the engine anchors that
    /// ray on the target's re-seated pair (`World::seat_capture_approach_start`).
    fn party_approach(
        &self,
        state: u8,
    ) -> Option<legaia_engine_battle_loop::battle_state::CaptureApproach> {
        if self.queued_category != 3
            || self.swing_reseed_entry
            || !matches!(state, 0x50 | 0x51)
            || self.target_code < 3
            || self.target_code >= 8
        {
            return None;
        }
        let combo = self.party_combo_vsyncs?;
        let (units, vsyncs) = self.ring_steps.iter().fold((0u32, 0u32), |(u, v), &s| {
            let s = s.max(1);
            (
                u + u32::from(
                    legaia_engine_battle_vm::battle_cam_script::action_yaw_pass_increment(s),
                ),
                v + u32::from(s),
            )
        });
        let span = u32::from(self.walk_yaw_base.checked_sub(ATTACK_YAW_RUNG)?);
        let total = (span * vsyncs + units / 2) / units.max(1);
        let walk = total.checked_sub(combo)?;
        if walk < PARTY_WALK_MIN_VSYNCS {
            return None;
        }
        let target = legaia_engine_capture::battle_drive::engine_seat(
            self.target_code,
            self.party_count.clamp(1, 3),
        );
        let strike = self
            .captured
            .get(usize::from(self.active_actor))?
            .as_ref()?
            .0;
        let target_ground = self
            .captured
            .get(usize::from(self.target_code))?
            .as_ref()?
            .0;
        Some(legaia_engine_battle_loop::battle_state::CaptureApproach {
            arrive: ATTACK_YAW_RUNG.saturating_add(u16::try_from(walk).ok()?),
            target,
            strike,
            target_ground,
        })
    }

    /// Whether the art-name label the RAM holds is younger than the
    /// displayed frame: a party Attack whose latched id `+0x1DB` is the
    /// queue's first art constant (`>= 0x1B`), committed fewer vsyncs ago
    /// than the frame lags. The art-constant arm of the anim commit places
    /// and opens the label on that commit (`FUN_8004C650`, then
    /// `FUN_801D8DE8(0x4C, 0)`, `0x8004BB5C..0x8004BC40`), and the
    /// close-up accumulator `ctx[+0x87C]` the same commit zeroes counts `8`
    /// a vsync since. `minigame_muscle_dome_retail_skeleton_hyper_arts`
    /// holds the Hyper Art `0x1E` at accumulator `0`: its frame shows no
    /// label.
    ///
    /// A monster's swing opens the same label off its clip entry's `+0x77`
    /// (`0x8004AF08..0x8004AF88`): when its node committed inside the display
    /// lag (`displayed_recommit`) and the clip the displayed frame played
    /// names no move, that frame shows none either.
    /// `battle_map02_hell_s_trigger_strike` stands on its first swing's commit
    /// (cursor `0`): its RAM holds `Beak Slam` in records 76 / 77, its frame
    /// no label.
    pub fn move_label_unseen(&self) -> bool {
        if let SeedPlan::Action { seat, .. } = self.seed_plan()
            && seat >= 3
        {
            let s = usize::from(seat);
            return self.queued_category == 3
                && self.displayed_recommit.get(s).copied().unwrap_or(false)
                && self.displayed_move_labels.get(s) == Some(&0);
        }
        self.art_label_unseen()
    }

    /// [`Self::move_label_unseen`]'s party half: the art-name label.
    fn art_label_unseen(&self) -> bool {
        matches!(self.seed_plan(), SeedPlan::Action { seat, .. } if seat < 3)
            && self.queued_category == 3
            && self.caster_latched >= 0x1B
            && self.committed_queue.iter().find(|&&b| b >= 0x1B) == Some(&self.caster_latched)
            && self.cam_accum / 8 < u32::from(self.display_phase_lag)
    }

    /// Whether the capture stands on the pass its art committed: the commit
    /// zeroes the accumulator after that pass's framing ran
    /// (`0x8004BF74`, [`Self::move_label_unseen`]), so the RAM's camera
    /// still walks the clip before's table
    /// (`minigame_muscle_dome_retail_skeleton_hyper_arts`: TR z `1945` where
    /// the art's own arm reads `1817`).
    pub fn camera_behind_commit(&self) -> bool {
        self.art_label_unseen() && self.cam_accum == 0
    }

    /// The live pose the image child stands a [`Self::camera_behind_commit`]
    /// capture on: the frame on screen predates the swing's own close-up. The
    /// art arm `FUN_801D71B8` calls the tween builder with one display frame
    /// (`a3 = 2`), which lands that pass and leaves `ceil(gap / 2)` in the
    /// table, so the depth the pass walked from is `2 * step` behind the RAM's,
    /// on the far side since the arm only tightens the shot
    /// (`minigame_muscle_dome_retail_skeleton_hyper_arts`: `1945`, step `103`:
    /// the frame fits `2150`). The channel keeps reading the RAM's pose.
    pub fn displayed_commit_live(&self) -> legaia_engine_camera::projection::BattleCamPose {
        let (mut live, end) = (self.displayed_cam_live, self.cam_tween.1);
        if self.camera_behind_commit() && self.cam_tween.0.tr[2] == end.tr[2] {
            live.tr[2] += 2.0 * f32::from(self.cam_tween_steps[5]);
        }
        live
    }

    /// Whether this capture's approach waits on a measured string
    /// ([`Self::party_combo_vsyncs`]): a party Attack in its Done band.
    pub fn wants_party_combo(&self) -> bool {
        matches!(self.seed_plan(), SeedPlan::Action { seat, state: 0x50 | 0x51 } if seat < 3)
            && self.queued_category == 3
            && !self.swing_reseed_entry
            && (3..8).contains(&self.target_code)
            && self.party_combo_vsyncs.is_none()
    }

    /// This capture with the replay's measured string
    /// ([`Self::party_combo_vsyncs`]).
    pub fn with_party_combo(&self, vsyncs: u32) -> Self {
        let mut out = self.clone();
        out.party_combo_vsyncs = Some(vsyncs);
        out
    }

    /// The backdrop pair's depth-cue weight a capture-band capture
    /// (`0x6E..=0x71`) holds, for the phase to stand the engine's on.
    ///
    /// `FUN_80050120` ramps the pair's `+0x78` by `step << 6` a pass while
    /// the latch `ctx[+0x243]` is up (`0x800505B0..0x80050714`), and the
    /// cast begin's capture route raises the latch behind its `0x6E` store
    /// (`0x801E44FC`). So the weight is a third clock of the band, from its
    /// entry - beside the close-up accumulator (from the caster's clip
    /// commit) and the yaw counter (from the seed) the drive already holds
    /// the band by. The three disagree on the engine, whose cast begin
    /// leaves for `0x6E` on its first pass where retail's waits several
    /// (`battle_noaru_gilium_lv2_thunder_storm_mid_cast`: retail `0x6C0`, the
    /// engine at the `0x800` indoor ceiling, the cave drawn a fifth darker -
    /// `image` `.414`; on retail's word `.805`).
    ///
    /// The Spirit / Item band is the same clock. Its pre-arm raises the latch
    /// at `0x3C` (`0x801E3B6C`), and `0x3D` then holds until the caster's idle
    /// ends, a wait as long as the idle's phase left it: Garnet's Heal stands
    /// retail's `0x500`, 20 units past the latch and 10 vsyncs into `0x3E` on
    /// its accumulator, where the replay's idle has waited out the `0x800`
    /// ceiling. The accumulator gates the band's run, so the weight is seated
    /// the same way (`battle_deene_garnet_heal_mid_cast`, `.77` to `.93`).
    pub fn capture_band_backdrop_cue(&self) -> Option<u16> {
        matches!(self.seed_plan(), SeedPlan::Action { .. })
            .then_some(())
            .filter(|_| {
                self.monster_magic_band_age()
                    || super::CAPTURE_BAND_STATES.contains(&self.action_state)
                    || super::SPIRIT_STAGE_STATES.contains(&self.action_state)
            })
            .and(self.backdrop_cue)
    }

    /// [`Self::capture_band_backdrop_cue`] on the **displayed** frame, for
    /// the image child: the weight climbed `64` a vsync of the display lag
    /// since that frame was drawn (`step << 6` a pass), so the frame on
    /// screen read that much less. A weight on one of the ceilings
    /// (`0x800` / `0xC00` / `0x1000`) may have sat there through the lag and
    /// stays as read.
    pub fn displayed_backdrop_cue(&self) -> Option<u16> {
        let w = self.capture_band_backdrop_cue()?;
        Some(if matches!(w, 0x800 | 0xC00 | 0x1000) {
            w
        } else {
            w.saturating_sub(64 * self.display_phase_lag)
        })
    }

    /// The ambient base the displayed frame coloured the ground grid from, on a
    /// monster's magic capture: the latch is up, so `FUN_80050120` took `8` a
    /// channel a vsync off the base since that frame was drawn, from a ceiling of
    /// `0x80`. A base on the floor `0x20` may have sat there for those vsyncs, so
    /// it stays as read, the way a weight on a ceiling does
    /// ([`Self::displayed_backdrop_cue`]).
    ///
    /// A monster's Spirit / Item band capture takes the same seat. State
    /// `0x3C` raises the latch (`sb v0,0x243(v1)` at `0x801E3B68`) and `0x3D`
    /// then holds until the caster's idle reaches its natural end, so how far
    /// the base has dimmed by the gate is the caster's idle phase at the
    /// seed, which the replay's own rounds decide: a harness seat, not a
    /// ramp the engine gets wrong. `battle_garmel_vera_v2_heal_mid_cast`
    /// holds base `0x38` (nine vsyncs of dim, the backdrop weight's `576`
    /// agreeing) where the replay, its wait `36` passes longer, sat on the
    /// floor `0x20` and drew the ground at half retail's brightness.
    pub fn displayed_ambient_base(&self) -> Option<u8> {
        if !(self.monster_magic_band_age() || self.monster_spirit_band_age()) {
            return None;
        }
        let base = u16::from(self.ambient_word).checked_sub(0x40)?;
        (base > 0x20).then(|| (base + 8 * self.display_lag).min(0x80) as u8)
    }

    /// A monster's Attack captured in a round every party seat committed
    /// Spirit (`+0x1DE == 4` on each): the party sat that round out, so the
    /// drive's party commits Spirit as well ([`BattleDrive::Action`]'s
    /// `spare`) rather than Attack. Every monster-strike capture in the
    /// library holds that round with each monster on its full HP - the
    /// harvest script's opening Spirit rounds - and a replay that swings
    /// instead kills a monster the capture holds standing
    /// (`battle_tunnelb_warman_strike`'s Warman A, struck down where
    /// retail's idles behind the fight).
    pub fn party_spirit_round(&self) -> bool {
        self.active_actor >= 3
            && self.queued_category == 3
            && !self.party_categories.is_empty()
            && self.party_categories.iter().all(|&c| c == 4)
    }

    /// The maximum HP engine slot `slot` stands on when the capture's seat
    /// word holds what its character record does not: a harvest probe's
    /// raised pool (`LEGAIA_TOUGH=2`, `autorun_battle_harvest.lua`). The seed
    /// builds a seat from the record, so such a state's HUD read the record's
    /// maximum, with every HP clamped under it, beside retail's `9999`. Only
    /// the probe's own value counts: two battle-less states hold other words
    /// in a seat no fight stood up (`rim_elm_queen_bee_battle`'s `5636`).
    pub fn party_seat_pool_raised(&self, slot: usize, seat: &Combatant) -> Option<u16> {
        const RAISED_POOL: u16 = 9999;
        let record = *self.record_hp_max.get(slot)?;
        (slot < usize::from(self.party_count.clamp(1, 3))
            && seat.hp_max == RAISED_POOL
            && record != RAISED_POOL
            && record != 0)
            .then_some(RAISED_POOL)
    }

    /// The maximum MP engine slot `slot` stands on when the capture's seat
    /// word `+0x152` holds what its character record's `+0x108` does not.
    /// Battle init copies the record's (`FUN_80053CB8`, `0x80053E58`), and
    /// from there the seat's is the fight's own: Lapis Wave zeroes it
    /// (`0x801F6D1C`) and Fatal Decision halves or zeroes it, neither touching
    /// the record. A replay seeded past such a cast never runs it, so the
    /// seat takes the capture's word (`battle_deene_lapis_strike`: `0` on
    /// every seat against records of `602` / `509` / `660`).
    pub fn party_seat_mp_max_off_record(&self, slot: usize, seat: &Combatant) -> Option<u16> {
        let record = *self.record_mp_max.get(slot)?;
        (slot < usize::from(self.party_count.clamp(1, 3)) && record != 0 && seat.mp_max != record)
            .then_some(seat.mp_max)
    }

    /// A monster's strike loop `0x1E` captured still on its walk clip `1`
    /// (`ActionSteer::walk_clock`).
    pub fn walk_clock_capture(&self) -> bool {
        matches!(self.seed_plan(), SeedPlan::Action { seat, state: 0x1E } if seat >= 3)
            && self.queued_category == 3
            && self.caster_clip == 1
            && matches!(self.span_gate, SpanGate::Age { .. })
    }

    /// A [`Self::walk_clock_capture`] saved on the arrival pass itself, no
    /// strike-loop pass run yet (`ctx[+0x15] == 0`; `ActionSteer::arrival`).
    pub fn walk_clock_arrival(&self) -> bool {
        self.walk_clock_capture() && self.strike_cursor == 0
    }

    /// Whether the search may aim the striker's physical turns at the
    /// capture's target (`ActionSteer::strike_aim`): a monster strike whose
    /// target is a party seat.
    pub fn strike_aim_steerable(&self) -> bool {
        matches!(self.seed_plan(), SeedPlan::Action { seat, .. } if seat >= 3)
            && self.queued_category == 3
            && self.target_code < 3
            && matches!(self.span_gate, SpanGate::Age { .. })
    }

    /// The pad drive that reaches this capture, when its plan has one.
    pub fn battle_drive(&self) -> Option<BattleDrive> {
        match self.seed_plan() {
            SeedPlan::Menu { flow, seat } => Some(BattleDrive::Menu {
                flow,
                seat,
                spirit: self.spirit_seats_ahead(seat),
                // A window capture's row: the seat's `+0x1DF`, the hovered
                // bag item or spell.
                pick: if flow.window_of_target().is_some() {
                    self.queued_action
                } else if flow == BattleFlowState::TargetSelect
                    && (3..8).contains(&self.target_code)
                {
                    // The Attack cursor's monster: the seat's `+0x1DD`.
                    self.target_code
                } else {
                    0
                },
            }),
            SeedPlan::Action { seat, state } => Some(BattleDrive::Action {
                seat,
                state,
                category: self.queued_category,
                queued: self.queued_action,
                spare: !self.action_victims().is_empty()
                    || !self.party_wipe_victims().is_empty()
                    || self.party_spirit_round(),
                absorbed: if seat < 3 { self.absorbed_seru } else { 0 },
                end: self.engine_span_gate(),
                style: Some(self.cam_style),
                steer: ActionSteer {
                    prior: self.prior_framing,
                    park: seat >= 3 && self.parked_approach(),
                    // A frame-time history that never left one step names the
                    // rate retail's yaw counter drifted at.
                    drift_step: (self.step_run.1 >= 16).then_some(self.step_run.0),
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
                        // A monster's Heal on another monster: the Spirit band
                        // films its patient (`FUN_801D5854` on the stacked target
                        // in `0x3F` / `0x40`) and names it on the plaque.
                        || (seat >= 3 && self.queued_category == 2 && self.target_code < 8)
                        // A monster cast on itself (Cort's Ultra Charge,
                        // `+0x1DD == ctx[+0x13]`): retail's cast-begin skips
                        // the facing store for a self target (`beq v0,t2` at
                        // `0x801E4350`), and the module frames from the
                        // heading the caster keeps.
                        || (seat >= 3 && self.queued_category == 2 && self.target_code == seat))
                        .then_some(self.target_code),
                    // Past a walk-in body's arm 0 the counter is the arm's
                    // own rung, not the seed's clock
                    // ([`super::seed_plan::yaw_counter_reseeded_by`]).
                    yaw: (!(self.action_state == 0x70
                        && self.module_phase >= 1
                        && super::seed_plan::yaw_counter_reseeded_by(self.queued_action)))
                    .then_some(self.walk_yaw_base),
                    // An Attack's counter climbs from the arm's `0x200`
                    // unless a swing re-seed could have moved it.
                    // A Spirit band action's climbs from the seed's, and how
                    // far by the stage wait's end is the caster's idle phase
                    // (`battle_bubu2_spoon_heal_mid_cast`: retail `2064`, the
                    // replay `2136`, past the clock's slack).
                    rung: if self.queued_category == 3 && !self.swing_reseed_entry {
                        Some(ATTACK_YAW_RUNG)
                    } else if self.queued_category == 2
                        && super::SPIRIT_STAGE_STATES.contains(&self.action_state)
                    {
                        Some(SEED_YAW_RUNG)
                    } else {
                        None
                    },
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
                    clip: ((seat < 3
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
                            || (self.caster_clip == 0 && state == 0x20)))
                        // ...and a monster's strike clip in the strike loop
                        // `0x1E`: the state's first pass commits it and zeroes
                        // the accumulator, so an `accum 0` capture reads both
                        // the state and the age one pass ahead of the commit
                        // (`battle_chitei2_kemaro_lv2_strike`: retail Kemaro
                        // on clip `0x06` at cursor `-24`, the engine's still
                        // on its idle). Clips `0` / `1` are the idle and the
                        // walk, which the engine may hold under another id.
                        // A later accumulator reads the same way: the walk's
                        // clock runs that age too (`battle_map01_frog_strike`:
                        // retail on clip `0x06` at `16`, the engine stopped on
                        // its walk at the same age, framing the walk's case 6).
                        || (seat >= 3
                            && state == 0x1E
                            && self.caster_clip > 1)
                        && matches!(self.span_gate, SpanGate::Age { .. }))
                    .then_some(self.caster_clip),
                    aim: (seat < 3
                        && self.queued_category == 3
                        && (3..8).contains(&self.target_code))
                    .then(|| self.target_code - 3),
                    queue: ((seat < 3 && self.queued_category == 3 && self.arts_queue)
                        || (self.strike_stream_steer && self.strike_stream_steerable())
                        // A monster cast's clip bytes (`+0x1E0`, the Spirit
                        // band's `+0x1E7`): the picker stored them under its
                        // own draws, and a replay that reaches the cast by
                        // another stream walks the clip table to another
                        // entry (`battle_balden_orb_v2_hyper_heal_mid_cast`:
                        // retail `09`, the walk's last tag-`0x23` entry `0A`).
                        || (seat >= 3 && self.queued_category == 2))
                        .then_some(self.committed_queue),
                    cursor: ((seat < 3
                        && self.queued_category == 3
                        && self.arts_queue
                        && matches!(state, 0x1E | 0x1F)
                        // Only on the dynamic art slots, the clips the
                        // `clip` gate leaves open: an idle `0` or a swing is
                        // placed by the age and the clip already.
                        && matches!(self.caster_clip, 0x10 | 0x11)
                        && matches!(self.span_gate, SpanGate::Age { .. }))
                        // ...and a monster's strike clip in the strike loop:
                        // a stream the replay's AI rolled longer plays the
                        // capture's clip as a later swing, which the clip
                        // gate alone takes (`battle_noaru_gilium_lv2_strike`:
                        // retail's first swing `0x08` at cursor `1`, the
                        // engine's `0x07` then `0x08`).
                        || (seat >= 3
                            && state == 0x1E
                            && self.queued_category == 3
                            && self.caster_clip > 1
                            && matches!(self.span_gate, SpanGate::Age { .. })))
                    .then_some(self.strike_cursor),
                    // `battle_map01_frog_strike` is on its first swing with
                    // the cursor already on `2`, the pass after its commit
                    // read the next byte; a stream of `07 06 ff` stops the
                    // cursor on `2` under the second swing.
                    // Also carried on a capture still on its walk clip, where no
                    // cursor gate reads it: the cast-sequence oracle's strike deal
                    // stands the fight's stream on it (`strike_deal`).
                    played: (seat >= 3 && state == 0x1E && self.queued_category == 3)
                        .then_some(self.committed_queue),
                    // Either striker's: a party swing's first block roll reads
                    // them as a monster's does.
                    ramps: (state == 0x1E && self.queued_category == 3).then_some((
                        self.approach_terms.0,
                        self.approach_terms.1,
                        self.acting_attack,
                    )),
                    // On a dynamic art slot the cursor cannot tell a starter
                    // from the art the loop staged behind it - both install
                    // into slot `0x11` - so the latched raw id names which
                    // commit the capture's age counts from.
                    latched: (seat < 3
                        && self.queued_category == 3
                        && self.arts_queue
                        && matches!(state, 0x1E | 0x1F)
                        && matches!(self.caster_clip, 0x10 | 0x11)
                        && self.caster_latched >= 0x10
                        && matches!(self.span_gate, SpanGate::Age { .. }))
                    .then_some(self.caster_latched),
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
                    loot: self.results_drop,
                    walk_clock: self.walk_clock_capture(),
                    arrival: self.walk_clock_arrival(),
                    strike_aim: self.strike_aim_steer,
                    harvest_pacing: seat < 3
                        && self.queued_category == 3
                        && self.arts_queue
                        && self.harvest_pacing,
                    approach_idle: self.approach_start.and_then(|a| a.target_idle),
                    tough: self.party.iter().enumerate().any(|(i, c)| {
                        c.as_ref()
                            .is_some_and(|c| self.party_seat_pool_raised(i, c).is_some())
                    }),
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
///
/// `strike` is `(engine seat, out)`: the tick that seat's latest action
/// first entered the strike loop `0x1E` lands in `out`, on the same clock
/// as the return value ([`RetailBattle::party_combo_vsyncs`]).
pub(super) fn run_drive(
    session: &mut NativeSession,
    director: &mut crate::retail_compare::RecordingDirector,
    drive: BattleDrive,
    age_short: &mut Option<u16>,
    strike: Option<(u8, &mut Option<u32>)>,
) -> Result<Option<u32>> {
    let mut strike = strike;
    drive.prime(&mut session.host.world);
    let mut reached = None;
    let mut held = 0;
    // The engine accumulator on the last tick an `Age` phase's state held
    // short of its age.
    let mut aged = None;
    for t in 0..drive.budget() {
        let world = &session.host.world;
        if reached.is_none()
            && let Some((seat, entry)) = strike.as_mut()
            && world.battle_ctx.active_actor == *seat
        {
            match world.battle_ctx.action_state {
                0x0C => **entry = None,
                0x1E if entry.is_none() => **entry = Some(t),
                _ => {}
            }
        }
        if reached.is_none() {
            if drive.in_aged_state(world) {
                aged = Some(active_clip_age(world));
            } else if let Some(a) = aged.take() {
                // The state ended before it was as old as retail's: the
                // re-run samples its last tick. A state that ran past
                // retail's age unreached missed another of the phase's gates
                // (a monster's strike clip, the art cursor) - a different
                // swing, which a re-run on its last tick would only score as
                // retail's.
                if drive.age_gate().is_none_or(|want| a < u32::from(want)) {
                    *age_short = Some(a.min(u32::from(u16::MAX)) as u16);
                }
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
                "[pos] t={t} act={} st=0x{:02X} yawc={:?} {}",
                world.battle_ctx.active_actor,
                world.battle_ctx.action_state,
                world.battle.camera.as_ref().map(|c| (
                    c.action_yaw_base(),
                    c.framing_pose().focus,
                    c.glide_target().focus,
                    c.framing_pose().yaw,
                    c.close_up_accum(),
                    c.glide_target().tr,
                )),
                (0..world.actors.len().min(8))
                    .map(|i| {
                        let a = &world.actors[i];
                        format!(
                            "| {i}: ({}, {}) b{:?} f{} c{:#04X} k{:?} r{} t{} tgt{} ",
                            a.move_state.world_x,
                            a.move_state.world_z,
                            a.battle.seat,
                            a.battle.facing_angle & 0xFFF,
                            world.battle_current_anim(i),
                            a.battle_animation.as_ref().map(|p| p.current_frame()),
                            a.battle.anim_rate.get(),
                            a.battle.render_flag,
                            a.battle.active_target
                        )
                    })
                    .collect::<String>()
            );
        }
        if std::env::var_os("LEGAIA_RC_DRIVE_TRACE").is_some() {
            if let Some(c) = world.battle.camera.as_ref() {
                eprintln!(
                    "[rc-cam] t={t} pose={:?} target={:?} gauges={:?}",
                    c.pose(),
                    c.glide_target(),
                    world
                        .actors
                        .iter()
                        .take(8)
                        .map(|a| a.battle.spirit_gauge)
                        .collect::<Vec<_>>()
                );
            }
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

/// A float pose's ground pair as whole units.
fn live_pair(focus: [f32; 3]) -> [i32; 2] {
    [focus[0] as i32, focus[2] as i32]
}

/// How many strike-loop passes a capture's yaw increment `inc` stands on.
///
/// Case 7 aims `0x700` below the case-6 yaw the approach left
/// (`0x801D6700`) and every pass re-arms `ceil(gap / 0xC)` and walks
/// `inc * step` of it, so the increments fall on one ladder from the first
/// pass's `150`: the pass is the rung nearest `inc`. The approach's own lag
/// behind its endpoint puts a capture a few units off a rung.
pub fn strike_passes_off_table(inc: u16, step: u8) -> u16 {
    let (inc, step) = (i32::from(inc), i32::from(step.max(1)));
    let mut gap = 0x700;
    let mut passes = 1;
    loop {
        let here = (gap + 0xB) / 0xC;
        gap -= here * step;
        let next = (gap + 0xB) / 0xC;
        if gap <= 0 || (here - inc).abs() <= (next - inc).abs() {
            return passes;
        }
        passes += 1;
    }
}

/// What a capture's RAM cannot say about the walk its displayed frame stands on,
/// read off that retail frame by scoring the law's discrete candidates: the
/// earlier actor's style. The earlier actor itself is the RAM's
/// ([`RetailBattle::gauge_actor`]). `yaw` is a measured angle, for a shot that does
/// not turn and whose caster held a heading the seed's arm read and nothing
/// kept. It reads the capture and moves no game state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayedWalkPin {
    pub style: u8,
    pub yaw: Option<u16>,
}

const DISPLAYED_WALK_PINS: &[(&str, DisplayedWalkPin)] = &[
    (
        // One `0x2B` pass in on a step that has just gone to `4`. The seed's
        // arm read the caster's old heading, which took the yaw down from the
        // Spirit's framing rather than up across the wrap.
        "battle_kor_gaza_166_astral_wave_mid_cast",
        DisplayedWalkPin {
            style: 0,
            yaw: Some(440),
        },
    ),
    (
        // The round's first actor, walking off the prompt's far framing; the
        // seat does not enter on that start.
        "battle_rugi_barra_windcutter_mid_cast",
        DisplayedWalkPin {
            style: 0,
            yaw: Some(3260),
        },
    ),
    (
        "battle_stone_zenoir_v2_fire_claw_mid_cast",
        DisplayedWalkPin {
            style: 0,
            yaw: Some(3923),
        },
    ),
    (
        "battle_vozz_viguro_thunder_break_mid_cast",
        DisplayedWalkPin {
            style: 2,
            yaw: None,
        },
    ),
    (
        "battle_retona_viguro_lv2_thunder_break_mid_cast",
        DisplayedWalkPin {
            style: 2,
            yaw: None,
        },
    ),
    (
        "battle_korb2_ghost_knight_windhash_mid_cast",
        DisplayedWalkPin {
            style: 2,
            yaw: None,
        },
    ),
    (
        "battle_bylon_theeder_v2_thunderbolt_mid_cast",
        DisplayedWalkPin {
            style: 0,
            yaw: Some(1215),
        },
    ),
    (
        "battle_jou_slippery_v3_moldy_breath_mid_cast",
        DisplayedWalkPin {
            style: 0,
            yaw: Some(102),
        },
    ),
];

/// A capture's [`DisplayedWalkPin`].
pub fn displayed_walk_pin(label: &str) -> Option<DisplayedWalkPin> {
    DISPLAYED_WALK_PINS
        .iter()
        .find(|(l, _)| *l == label)
        .map(|(_, p)| *p)
}

/// The start of a case-6 walk ([`RetailBattle::retake_stale_walk_pose`]).
struct WalkStart {
    pitch: i32,
    yaw: i32,
    tr_y: i32,
    tr_z: i32,
    focus: [i32; 2],
}

/// The yaw counter a party seat's Spirit leaves behind: recordings read `2294`
/// and `2324` in the last `0x0A` before the next actor's seed. Off by a hundred
/// units, the displayed yaw moves by about eight.
const SPIRIT_END_COUNTER: i32 = 2300;

/// `0x28` stores `ctx[+0x6D8] = 0x14`; `0x29` drains it by the pass's step and
/// leaves on the pass that reads negative.
const CAST_WAIT_TIMER: i32 = 0x14;

/// The steps of a case-6 walk that ended two passes before the capture's, in
/// the order retail ran them: the seed `0x0C`, `0x28`, `0x29`'s passes down to
/// the exit, then the terminator (ring slot `3`). `ring` is
/// [`super::frame_step::recent_pass_steps`]. Recordings walk `14` passes at
/// step `2`, `10` at step `3`.
///
/// `shot_passes` counts the `0x2B` passes in the RAM, and the display stands
/// two passes behind them. On two the display is the terminator. On one it is
/// the pass before, `0x29`'s exit, which armed and walked, so the terminator
/// takes no step and [`case6_walk`] arms the exit like the rest.
pub fn case6_walk_passes(ring: &[u8; 16], shot_passes: usize) -> Vec<i32> {
    // A step-`2` walk runs past the ring's end; its oldest slot stands in.
    let step = |slot: usize| i32::from(ring[slot.min(15)]);
    let terminator = shot_passes + 1;
    let mut passes = vec![if shot_passes == 1 {
        0
    } else {
        step(terminator)
    }];
    let mut drained = 0;
    let mut slot = terminator + 1;
    while drained <= CAST_WAIT_TIMER {
        passes.push(step(slot));
        drained += step(slot);
        slot += 1;
    }
    passes.extend([step(slot), step(slot + 1)]);
    passes.reverse();
    passes
}

/// One component's case-6 walk: every pass but the last re-arms
/// `ceil(gap / 0xC)` (`FUN_801D829C`), and each moves that increment times its
/// step. The last pass is the terminator, which arms nothing.
pub fn case6_walk(from: i32, to: i32, passes: &[i32]) -> i32 {
    let mut v = from;
    let mut inc = 0;
    for (i, s) in passes.iter().enumerate() {
        if i + 1 < passes.len() {
            let gap = to - v;
            inc = gap.signum() * ((gap.abs() + 11) / 12);
        }
        v += inc * s;
    }
    v
}

/// The yaw of a case-6 walk: the seed's arm aims the counter's `0x800` rung
/// less the caster's home heading (the turn to the target comes after it), and
/// every later pass the drifting counter less the heading it now holds.
pub fn case6_yaw_walk(from: i32, facing: i32, passes: &[i32]) -> i32 {
    let wrap = |g: i32| (g + 2048).rem_euclid(4096) - 2048;
    let (mut v, mut inc, mut counter) = (from, 0, 0x800);
    for (i, s) in passes.iter().enumerate() {
        if i + 1 < passes.len() {
            let heading = if i == 0 { 0x800 } else { facing };
            let gap = wrap(counter - heading - v);
            inc = gap.signum() * ((gap.abs() + 11) / 12);
        }
        v = (v + inc * s).rem_euclid(4096);
        counter += (s * 4 / 3).max(1);
    }
    v
}

/// The gap to case 6's endpoint one pass before `gap`, across the terminator
/// pass that walked on its increment: `gap` stands `s1` (that pass's step)
/// past the armed pass, which took `s2` of the same increment
/// `ceil(gap_before / 0xC)` (case 6's duration, `FUN_801D829C`). A recording of
/// Aluru Cannon walks TR z `4772, 4808, 4856` to `4915` on `s2 = 3`, `s1 = 4`.
pub fn stale_pass_gap(gap: i32, s1: i32, s2: i32) -> i32 {
    (0..=gap + 1)
        .find(|&k| {
            let before = gap + (s1 + s2) * k;
            (before + 11) / 12 == k
        })
        .map_or(gap, |k| gap + s1 * k)
}

/// The widest strike turn [`RetailBattle::strike_arm_turn`] reads as one
/// pass's re-face: a heading a pass moves by far less than this
/// (`battle_jou_party_gala_chain_mid`'s `28` is the widest the corpus holds).
const STRIKE_ARM_TURN_MAX: i32 = 0x80;
