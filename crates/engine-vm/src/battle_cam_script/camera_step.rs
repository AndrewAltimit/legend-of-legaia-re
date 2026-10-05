//! The battle camera's per-step state machine (`impl BattleCamera`).
//! Split out of `battle_cam_script.rs`.

use super::*;

impl BattleCamera {
    /// New camera snapped to the entry phase's framing (a battle that opens
    /// on tutorial dialogue starts in the held close-up; any other battle
    /// starts at the far menu framing sized to no formation - see
    /// [`BattleCamera::new_with_formation`] for the live-formation entry).
    pub fn new(phase: BattleCamPhase, frames_now: u64) -> Self {
        Self::new_with_formation(phase, None, 0.0, frames_now)
    }

    /// [`BattleCamera::new`] with the live formation available at entry, so
    /// a battle opening on the far menu framing snaps to the case-9
    /// formation-sized depth + centre instead of the degenerate minimum
    /// (retail's case 9 runs against the live actor table).
    /// `entry_yaw` is the live `_DAT_8007B792` the fight inherits - see
    /// [`BattleCamInputs::entry_yaw`] for why a battle never starts at `0`.
    pub fn new_with_formation(
        phase: BattleCamPhase,
        formation: Option<FormationBox>,
        entry_yaw: f32,
        frames_now: u64,
    ) -> Self {
        let actor = BattleCamActor::default();
        let action = ActionFraming::default();
        let yaw = entry_yaw.rem_euclid(4096.0);
        let pose = match phase {
            BattleCamPhase::Dialogue => dialogue_pose(formation),
            BattleCamPhase::Submenu | BattleCamPhase::TargetEnemy => actor.submenu_pose(),
            BattleCamPhase::TargetAlly => target_ally_pose(actor),
            BattleCamPhase::Action => action_framing(actor, action),
            BattleCamPhase::Recover => recover_framing(actor, None, action, yaw, false),
            BattleCamPhase::ActionEnd => action_end_framing(actor, None, action, yaw),
            BattleCamPhase::Menu => menu_framing(formation, yaw),
        };
        BattleCamera {
            phase,
            pose,
            glides: std::collections::VecDeque::new(),
            last_frames: frames_now,
            frame_accum: 0,
            actor,
            formation,
            target: None,
            action,
            action_yaw: 0,
            last_action_state: 0,
            acting_body: None,
            spell_cam: None,
            last_active_commits: None,
            last_swing_seeds: None,
            option: CAMERA_OPTION_CLOSE,
            shake: ShakeState::default(),
            attack: AttackChannel::default(),
            rand_state: STANDALONE_RAND_SEED,
            module_glide: None,
            escape_shot: false,
            cursor: None,
        }
    }

    /// Install the per-art attack camera's per-actor channels and (once) the
    /// disc track table. Hosts call this every frame through [`drive`].
    pub fn set_attack_channels(
        &mut self,
        channels: Option<AttackCamChannels>,
        tracks: Option<&legaia_asset::battle_attack_camera_table::AttackCameraTracks>,
    ) {
        self.attack.actor = channels;
        if self.attack.tracks.is_none()
            && let Some(t) = tracks
        {
            self.attack.tracks = Some(t.clone());
        }
    }

    /// The per-art framing for this frame, or `None` when no arm fires - in
    /// which case retail leaves the case-6 framing standing and so does the
    /// port. Mutates the ramp latch exactly as the arms do.
    pub(super) fn attack_framing(
        &mut self,
    ) -> Option<crate::battle_attack_camera::AttackCamFraming> {
        use crate::battle_attack_camera::{AttackCamActor, attack_camera_framing};
        let c = self.attack.actor?;
        let tracks = self.attack.tracks.as_ref()?;
        let a = self.actor;
        let world = |v: f32| (v as i32) as u16;
        attack_camera_framing(
            AttackCamActor {
                character: c.character,
                art_id: c.art_id,
                arm_select: c.arm_select,
                anim_frame: c.anim_frame,
                pos: [world(a.world[0]), world(a.world[1]), world(a.world[2])],
                facing: a.facing as u16,
            },
            &mut self.attack.ctx,
            tracks,
        )
    }

    /// Install the case-6 context inputs the [`BattleCamPhase::Action`]
    /// framing reads. Hosts call this every frame; an already-armed glide is
    /// left alone (retail re-arms the whole framing on the state change, not
    /// mid-tween).
    pub fn set_action_framing(&mut self, action: ActionFraming) {
        self.action = action;
    }

