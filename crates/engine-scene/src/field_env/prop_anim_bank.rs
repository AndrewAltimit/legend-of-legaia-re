//! `PropAnimBank`: every animated prop of one scene, ticked together.

use super::*;

/// One placed prop's animation + interaction runtime: its live [`PropAnim`],
/// the program its bind record decodes to, the record itself (the prop's
/// field-VM script), and the actor state the retail collision / touch probes
/// read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropAnimState {
    /// Live clip state.
    pub anim: PropAnim,
    /// The bind record's decoded animation program + class bits.
    pub program: PropProgram,
    /// The actor's live position (the placement's world position) - what the
    /// moving-arm contact box anchors at.
    pub world: (i32, i32),
    /// Static-arm contact-box centre: the placement's world position plus the
    /// record's collision-footprint offset (`FUN_801CFC40`'s tile-record
    /// derivation; [`legaia_scene_formats::field_objects::Placement::collider_x`]).
    pub collider: (i32, i32),
    /// Flat MAN partition-0 record index of the bind (the prop's script).
    pub record: usize,
    /// The record's bytes (`[u8 n][n*2 name][u8 anim]` header + script), the
    /// buffer a touch interaction runs through the field VM.
    pub record_body: std::sync::Arc<Vec<u8>>,
    /// The parked script cursor (`actor+0x9E`): where the next touch resumes.
    /// Initially just past the spawn prologue's terminating `0x21` (or at the
    /// post-header `pc0` when the record has no `0x24`/`0x25` prologue
    /// marker); updated to wherever an interaction run ends.
    pub parked_pc: usize,
    /// The actor flag word `+0x10`: seeded from the spawn prologue's `0x31`
    /// ops, updated by an interaction run's own `0x31`/`0x32`. Bits `0`/`1`
    /// make the prop collision-exempt (`FUN_801CF754` / `FUN_801CF9F4` skip
    /// `flags & 3` actors) - how an opened door stops blocking.
    pub cflags: u32,
}

impl PropAnimState {
    /// `FUN_801CF754` / `FUN_801CF9F4`'s `flags & 3` filter: the prop no
    /// longer enters the collision candidate list nor the touch probes. Set
    /// by the door's touch pass (`31 00` as the swing starts).
    pub fn collision_exempt(&self) -> bool {
        self.cflags & 3 != 0
    }

    /// `FUN_801CFC40`'s `flags & 0x40020000` class - contact result bit `1`:
    /// blocks like any actor, but the touch is only posted by the
    /// button-gated facing probe (the cupboard interact gate).
    pub fn interact_gated(&self) -> bool {
        self.cflags & 0x4002_0000 != 0
    }

    /// `FUN_801CFC40`'s `flags & 0x1020000` box-source class: live-position
    /// anchor with the moving-actor extents (±40) instead of the static
    /// footprint box (±80).
    pub fn moving_box(&self) -> bool {
        self.cflags & 0x0102_0000 != 0
    }
}

/// Per-scene bank of live actor clip cursors: one entry per placed prop, keyed
/// by the placement's footprint-anchor tile ([`EnvDraw::anchor`]) so the four
/// Rim Elm cupboards each keep their own cursor, plus the **cross-context**
/// cursors ([`Self::actor_clips`]) a running script pokes by target id.
///
/// Everything in here is the same retail surface - the `+0x5C` / `+0x62` /
/// `+0x68` / `+0x6A` words of an actor record that the per-frame anim tick
/// `FUN_800204F8` advances - so the bank is what "the clip tick" means in the
/// port. Scripts only ever *read* the latch; the tick is the only writer.
#[derive(Debug, Clone, Default)]
pub struct PropAnimBank {
    /// Live props, keyed by anchor tile.
    pub props: std::collections::BTreeMap<(u8, u8), PropAnimState>,
    /// Clip cursors for the actors a script reaches **cross-context** - the
    /// `0x80`-prefixed opcode forms whose target byte `FUN_8003C83C` resolves
    /// to some other actor's record. Keyed by that target byte (which is the
    /// resolver's own key: `ctx[+0x50]`, with [`PLAYER_ANCHOR_TARGET`] for the
    /// player). An entry exists once a poke has bound a clip to it; the player
    /// anchor is created on first use because retail's player actor is always
    /// running its locomotion clip.
    pub actor_clips: std::collections::BTreeMap<u8, PropAnim>,
    /// Scene ANM clip metadata by anim id (`(frame_count, scaled_step,
    /// step_div)` for record `id - 1`), captured at scene build so a clip poke
    /// mid-conversation can size its cursor without re-opening the bundle.
    /// Index `0` is unused (`anim_id == 0` means "no clip").
    clips: Vec<Option<(u16, bool, u8)>>,
    /// Set by [`Self::tick_anims`], cleared by
    /// [`Self::tick_actor_clips_for_frame`] - the once-per-frame guard that
    /// keeps a cursor advancing exactly one step per field frame no matter
    /// which of the world's two drivers reached it first.
    ticked_this_frame: bool,
}