    /// Observe the live action-SM state and apply the yaw counter's
    /// per-action seeds on its edges - the `ctx[+0x6DA]` ladder in the module
    /// doc:
    ///
    /// - `0x00` (round begin, `0x801E2B40`): `0`.
    /// - The `0x0C` seed pass: `0x800` for every category (`0x801E2CF8`),
    ///   and the Attack branch stores `0x200` **in the same pass** as it
    ///   sets `ctx[7] = 0x14` (`0x801E2F18..0x801E2F20`). So the edge is
    ///   "an action band was entered from the setup / done bands", not "the
    ///   SM sat in `0x0C`" - a host's SM may run the seed pass and the band
    ///   entry inside one tick (the engine's does: `0x0A -> 0x14` for a party
    ///   attack, `0x00 -> 0x15` for a monster's), and the camera only sees
    ///   the state at the tick boundary.
    /// - A party swing's clip commit re-seeds it to `(rand() % 2) * 0x800 +
    ///   0x280` - `FUN_8004E13C`'s value-2 arm, which this observer does not
    ///   see; [`Self::observe_swing_reseed`] applies it. Captures show the
    ///   re-seed is not tied to the strike loop's entry:
    ///   `player_steal_skeleton_pre` reads the Attack base `0x218` eight
    ///   frames into `0x1E`.
    ///
    /// A monster's attack therefore frames from the `0x200` base and a party
    /// attack from `0x280` / `0xA80`, both drifting at the SM's rate
    /// ([`Self::advance_to`]). Other categories keep the seed's `0x800`; an
    /// attack-band entry from another action band (no seed pass between)
    /// stores nothing, as retail's does not. The coin comes from the same
    /// PsyQ `rand()` stream as the attack camera's column flip; retail draws
    /// both from the process-wide generator, so no particular sequence is
    /// being reproduced.
    /// The staged-animation commit's counter reset: when the active actor
    /// commits a clip, `FUN_8004AD80` zeroes `ctx[+0x26E]` (the ramp),
    /// `ctx[+0x87C]` (the accumulator) and `ctx[+0x26F]` (the latch)
    /// (`0x8004BF50..0x8004BF78`). `ctx[+0x270]` (the death ramp) is not
    /// touched.
    ///
    /// REF: FUN_8004AD80
    pub fn observe_active_commits(&mut self, commits: u32) {
        let prev = self.last_active_commits.replace(commits);
        if prev.is_some_and(|p| p != commits) {
            let c = &mut self.attack.ctx;
            c.ramp = 0;
            c.accum = 0;
            c.latch = 0;
        }
    }

    pub fn observe_action_state(&mut self, state: u8) {
        let prev = self.last_action_state;
        if state == prev {
            return;
        }
        self.last_action_state = state;
        self.release_module_shot(state);
        if state == 0x00 {
            self.action_yaw = 0;
            return;
        }
        // The seed pass `0x0C` opens every category's band; `0x50` (Done)
        // closes them. The Attack band is `0x14..=0x20`.
        let in_action = |s: u8| (ACTION_SEED_STATE..ACTION_DONE_STATE).contains(&s);
        let in_attack = |s: u8| (ATTACK_BAND_FIRST..=ATTACK_BAND_LAST).contains(&s);
        // Came through the seed pass: from outside the action bands, or from
        // the seed states themselves (`0x0C..0x14`).
        let via_seed = !in_action(prev) || prev < ATTACK_BAND_FIRST;
        if in_attack(state) && via_seed {
            self.action_yaw = 0x200;
        } else if in_action(state) && !in_action(prev) {
            self.action_yaw = 0x800;
        }
    }

    /// `FUN_8004E13C`'s value-2 re-seed (`0x8004E288..0x8004E2B4`): a clip
    /// commit whose `+0x87` byte is `2`, after one that was not, with a party
    /// seat acting, sets the yaw counter to `(rand() % 2) * 0x800 + 0x280`.
    /// The world draws the coin at the commit and counts the re-seeds; the
    /// camera applies one whenever the count moves. (The same arm's
    /// `ctx[+0xD] = 0` is the world's own write to the live style byte.)
    /// Install the options screen's Battle Camera word `_DAT_800846C0`
    /// (Close `0` / Normal `1` / Far `2`). Retail reads it in four places,
    /// all of them making the action shots calmer as it rises:
    ///
    /// - the action SM's prologue (`0x801E29D4..0x801E29DC`) skips the
    ///   per-frame `ctx[+0x6DA]` drift - and its idle-orbit store - on Far;
    /// - case 7's pull-in (`0x801D6724..0x801D6730`) needs Close;
    /// - case 8 (`0x801D6958..0x801D69EC`) holds the live yaw on Far and,
    ///   on Normal or Far, skips its dead- and live-target arms for an
    ///   ordinary fight (the `_DAT_8007BD2C` / `ctx[+0x287]` exception that
    ///   keeps them in a scripted one is not carried);
    /// - the per-art attack camera's call (`0x801D7138..0x801D7144`) is
    ///   skipped on Far.
    pub fn set_camera_option(&mut self, option: u8) {
        self.option = option;
    }

    pub fn observe_swing_reseed(&mut self, (count, coin): (u32, u8)) {
        let prev = self.last_swing_seeds.replace(count);
        if prev.is_some_and(|p| p != count) {
            self.action_yaw = i32::from(coin & 1) * 0x800 + 0x280;
        }
    }

    /// The live yaw counter `ctx[+0x6DA]` the in-fight framings subtract the
    /// actor facing from (16-bit, free-running).
    pub fn action_yaw_base(&self) -> i32 {
        self.action_yaw
    }

    /// Put the yaw counter on the half-turn nearer `yaw`: keep it, or flip
    /// its `0x800` bit, whichever lands closer (mod `0x1000`). The strike
    /// loop's seed `(rand() % 2) * 0x800 + 0x280` is a coin on the shared
    /// `rand()` stream; an instrument replaying a capture on another stream
    /// aligns the coin with this, the way it aligns the idle orbit
    /// ([`Self::align_orbit_yaw`]). The drift below the half-turn is left
    /// to the engine.
    pub fn align_action_yaw_half(&mut self, yaw: i32) {
        let dist = |a: i32| {
            let d = (a - yaw).rem_euclid(0x1000);
            d.min(0x1000 - d)
        };
        let flipped = self.action_yaw ^ 0x800;
        if dist(flipped) < dist(self.action_yaw) {
            self.action_yaw = flipped;
        }
    }

    /// Install `_DAT_8007B630`, the screen-shake amplitude.
    ///
    /// The **only** retail writer of that global is the field-VM opcode
    /// `0x4C` outer-nibble `8` sub-`4` (`[4C, 84, amplitude]`, dispatcher arm
    /// `0x801E2134`, jump-table slot `0x801CEF58`), and the only callers of
    /// `FUN_801D9D30` are the field-family overlay's per-frame camera
    /// updaters (`0x801D1344` and siblings). Both ends are therefore field
    /// side: in retail the battle-action overlay occupies the same slot-A
    /// base, so no shake caller is resident during a fight. The engine drives
    /// it from the same camera because the translation pair the routine
    /// jitters, `0x800840B8/BC`, *is* this pose's `tr` - one global, one
    /// owner - and a script that raised the amplitude before a battle would
    /// otherwise leave it stranded.
    ///
    /// REF: FUN_801D9D30 (the kernel is
    /// [`crate::battle_camera::apply_shake`])
    pub fn set_shake_amplitude(&mut self, amplitude: u8) {
        self.shake.amplitude = u32::from(amplitude);
    }

    /// Install the formation the far menu framing sizes itself to (retail's
    /// per-frame `min`/`max` walk over the present actors). Hosts call this
    /// as the battle formation changes; an already-armed glide is left alone.
    pub fn set_formation(&mut self, formation: Option<FormationBox>) {
        self.formation = formation;
    }

    /// The far menu framing for the live formation, at the current yaw (the
    /// idle orbit owns yaw across this transition - retail passes
    /// `_DAT_8007B792` straight through).
    pub(super) fn menu_pose(&self) -> BattleCamPose {
        menu_framing(self.formation, self.pose.yaw)
    }

    /// Point the submenu close-up at the acting battle actor. Retail
    /// rebuilds the framing from the actor record on every submenu open
    /// (`FUN_801D5854` case `0`), so hosts should call this as the active
    /// seat changes.
    ///
    /// While the close-up is up, a **different** actor re-arms it. The
    /// command flow hands the ring straight from one member to the next -
    /// every commit ends on the ring (`0x28`) for the next member owing a
    /// command or on the commit confirm (`0x6E`) - with no far framing in
    /// between, and the menu driver `FUN_801D388C` re-arms case `0` with
    /// `a0 = ctx[+0x13]`, the member now commanding (`0x801D4758..0x801D4760`,
    /// `0x801D53B4..0x801D53BC`). So the close-up glides over to the new
    /// member on the case's own `a3 = 0xC` (6 camera steps). Without the
    /// re-arm the phase never changes (`Submenu` to `Submenu`) and the camera
    /// stays on the first member for the whole command phase.
    pub fn set_actor(&mut self, actor: BattleCamActor) {
        let changed = actor != self.actor;
        self.actor = actor;
        if changed && self.phase == BattleCamPhase::Submenu {
            let mut from = self.pose;
            self.glides.clear();
            self.glides.push_back(Glide::linear(
                &mut from,
                self.actor.submenu_pose(),
                SUBMENU_TR_Z_RAW,
                SUBMENU_ENTER_STEPS,
                true,
            ));
            self.pose = from;
        }
    }

    /// Install what the target cursor rests on. Every cursor move re-arms
    /// the framing case in retail (the menu driver's cursor steps call
    /// `FUN_801D5854` again with the new `+0x1DD`), so a change while a
    /// cursor framing is live glides to the new pose over the case's own
    /// `a3 = 0xC` - 6 camera steps.
    pub fn set_cursor(&mut self, cursor: Option<CursorFraming>) {
        let changed = cursor != self.cursor;
        self.cursor = cursor;
        if changed
            && matches!(
                self.phase,
                BattleCamPhase::TargetEnemy | BattleCamPhase::TargetAlly
            )
            && let Some((target, raw_z)) = self.cursor_pose()
        {
            let mut from = self.pose;
            self.glides.clear();
            self.glides
                .push_back(Glide::linear(&mut from, target, raw_z, CURSOR_STEPS, true));
            self.pose = from;
        }
    }

    /// The cursor framing's target pose and raw depth, off the live pose's
    /// focus (case 1's bearing reads the camera's current focus word).
    fn cursor_pose(&self) -> Option<(BattleCamPose, i32)> {
        match self.cursor? {
            CursorFraming::Enemy { target } => Some((
                target_enemy_pose(self.actor, target, self.pose.focus),
                SWING_TR_Z_RAW,
            )),
            CursorFraming::Ally(ally) => Some((target_ally_pose(ally), SUBMENU_TR_Z_RAW)),
        }
    }

    /// Install the acting actor's target, which cases `7` and `8` frame
    /// against ([`PostActionTarget`]). Hosts call this every frame through
    /// [`drive`].
    pub fn set_post_action_target(&mut self, target: Option<PostActionTarget>) {
        self.target = target;
    }

    /// Case 7's framing for the live actor / target pair, on the live yaw
    /// counter and the live camera yaw (which its one-way unwrap reads).
    ///
    /// The "pull in" tweak (`0x801D6724..0x801D67BC`) is gated on
    /// `_DAT_800846C0 == 0` and then, for a party seat, on the acting
    /// actor's **live** anim id `+0x1D9` being `0x11` - the dynamic art-bank
    /// slot the commit ladder materialises a staged id `>= 0x1A` into for a
    /// party actor (`FUN_8004AD80` `0x8004B6E8..0x8004B76C`; the
    /// `battle_melee_hit_spark` capture reads `+0x1D9 = 0x11` under the
    /// latched `+0x1DB = 0x27`, mid-way into the pulled-in pose). The port's
    /// actor keeps the raw staged id, so the same test is "the latched id is
    /// the SpecialStarter or an art constant". A monster seat pulls in on
    /// `ctx[+0x243] != 0` - the last committed clip's header byte - which the
    /// engine does not carry, so a monster's two-shot never pulls in here.
    /// The acting actor as cases 7 and 8 read it: X / Z from the body pair
    /// `+0x3C` / `+0x40` (`lh v0,0x3c(s2)` at `0x801D661C`, `lhu v0,0x3c(s2)`
    /// at `0x801D6870`), Y the live `+0x36`. Case 6 reads the live pair
    /// `+0x34` / `+0x38` instead, which is [`Self::actor`] as the host
    /// hands it.
    pub(super) fn body_actor(&self) -> BattleCamActor {
        let mut a = self.actor;
        if let Some([x, z]) = self.acting_body {
            a.world[0] = x;
            a.world[2] = z;
        }
        a
    }