impl PropAnimBank {
    /// Build the bank for a scene: one entry per bound placement, with its
    /// bind record's program run through the spawn prologue so the prop
    /// starts in its authored rest state.
    ///
    /// An unposed bind (`anim_id == 0`) of the interact-gated class gets an
    /// entry too. `FUN_8003A55C` spawns an actor for every bound cell and only
    /// copies the header's anim byte into `+0x5C` (`0x8003A8DC`) - it never
    /// branches on it - so such an object is still examined for its record:
    /// `rikuroa` P0[2], the Genesis Tree, has no clip, and examining it after
    /// Caruban is what spawns the P2[53] revival. Its stand-in clip never
    /// draws: both hosts pose only `anim_id != 0` placements. An unposed
    /// touch-class object stays with the walk-touch dispatch
    /// (`FieldPropState::walk_touch`), which resolves its
    /// record's arm against the live flags at contact time.
    ///
    /// `clip` resolves an anim id to `(frame_count, scaled_step, step_div)`
    /// from the scene's ANM bundle (record `anim_id - 1`); a prop whose clip
    /// does not resolve gets a 1-frame stand-in clip, so its script's
    /// end-latch spin still passes (a missing ANM bundle must not leave a
    /// door's `2D 08` spin - and therefore its collision - stuck forever).
    pub fn build(
        placements: &[Placement],
        binds: &HashMap<(u8, u8), ObjectBind>,
        man_file: &ManFile,
        man: &[u8],
        clip: impl FnMut(u8) -> Option<(u16, bool, u8)>,
    ) -> Self {
        Self::build_with_flags(placements, binds, man_file, man, clip, None)
    }

    /// [`Self::build`] with each prop's spawn pass run against the live
    /// story flags ([`run_spawn_prologue`]): the prologue's flag tests pick
    /// its arm, and its flag writes land through `flags`.
    pub fn build_with_flags(
        placements: &[Placement],
        binds: &HashMap<(u8, u8), ObjectBind>,
        man_file: &ManFile,
        man: &[u8],
        mut clip: impl FnMut(u8) -> Option<(u16, bool, u8)>,
        mut flags: Option<&mut SpawnFlags<'_>>,
    ) -> Self {
        let mut bank = PropAnimBank {
            // Capture the whole id space up front: a prop resolves its clip
            // once at spawn, but a *cross-context* poke (`A2 <target> <clip>`)
            // arrives mid-conversation, long after the bundle borrow is gone.
            clips: (0..=u8::MAX)
                .map(|id| if id == 0 { None } else { clip(id) })
                .collect(),
            ..Default::default()
        };
        for p in placements {
            let anchor = (p.anchor_col, p.anchor_row);
            if bank.props.contains_key(&anchor) {
                continue;
            }
            let Some(bind) = binds.get(&anchor) else {
                continue;
            };
            let (frames, scaled, div) = bank.clip_meta(bind.anim_id).unwrap_or((1, false, 0));
            let Some((record, pc0)) = partition0_record(man_file, man, bind.record as usize) else {
                continue;
            };
            let (program, parked_pc) = decode_prop_program_with(record, pc0, flags.as_deref_mut());
            // An unposed bind whose record moves the player (a door marker)
            // is the walk-touch dispatch's, which resolves its record against
            // the live flags. Any other clip-less bind still owns its touch:
            // retail's static-class contact posts the touch whatever the
            // actor draws (`FUN_801D5B5C`), and the record runs from its
            // parked cursor. `chitei2`'s lift-cage doors (partition-0 records
            // 6..11, a bare `31 00`) stand solid until the touch runs that
            // `31 00`; without a bank entry they never opened, and the lift
            // platform's way west to the deroa door stayed shut.
            if bind.anim_id == 0
                && program.spawn_cflags & 0x4002_0000 == 0
                && legaia_engine_field::man_field_scripts::p0_record_walk_touch_event(
                    man_file,
                    man,
                    bind.record as usize,
                )
                .is_some()
            {
                continue;
            }
            let mut anim = PropAnim::spawned(bind.anim_id, frames, scaled, div);
            for c in &program.spawn {
                c.apply(&mut anim);
            }
            // The spawn pass is run by `FUN_8003A55C` before the first actor
            // tick, and the tick then consumes the restart request it left.
            anim.tick();
            let cflags = program.spawn_cflags;
            bank.props.insert(
                anchor,
                PropAnimState {
                    anim,
                    program,
                    world: (p.world_x, p.world_z),
                    collider: (p.collider_x, p.collider_z),
                    record: bind.record as usize,
                    record_body: std::sync::Arc::new(record.to_vec()),
                    parked_pc,
                    cflags,
                },
            );
        }
        bank
    }