    pub(super) fn recover_pose(&self) -> BattleCamPose {
        let pull_in = self.option == CAMERA_OPTION_CLOSE
            && self.action.party_slot
            && self
                .attack
                .actor
                .is_some_and(|c| c.art_id >= crate::battle_attack_camera::FIRST_ART);
        recover_framing(
            self.body_actor(),
            self.target,
            self.live_action_framing(),
            self.pose.yaw,
            pull_in,
        )
    }

    /// Case 8's framing for the live pair, plus the eye-space Z **in world
    /// units** the glide has to converge on - the death re-frame moves TR.z
    /// by `4 * ctx[+0x270]` in that space, so a caller that re-derived the
    /// depth from [`ActionFraming::depth_raw`] would walk to the wrong place.
    ///
    /// Retail's dead-target arm (`0x801D6A20`, taken when the framed target's
    /// live-HP halfword is zero) is reached here through
    /// [`PostActionTarget::live`]. The port conflates retail's two separate
    /// tests - the node test `actor_table[target][+4] != 0` that picks the
    /// focus, and `target[+0x14C] == 0` that opens this arm - into that one
    /// flag, which is the same conflation the focus fork already documents.
    pub(super) fn action_end_pose(&mut self) -> (BattleCamPose, i32) {
        let f = self.live_action_framing();
        let actor = self.body_actor();
        let mut pose = action_end_framing(actor, self.target, f, self.pose.yaw);
        if self.option == CAMERA_OPTION_FAR {
            // `sh a0,0x12(sp)` with the live `_DAT_8007B792` (`0x801D696C`).
            pose.yaw = self.pose.yaw;
        }
        if self.option != CAMERA_OPTION_CLOSE {
            return (pose, f.depth_raw);
        }
        let Some(t) = self.target.filter(|t| !t.live) else {
            // A standing target takes the live-target arm. Only a real
            // target slot reaches it (`actor[+0x1DD] < 8`, `0x801D6BFC`).
            if let Some(t) = self.target {
                let raw_z = apply_live_target_reframe(&mut pose, t, f.depth_raw, self.pose);
                return (pose, raw_z);
            }
            return (pose, f.depth_raw);
        };
        // Both dead-target arms start from the target-facing yaw.
        let yaw = dead_target_yaw(t.facing, self.attack.ctx.phase_cursor, f);
        if t.node_gone || t.lone_defeat {
            // The stand-off arm adds the live ladder rather than zeroing it.
            let yaw = yaw + self.action_yaw;
            let raw_z = apply_node_gone_reframe(&mut pose, actor, yaw, f.body_radius);
            return (pose, raw_z);
        }
        // Stored straight over the unwrapped base (`sh v1,0x12(sp)`); the
        // tween builder's shortest-arc adjust takes it from there.
        pose.yaw = yaw.rem_euclid(4096) as f32;
        // The death re-frame owns the yaw ladder too: `sh zero,0x4(t0)` with
        // `t0 = ctx + 0x6D6` zeroes `ctx[+0x6DA]` before the fork.
        self.action_yaw = 0;
        let ramp = self.attack.ctx.death_ramp;
        if apply_death_reframe(&mut pose, f.depth_raw, ramp, t.world[1]) {
            self.attack.ctx.death_ramp = 0;
            return (pose, f.depth_raw);
        }
        (pose, f.depth_raw - 4 * i32::from(ramp))
    }

    /// Which framing owns the camera this frame. Hosts export it so what the
    /// camera is *doing* is observable - a pose alone cannot distinguish "far
    /// framing, mid-glide" from "action close-up, settled".
    pub fn phase(&self) -> BattleCamPhase {
        self.phase
    }

    /// Whether a glide is still carrying the camera toward its framing - the
    /// capture harness's "settled" test, since a retail capture of a menu is
    /// taken on a camera that has long arrived.
    pub fn is_gliding(&self) -> bool {
        !self.glides.is_empty()
    }

    /// Phase-align the idle orbit's clock: set the free-running azimuth to
    /// `yaw` (12-bit units). Applies only where the orbit owns yaw - the
    /// [`BattleCamPhase::Menu`] far framing with no yaw glide in flight - and
    /// returns whether it did.
    ///
    /// The orbit is a clock (`-4` per camera step from whatever azimuth the
    /// field left), so a comparison against one retail instant reads the
    /// capture's timing unless the two clocks are aligned; this is the
    /// capture harness's handle for that, the camera twin of the field HUD
    /// countdown hold. Nothing in play calls it.
    pub fn align_orbit_yaw(&mut self, yaw: f32) -> bool {
        let yaw_gliding = self.glides.front().is_some_and(|g| g.yaw_glides);
        if self.phase != BattleCamPhase::Menu || yaw_gliding {
            return false;
        }
        self.pose.yaw = yaw.rem_euclid(4096.0);
        true
    }

    /// Retail's `ctx[+0x87C]` - the close-up accumulator the framing
    /// prologues advance by `8` a display frame and the active actor's clip
    /// commit zeroes ([`Self::observe_active_commits`]).
    pub fn close_up_accum(&self) -> u32 {
        self.attack.ctx.accum
    }