    /// The clip frame prop `anchor` currently poses from, if it has one.
    pub fn frame(&self, anchor: (u8, u8)) -> Option<usize> {
        self.props.get(&anchor).map(|p| p.anim.frame())
    }

    /// The pose key prop `anchor` draws with ([`PropAnim::pose_key`]), if it
    /// has a clip.
    pub fn pose_key(&self, anchor: (u8, u8)) -> Option<PropPoseKey> {
        self.props.get(&anchor).map(|p| p.anim.pose_key())
    }

    /// Advance every prop's clip one frame (the per-actor anim tick
    /// `FUN_800204F8`, which runs unconditionally - the windmill turns whether
    /// or not anyone is near). Touch / interact dispatch is the world's job
    /// (`World::start_prop_interaction` runs the touched prop's record through
    /// the field VM); this only steps the cursors.
    pub fn tick_anims(&mut self) {
        self.tick_anims_scaled(1);
    }

    /// [`Self::tick_anims`] for a game frame `scale` vsyncs long
    /// ([`PropAnim::tick_scaled`]).
    pub fn tick_anims_scaled(&mut self, scale: u16) {
        for p in self.props.values_mut() {
            p.anim.tick_scaled(scale);
        }
        for a in self.actor_clips.values_mut() {
            a.tick_scaled(scale);
        }
        self.ticked_this_frame = true;
    }

    /// Advance the cross-context cursors for a frame in which
    /// [`Self::tick_anims`] has not already done it, and return whether that
    /// tick happened here.
    ///
    /// The world reaches the clip cursors down two paths - the field frame's
    /// `World::tick_prop_interactions` and, for a host that drives only a
    /// conversation, `World::step_inline_dialogue`. A cursor that advanced
    /// twice in one frame would latch its end early, and one that advanced
    /// zero times would leave a script's end-latch spin waiting on a clip that
    /// never moves, so exactly one of the two ticks each frame.
    pub fn tick_actor_clips_for_frame(&mut self) -> bool {
        self.tick_actor_clips_for_frame_scaled(1)
    }

    /// [`Self::tick_actor_clips_for_frame`] for a game frame `scale` vsyncs
    /// long; `scale` `0` (a vsync between game frames) ticks nothing and
    /// leaves the once-per-frame guard as it is.
    pub fn tick_actor_clips_for_frame_scaled(&mut self, scale: u16) -> bool {
        if scale == 0 {
            return false;
        }
        if std::mem::take(&mut self.ticked_this_frame) {
            return false;
        }
        for a in self.actor_clips.values_mut() {
            a.tick_scaled(scale);
        }
        true
    }

    /// Scene ANM metadata for `anim_id` (`(frame_count, scaled_step,
    /// step_div)` of record `anim_id - 1`), or `None` when the scene carries
    /// no bundle / no such record.
    pub fn clip_meta(&self, anim_id: u8) -> Option<(u16, bool, u8)> {
        *self.clips.get(anim_id as usize)?
    }

    /// The live clip cursor a cross-context poke at `target` reads and writes,
    /// if one has been bound.
    pub fn actor_clip(&self, target: u8) -> Option<&PropAnim> {
        self.actor_clips.get(&target)
    }

    /// Mutable form of [`Self::actor_clip`].
    pub fn actor_clip_mut(&mut self, target: u8) -> Option<&mut PropAnim> {
        self.actor_clips.get_mut(&target)
    }

    /// The player's clip cursor, created on first use in the state retail's
    /// player actor idles in: the actor template's looping `+0x62`
    /// ([`ANIM_SPAWN_FLAGS`]) over the locomotion clip, whose length
    /// `locomotion_frames` supplies. A looping clip latches [`ANIM_END`] once
    /// per wrap, which is why a script may clear and wait on the latch without
    /// poking a clip of its own first.
    pub fn player_clip(&mut self, locomotion_frames: u16) -> &mut PropAnim {
        self.actor_clips
            .entry(PLAYER_ANCHOR_TARGET)
            .or_insert_with(|| PropAnim::cross_context(LOCOMOTION_MOVE_ID, locomotion_frames))
    }

    /// The cursor of a cross-context `target` that is not the player, created
    /// on first use from `live` - the actor's own running clip, when the
    /// world tracks one - else in the actor-template state over
    /// `fallback_frames`, under [`NPC_CLIP_STANDIN_ID`]. Every spawned actor
    /// loops its clip from the template `+0x62`, so its end latch lands once
    /// per wrap and a script may clear and wait on it without poking a clip
    /// first: `vozz` P1[7], the Genesis Tree talk, raises actor `0x06`'s clamp
    /// (`AB 06 03`), clears its latch and spins on it before the scene goes
    /// on.
    pub fn actor_clip_or_live(
        &mut self,
        target: u8,
        live: Option<PropAnim>,
        fallback_frames: u16,
    ) -> &mut PropAnim {
        self.actor_clips.entry(target).or_insert_with(|| {
            live.unwrap_or_else(|| PropAnim::cross_context(NPC_CLIP_STANDIN_ID, fallback_frames))
        })
    }

    /// Bind a cross-context clip poke - `A2 <target> <clip>`, retail's op-`0x22`
    /// `ExecMove` writing `ctx[+0x5C]` and calling the anim tick - onto
    /// `target`'s cursor.
    ///
    /// This is the **binder** half of `FUN_800204F8`, and it is faithful about
    /// the two things that matter to a following end-latch spin: the rebind is
    /// skipped entirely when the id is unchanged (`lh v1,0x5c` / `lh v0,0x5e` /
    /// `beq v1,v0`), and when it does run it zeroes the frame cursor
    /// (`sh zero,0x68(s0)`) and leaves `+0x62` alone - the script's own
    /// `2B`/`2C` ops own the hold / clamp / reverse bits, and clearing the end
    /// latch is the script's `AC <target> 08`, not the bind's.
    ///
    /// `fallback_frames` sizes the cursor when the scene bundle cannot
    /// (see [`PLAYER_CLIP_STANDIN_FRAMES`]).
    ///
    /// PORT: FUN_800204F8 (binder half)
    pub fn bind_actor_clip(&mut self, target: u8, anim_id: u8, fallback_frames: u16) {
        let meta = self.clip_meta(anim_id);
        let entry = self
            .actor_clips
            .entry(target)
            .or_insert_with(|| PropAnim::cross_context(0, fallback_frames));
        if entry.anim_id == anim_id {
            return;
        }
        let (frames, scaled, div) = meta.unwrap_or((fallback_frames.max(1), false, 0));
        entry.anim_id = anim_id;
        entry.frames = frames.max(1);
        entry.scaled_step = scaled;
        entry.step_div = div;
        entry.cursor = 0;
    }
}