    /// Current camera pose (12-bit angle units + eye-space TR), **with** the
    /// live screen-shake offset folded into the translation pair - which is
    /// where retail's `FUN_801D9D30` puts it.
    pub fn pose(&self) -> BattleCamPose {
        let mut p = self.pose;
        p.tr[0] += self.shake.accum[0] as f32;
        p.tr[1] += self.shake.accum[1] as f32;
        p
    }

    /// The framing pose without the shake - the value the phase script's
    /// glides converge on, and the one a test asserts a framing against.
    pub fn framing_pose(&self) -> BattleCamPose {
        self.pose
    }

    /// The live shake offset (retail `DAT_801C6EA4 + 0x18/+0x1C`).
    pub fn shake_offset(&self) -> [i32; 2] {
        self.shake.offset
    }

    /// The action framing for the acting actor, as case 6 would build it
    /// now - on the live `ctx[+0x6DA]` counter.
    pub(super) fn action_pose(&self) -> BattleCamPose {
        action_framing(self.actor, self.live_action_framing())
    }

    /// [`Self::action`] with the live yaw counter and close-up counters
    /// substituted in, and the swing-clip commit's `ctx[+0xD] = 0` applied
    /// while it is latched.
    pub(super) fn live_action_framing(&self) -> ActionFraming {
        ActionFraming {
            yaw_base: self.action_yaw,
            accum: self.attack.ctx.accum,
            ramp: self.attack.ctx.ramp,
            style: self.action.style,
            ..self.action
        }
    }

    /// Take the camera back from a battle-stage module that owned the camera
    /// globals (the Cort arrival, PROT 0968), at the pose it left them in.
    ///
    /// The module hands the fight back to flow state `0x0B`, and the round it
    /// opens re-arms case `9` - the far framing - over case 9's own `a3 = 0xE`
    /// (7 camera steps), tweening from wherever the module left the
    /// registers. Yaw passes straight through to the idle orbit, as in every
    /// case-9 re-arm.
    ///
    /// REF: FUN_801D5854 (case 9), FUN_801D829C
    pub fn hand_back_from(&mut self, pose: BattleCamPose) {
        let mut from = pose;
        self.glides.clear();
        self.glides.push_back(Glide::linear(
            &mut from,
            self.menu_pose(),
            menu_raw_z(self.formation),
            SWING_RETURN_STEPS,
            false,
        ));
        self.pose = from;
        self.phase = BattleCamPhase::Menu;
    }

    /// Observe the live battle phase; a change arms the measured glide.
    pub fn set_phase(&mut self, phase: BattleCamPhase) {
        if phase == self.phase {
            return;
        }
        // `Glide::linear` may unwrap the CURRENT yaw a full turn (retail's
        // wrap-adjust moves whichever side is behind), so the live pose has to
        // see the adjustment.
        let mut from = self.pose;
        self.glides.clear();
        match phase {
            BattleCamPhase::Action => {
                // A new action: retail's `FUN_8004E13C` re-rolls the per-art
                // track column (`ctx[+0x26D] = rand() % 2`) and the ramps
                // restart from zero, so the swing frames from one of the
                // table's two columns.
                let coin = crate::battle_formulas::world_rand(&mut self.rand_state) & 1 != 0;
                self.attack.ctx.begin_action(coin);
                // Retail re-arms case 6 on the state change and tweens over
                // its own `a3 = 0xC` (6 camera steps), yaw included.
                self.glides.push_back(Glide::linear(
                    &mut from,
                    self.action_pose(),
                    self.live_action_framing().raw_z(),
                    ACTION_STEPS,
                    true,
                ));
            }
            BattleCamPhase::Recover => {
                // Case 7 hands `FUN_801D829C` `a3 = 0xC` (`0x801D67C8`), the
                // same 6 camera steps case 6 uses.
                let target = self.recover_pose();
                let raw_z = self.live_action_framing().depth_raw;
                self.glides.push_back(Glide::linear(
                    &mut from,
                    target,
                    raw_z,
                    POST_ACTION_STEPS,
                    true,
                ));
            }
            BattleCamPhase::ActionEnd => {
                // Case 8's own `a3 = 0xC` (`0x801D6EEC`).
                let (target, raw_z) = self.action_end_pose();
                self.glides.push_back(Glide::linear(
                    &mut from,
                    target,
                    raw_z,
                    POST_ACTION_STEPS,
                    true,
                ));
            }
            BattleCamPhase::TargetEnemy | BattleCamPhase::TargetAlly => {
                // Cases 1 and 3 both hand `FUN_801D829C` `a3 = 0xC`
                // (`0x801D5AF8`, `0x801D5C48`): 6 camera steps, yaw included.
                if let Some((target, raw_z)) = self.cursor_pose() {
                    self.glides.push_back(Glide::linear(
                        &mut from,
                        target,
                        raw_z,
                        CURSOR_STEPS,
                        true,
                    ));
                }
            }
            BattleCamPhase::Menu
                if matches!(
                    self.phase,
                    BattleCamPhase::Action
                        | BattleCamPhase::Recover
                        | BattleCamPhase::ActionEnd
                        | BattleCamPhase::TargetEnemy
                        | BattleCamPhase::TargetAlly
                ) =>
            {
                // End of action: case 9 re-arms the far framing over its own
                // `a3 = 0xE` (7 steps) and passes yaw straight through, so
                // the idle orbit owns it again immediately.
                self.glides.push_back(Glide::linear(
                    &mut from,
                    self.menu_pose(),
                    menu_raw_z(self.formation),
                    SWING_RETURN_STEPS,
                    false,
                ));
            }
            BattleCamPhase::Menu => {
                if self.phase == BattleCamPhase::Dialogue {
                    // Dialogue dismiss: rate-clamped pitch/TR glide while
                    // the idle orbit resumes immediately (yaw not glided).
                    self.glides.push_back(Glide {
                        target: self.menu_pose(),
                        rate: [
                            DIALOGUE_EXIT_PITCH_RATE,
                            0.0,
                            f32::INFINITY,
                            f32::INFINITY,
                            DIALOGUE_EXIT_Z_RATE,
                            f32::INFINITY,
                            f32::INFINITY,
                            f32::INFINITY,
                        ],
                        yaw_glides: false,
                        steps_left: None,
                    });
                } else {
                    // Submenu exit: swing up over the shoulder, then ease
                    // back down to the menu framing (orbit resumes for the
                    // return segment - retail re-enters at yaw 0). The swing
                    // stays on the acting actor (retail case 1); only the
                    // return pulls the focus out to the formation centre.
                    let swing = Glide::linear(
                        &mut from,
                        BattleCamPose {
                            focus: self.actor.world,
                            ..SWING_POSE
                        },
                        SWING_TR_Z_RAW,
                        SUBMENU_SWING_STEPS,
                        true,
                    );
                    let mut swing_end = swing.target;
                    let back = Glide::linear(
                        &mut swing_end,
                        self.menu_pose(),
                        menu_raw_z(self.formation),
                        SWING_RETURN_STEPS,
                        false,
                    );
                    self.glides.push_back(swing);
                    self.glides.push_back(back);
                }
            }
            BattleCamPhase::Submenu => {
                self.glides.push_back(Glide::linear(
                    &mut from,
                    self.actor.submenu_pose(),
                    SUBMENU_TR_Z_RAW,
                    SUBMENU_ENTER_STEPS,
                    true,
                ));
            }
            BattleCamPhase::Dialogue => {
                // Retail never re-enters the dialogue close-up mid-battle;
                // snap defensively.
                self.pose = dialogue_pose(self.formation);
                from = self.pose;
            }
        }
        self.pose = from;
        self.phase = phase;
    }

    /// Advance to the world's retail-frame counter, stepping the camera once
    /// per 2 display frames (the measured cadence: every trace entry is an
    /// even frame apart).
    pub fn advance_to(&mut self, frames_now: u64) {
        let elapsed = frames_now.saturating_sub(self.last_frames);
        self.last_frames = frames_now;
        // The action SM advances `ctx[+0x6DA]` once per display frame,
        // outside its state switch, so the counter runs whether or not an
        // action is executing.
        if self.option != CAMERA_OPTION_FAR {
            self.action_yaw = self.action_yaw.wrapping_add(elapsed as i32) & 0xFFFF;
        }
        // `FUN_801D5854`'s prologue ramps `ctx[+0x26E]` / `ctx[+0x87C]` by
        // `8 * frame_step` on EVERY call, so they advance per display frame
        // for as long as any framing case is being re-armed - not per camera
        // step.
        self.attack.ctx.advance(elapsed as u32);
        self.frame_accum += elapsed;
        while self.frame_accum >= 2 {
            self.frame_accum -= 2;
            self.step_once();
        }
    }

    /// One step of the walker toward the per-art pose.
    ///
    /// Retail rebuilds its whole nine-component step table every display
    /// frame (`FUN_801D71B8` calls `FUN_801D829C` on each pass) and the walker
    /// then applies one step of it, so the effective law is "move toward the
    /// live target at `ceil(|delta| / duration)` per frame" - a chase with the
    /// arm's own time constant, not a fixed glide toward a frozen target. That
    /// is what this does, on the same builder, with retail's `a3` halved into
    /// the port's 2-frame camera step.
    pub(super) fn step_toward_attack_pose(
        &mut self,
        f: crate::battle_attack_camera::AttackCamFraming,
    ) {
        let target = BattleCamPose {
            pitch: f32::from(f.pose.rot[0]),
            yaw: f32::from(f.pose.rot[1]),
            tr: [
                f32::from(f.pose.dist[0]),
                f32::from(f.pose.dist[1]),
                f32::from(f.pose.dist[2]),
            ],
            // The arms build the look-at as the NEGATED actor position; the
            // engine holds the focus un-negated (see `BattleCamPose::focus`).
            focus: [
                -f32::from(f.pose.look_at[0]),
                -f32::from(f.pose.look_at[1]),
                -f32::from(f.pose.look_at[2]),
            ],
        };
        let mut from = self.pose;
        let steps = (u32::from(f.duration_frames) / 2).max(1);
        let g = Glide::linear(&mut from, target, i32::from(f.pose.dist[2]), steps, true);
        self.pose = from;
        self.step_components(&g);
    }

    /// Re-arm case 6's tween on the **live** acting actor.
    ///
    /// The action SM reaches `FUN_801D5854(actor, 6)` from every one of its
    /// action states on every pass - `0x0C`, `0x14`, `0x1F`, `0x20`, `0x32`,
    /// `0x37`, `0x3C`..`0x40`, `0x46`, `0x47` all call it before they do
    /// anything else - so case 6 rebuilds its three tween-target vectors out
    /// of the live actor record each display frame and hands them to
    /// `FUN_801D829C`, which re-emits the step table. The framing therefore
    /// *chases* the actor rather than gliding to a target frozen at the state
    /// change, which matters because the attack states move the actor:
    /// `0x14` stages the approach walk and `0x19` runs it, so a party member
    /// crosses most of the gap to its target before the swing. A focus pinned
    /// to the seat it left frames an empty patch of ground - at the traced
    /// close-up depth (`prescale(0x500)` = 2048 against 4x-scaled stage
    /// coordinates) the whole formation ends up outside the frustum, several
    /// combatants behind the eye.
    ///
    /// The remaining step count is carried over from the armed segment, so a
    /// framing whose actor never moves steps exactly as the frozen glide did
    /// and still arrives on the target at step [`ACTION_STEPS`].
    ///
    /// REF: FUN_801D5854 (case 6), FUN_801D829C (the step-table builder)
    pub(super) fn retarget_action_glide(&mut self) {
        let live = self.action_pose();
        let raw_z = self.live_action_framing().raw_z();
        let steps = self
            .glides
            .front()
            .and_then(|g| g.steps_left)
            .unwrap_or(ACTION_STEPS);
        let mut from = self.pose;
        let g = Glide::linear(&mut from, live, raw_z, steps, true);
        self.pose = from;
        self.glides.clear();
        self.glides.push_back(g);
    }

    /// Re-derive the **far** framing against the live formation.
    ///
    /// Case 9 is not armed once and left: `FUN_801D0748`'s own battle tick
    /// calls `FUN_801D5854(0, 9)` at `0x801D0E98`, and the menu driver
    /// `FUN_801D388C` re-arms it at `0x801D4908` / `0x801D5688`, so retail
    /// rebuilds `max(span * 3, 0x800)` and the bbox centre out of the live
    /// actor table every pass - exactly like case 6.
    ///
    /// This matters because the formation *moves*: an attacker walks most of
    /// the way to its target during the approach, collapsing the span to the
    /// `0x800` floor. A depth frozen at the moment the far framing was armed
    /// then survives the actor walking back to its seat, leaving the eye at
    /// the minimum depth against a full-width formation - one combatant
    /// filling the frame and the other **behind the eye**. That is what the
    /// port shipped: entering the resting framing mid-approach pinned the
    /// depth at `prescale(0x800)` for the rest of the fight.
    ///
    /// The dialogue-dismiss segment is skipped: it is the one rate-clamped
    /// glide (`steps_left == None`), whose per-component rates are the traced
    /// dismiss law rather than an arrive-together tween.
    ///
    /// REF: FUN_801D5854 (case 9)
    pub(super) fn retarget_menu_glide(&mut self) {
        // Skipped for the two segments that are not "walk to the far framing":
        // the rate-clamped dialogue dismiss (`steps_left == None`) and the
        // submenu-exit swing, which is a scripted two-segment chain through
        // case 1's over-the-shoulder pose before it reaches this framing.
        if self.glides.len() > 1 {
            return;
        }
        if self
            .glides
            .front()
            .is_some_and(|g| g.steps_left.is_none() || g.yaw_glides)
        {
            return;
        }
        let live = self.menu_pose();
        // Settled on the live formation with nothing armed: the idle orbit
        // owns the frame, so leave it alone rather than re-arming every step.
        if self.glides.is_empty() && self.pose.tr == live.tr && self.pose.focus == live.focus {
            return;
        }
        let steps = self
            .glides
            .front()
            .and_then(|g| g.steps_left)
            .unwrap_or(SWING_RETURN_STEPS);
        let mut from = self.pose;
        // `yaw = false`: case 9 passes `_DAT_8007B792` straight through, so
        // the idle orbit keeps owning the azimuth across the re-derive.
        let g = Glide::linear(&mut from, live, menu_raw_z(self.formation), steps, false);
        self.pose = from;
        self.glides.clear();
        self.glides.push_back(g);
    }

    /// [`Self::retarget_action_glide`]'s sibling for cases `7` and `8`: rebuild
    /// the step table against the live pose, carrying the armed segment's
    /// remaining step count so a settled framing stays settled.
    ///
    /// `raw_z` is the framing's own depth in world units: case 8's death and
    /// stand-off arms move it off `ctx[+0x6D0]`, and the chase has to
    /// converge on the depth the pose took, not on the unmoved one.
    pub(super) fn retarget_post_action_glide(&mut self, live: BattleCamPose, raw_z: i32) {
        let steps = self
            .glides
            .front()
            .and_then(|g| g.steps_left)
            .unwrap_or(POST_ACTION_STEPS);
        let mut from = self.pose;
        let g = Glide::linear(&mut from, live, raw_z, steps, true);
        self.pose = from;
        self.glides.clear();
        self.glides.push_back(g);
    }

    /// One rate-limited step of every driven component (all but yaw, which
    /// only moves when the segment owns it - otherwise the idle orbit does).
    pub(super) fn step_components(&mut self, g: &Glide) {
        self.pose.pitch = step_toward(self.pose.pitch, g.target.pitch, g.rate[0]);
        for k in 0..3 {
            self.pose.tr[k] = step_toward(self.pose.tr[k], g.target.tr[k], g.rate[2 + k]);
            self.pose.focus[k] = step_toward(self.pose.focus[k], g.target.focus[k], g.rate[5 + k]);
        }
        if g.yaw_glides {
            self.pose.yaw = step_toward(self.pose.yaw, g.target.yaw, g.rate[1]);
        }
    }

    pub(super) fn step_once(&mut self) {
        // Re-roll the jitter first, on retail's own kernel. `amplitude == 0`
        // is the resting state and backs the previous offset straight out, so
        // an un-shaken camera returns to its framing pose on the next step
        // rather than holding the last kick.
        crate::battle_camera::apply_shake(
            &mut self.shake.accum,
            &mut self.shake.offset,
            self.shake.amplitude,
            &mut self.rand_state,
        );
        // A granted flee's shot owns the camera to the battle's end: the run
        // band and the escape teardown frame nothing of their own.
        if self.escape_shot {
            self.glides.clear();
            self.step_module_shot();
            return;
        }
        // Yaw: the idle orbit owns it in the Menu phase unless the active
        // glide segment glides it (submenu enter / the exit swing).
        let yaw_gliding = self.glides.front().is_some_and(|g| g.yaw_glides);
        if !yaw_gliding && self.phase == BattleCamPhase::Menu {
            self.pose.yaw = (self.pose.yaw - ORBIT_STEP).rem_euclid(4096.0);
        }
        // The per-art attack camera runs AFTER `FUN_801D5854` has armed its
        // own tween (`0x801D7180`), builds a pose of its own and calls the
        // same tween builder again - so whenever an arm fires it is the
        // per-art target the walker steps toward this frame, not case 6's.
        // An art with no arm returns `None` and case 6's glide stands.
        if self.phase == BattleCamPhase::Action {
            // The summon band's `0x33` / `0x34` call `FUN_801DC0A0(caster,
            // 0x12)` every pass, which arms its own close-up; neither state
            // calls `FUN_801D5854`, so case 6 does not run.
            if SUMMON_CAST_STATES.contains(&self.last_action_state) {
                let c = self.attack.ctx;
                let (target, raw_z) =
                    summon_cast_framing(self.actor, self.acting_body, c.accum, c.ramp);
                let mut from = self.pose;
                let steps = (SUMMON_CAST_TWEEN_FRAMES / 2).max(1);
                let g = Glide::linear(&mut from, target, raw_z, steps, true);
                self.pose = from;
                self.glides.clear();
                self.step_components(&g);
                return;
            }
            // `0x35` / `0x36` frame nothing of their own: the camera is the
            // summon module's ([`Self::step_module_shot`]). A module whose
            // camera arms are not ported arms no shot, and keeps case 6.
            if SUMMON_MODULE_STATES.contains(&self.last_action_state) && self.module_shot_armed() {
                self.glides.clear();
                self.step_module_shot();
                return;
            }
            // The magic band's `0x2A..=0x2E` call no `FUN_801D5854` case: the
            // cast-effect driver's shot is the camera, re-armed on every pass
            // that calls it, and a pass that does not leaves the last tween
            // to land.
            if SPELL_CAM_STATES.contains(&self.last_action_state) {
                if let Some(mut si) = self.spell_cam.take() {
                    si.accum = self.attack.ctx.accum;
                    si.live_yaw = self.pose.yaw;
                    si.frame_step = SPELL_CAM_FRAME_STEP;
                    if let Some(shot) = spell_cam_case(&si).shot {
                        let mut from = self.pose;
                        let steps = (shot.frames / 2).max(1);
                        let g = Glide::linear(&mut from, shot.pose, shot.raw_z, steps, true);
                        self.pose = from;
                        self.glides.clear();
                        self.glides.push_back(g);
                    }
                }
                self.step_front_glide();
                return;
            }
            // `0x70` re-arms no framing: hold, or walk the module's shot.
            if self.last_action_state == CAPTURE_MODULE_STATE {
                self.glides.clear();
                self.step_module_shot();
                return;
            }
        }
        // `FUN_801D71B8` hangs off `FUN_801D5854`'s **shared** tail, so it
        // runs after cases 7 and 8 exactly as after case 6: the strike loop
        // `0x1E`, which arms case 7, is where most art swings are filmed.
        // Its own first test is the target's live HP (`0x801D7200`), so a
        // case-8 death re-frame is never overridden.
        if self.option != CAMERA_OPTION_FAR
            && matches!(
                self.phase,
                BattleCamPhase::Action | BattleCamPhase::Recover | BattleCamPhase::ActionEnd
            )
            && let Some(f) = self.attack_framing()
        {
            self.glides.clear();
            self.step_toward_attack_pose(f);
            return;
        }
        if self.phase == BattleCamPhase::Action {
            self.retarget_action_glide();
        }
        // Cases 7 and 8 are re-armed by their own action-SM states on every
        // pass exactly like case 6, and both frame on positions that move
        // (case 7 on the *midpoint*, which walks as the pair separates), so
        // they chase the live pair rather than gliding to a target frozen at
        // the state change.
        match self.phase {
            BattleCamPhase::Recover => {
                let target = self.recover_pose();
                let raw_z = self.live_action_framing().depth_raw;
                self.retarget_post_action_glide(target, raw_z);
            }
            BattleCamPhase::ActionEnd => {
                let (target, raw_z) = self.action_end_pose();
                self.retarget_post_action_glide(target, raw_z);
            }
            BattleCamPhase::Menu => self.retarget_menu_glide(),
            _ => {}
        }
        self.step_front_glide();
    }

    /// Step the front glide segment once, popping it when it lands.
    pub(super) fn step_front_glide(&mut self) {
        let Some(g) = self.glides.front().copied() else {
            return;
        };
        let done = match g.steps_left {
            // Arrive-together glide: the final step lands ON the target
            // (no float residue from the per-step rate division).
            Some(1) => {
                self.pose.pitch = g.target.pitch;
                self.pose.tr = g.target.tr;
                self.pose.focus = g.target.focus;
                if g.yaw_glides {
                    self.pose.yaw = g.target.yaw;
                }
                true
            }
            Some(n) => {
                if let Some(front) = self.glides.front_mut() {
                    front.steps_left = Some(n - 1);
                }
                self.step_components(&g);
                false
            }
            // Rate-clamped glide: each component clamps independently.
            None => {
                self.step_components(&g);
                self.pose.pitch == g.target.pitch
                    && self.pose.tr == g.target.tr
                    && self.pose.focus == g.target.focus
                    && (!g.yaw_glides || self.pose.yaw == g.target.yaw)
            }
        };
        if done {
            self.glides.pop_front();
            if g.yaw_glides {
                // Re-enter the wrapped orbit domain (the exit swing lands
                // on 4096 = 0, where the idle orbit resumes).
                self.pose.yaw = self.pose.yaw.rem_euclid(4096.0);
            }
        }
    }
}
