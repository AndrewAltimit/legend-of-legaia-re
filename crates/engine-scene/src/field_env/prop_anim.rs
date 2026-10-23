//! Placed-prop animation state: the `actor+0x62` anim flag bits, `PropAnim` playback and
//! the per-bone pose offsets it produces.

// ---------------------------------------------------------------------------
// Placed-prop animation: the door swing.
// ---------------------------------------------------------------------------

/// `actor+0x62` bit `0x0002` - **hold**. Set, the per-frame anim tick skips the
/// cursor advance, so the clip freezes at whatever frame it is on.
pub const ANIM_HOLD: u16 = 0x0002;
/// `actor+0x62` bit `0x0008` - **clamp** (one-shot). Set, the cursor stops at
/// the end of the clip; clear, it wraps (the looping idle case).
pub const ANIM_CLAMP: u16 = 0x0008;
/// `actor+0x62` bit `0x0080` - **reverse**. Set, the cursor counts down.
pub const ANIM_REVERSE: u16 = 0x0080;
/// `actor+0x62` bit `0x0100` - the **end latch**. Set by the tick on the frame
/// the cursor reaches either end of the clip; scripts wait on it.
pub const ANIM_END: u16 = 0x0100;
/// `actor+0x62` bit `0x0200` - **restart request**. Consumed by the next tick,
/// which snaps the cursor to frame `0` (forward) or the last frame (reverse).
pub const ANIM_RESTART: u16 = 0x0200;

/// The `+0x62` word every actor is born with, from the placed-object actor
/// template `DAT_80073E70` (`FUN_80020DE0` copies `template[1]` into `+0x62`):
/// no hold, no clamp, forward - i.e. **looping playback**, which is what the
/// scene's NPCs run their idle clips under.
pub const ANIM_SPAWN_FLAGS: u16 = 0x0015;

/// The `+0x6A` rate a placed object is born with: the template's `0x10`
/// (1 frame per tick, since the cursor is in 1/16-frame units) halved by
/// `FUN_8003A55C`'s `*(short *)(actor + 0x6a) >>= 1`.
pub const ANIM_SPAWN_RATE: i16 = 8;

/// The cross-context target byte that names the **player** actor. Retail's
/// id-to-context resolver `FUN_8003C83C` special-cases it ahead of the
/// actor-list walk - `li v0,0xf8` / `bne a0,v0` / `lw v0,-0x3c9c(v0)`, i.e.
/// return the live player object out of `_DAT_8007C364` - so an NPC record's
/// `A2 F8 <clip>` / `AC F8 08` / `AD F8 08` triple reads and writes the
/// *player's* `+0x62`, not the dispatching record's.
///
/// REF: FUN_8003C83C
pub const PLAYER_ANCHOR_TARGET: u8 = 0xF8;

/// Frame count the player's clip cursor falls back to when neither the scene
/// ANM bundle nor an installed [`legaia_engine_field::field_anim::FieldPlayerAnim`] can
/// measure the real clip - the player-side twin of the prop bank's 1-frame
/// stand-in. It only has to be finite: what it bounds is how long a script's
/// end-latch spin waits, and a headless world has no clip to watch anyway.
pub const PLAYER_CLIP_STANDIN_FRAMES: u16 = 15;

/// The clip id a field NPC's cross-context cursor carries when a script
/// reaches its `+0x62` before poking a clip and the world tracks no running
/// clip for it (a headless world binds none). It is not a real move: any
/// non-zero id makes [`PropAnim::tick`] advance the cursor, so the end latch
/// lands once per wrap as the actor's looping idle clip would latch it, and
/// a later `A2 <target> <clip>` poke always rebinds (the binder skips only an
/// unchanged id).
pub const NPC_CLIP_STANDIN_ID: u8 = 0xFF;

/// The move id the player actor's `+0x5C` carries while the locomotion
/// controller owns the clip. Ids `1`/`2` are the walk moves the field
/// controller animates itself; a scripted gesture pokes a higher id, and
/// poking `1`/`2` back is how a record hands the player to locomotion again
/// (the `A2 F8 02` that closes the innkeeper's bow).
pub const LOCOMOTION_MOVE_ID: u8 = 1;

/// Half-extent of the prop contact box, in world units. Same box the field
/// locomotion's static-entity collision arm uses for a placed record
/// (`World::props.colliders` / `FIELD_PROP_BOX_HALF`); retail's
/// `FUN_801CFC40` links the player and the touched actor through their `+0x98`
/// partner slots and `FUN_801D5B5C` then resumes the touched actor's script.
pub const PROP_TOUCH_BOX_HALF: i32 = 0x40 + 0x10;

/// Live animation state of one placed prop - the fields of the retail actor
/// record that the per-frame anim tick reads and writes.
///
/// | field | actor offset | meaning |
/// |---|---|---|
/// | `anim_id` | `+0x5C` | clip id; `0` = none. Clip = scene-ANM record `anim_id - 1`. |
/// | `cursor` | `+0x68` | frame cursor in **1/16-frame** units (`frame = cursor >> 4`). |
/// | `flags` | `+0x62` | the `ANIM_*` bits above. |
/// | `rate` | `+0x6A` | cursor units added per tick. |
///
/// PORT: FUN_800204F8 (the binder + per-frame advancer, called from the actor
/// tick `FUN_80021DF4`)
/// REF: FUN_8001B964 (the draw walker: `frame = (i16)(actor+0x68) >> 4`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PropAnim {
    /// Clip id (`actor+0x5C`).
    pub anim_id: u8,
    /// Frame count of the bound clip (the ANM record's `b` header field).
    pub frames: u16,
    /// Bit 0 of the ANM record header's `a` high byte: selects the scaled step.
    pub scaled_step: bool,
    /// The ANM record header's `flag` low byte - the scaled step's divisor.
    pub step_div: u8,
    /// Frame cursor in 1/16-frame units (`actor+0x68`).
    pub cursor: i16,
    /// Animation control word (`actor+0x62`).
    pub flags: u16,
    /// Per-tick cursor step (`actor+0x6A`).
    pub rate: i16,
}

impl PropAnim {
    /// A freshly spawned placed prop: the actor-template flags and the halved
    /// template rate, cursor at zero.
    pub fn spawned(anim_id: u8, frames: u16, scaled_step: bool, step_div: u8) -> Self {
        Self {
            anim_id,
            frames,
            scaled_step,
            step_div,
            cursor: 0,
            flags: ANIM_SPAWN_FLAGS,
            rate: ANIM_SPAWN_RATE,
        }
    }

    /// An actor a script reaches **cross-context**, in the state the actor
    /// template leaves every spawned actor in: looping playback
    /// ([`ANIM_SPAWN_FLAGS`]) at the template rate, cursor at zero. `anim_id`
    /// is the actor's live `+0x5C` - `0` for one that has never been poked, the
    /// locomotion move for the player.
    pub fn cross_context(anim_id: u8, frames: u16) -> Self {
        Self {
            anim_id,
            frames: frames.max(1),
            scaled_step: false,
            step_div: 0,
            cursor: 0,
            flags: ANIM_SPAWN_FLAGS,
            rate: ANIM_SPAWN_RATE,
        }
    }

    /// The clip frame the draw walker poses from: `(i16)(actor+0x68) >> 4`.
    pub fn frame(&self) -> usize {
        (self.cursor >> 4).max(0) as usize
    }

    /// What the draw poses this prop with: the cursor the frame blender reads
    /// plus the clamp bit its last-frame rule reads, normalised so two states
    /// that pose identically compare equal. [`PropPoseKey::REST`] is frame 0
    /// exactly - the baked rest mesh.
    ///
    /// `scaled_step` is the record's blend gate as well as its step selector
    /// (both read clip byte `+1` bit 0; see
    /// [`legaia_actor_formats::player_anm::PlayerAnmRecord::blends`]), so a clip that
    /// does not blend drops the sub-frame fraction here, and the clamp bit is
    /// kept only where it can matter - a fraction on the last frame.
    pub fn pose_key(&self) -> PropPoseKey {
        let frames = i32::from(self.frames.max(1));
        let frame = i32::from(self.cursor >> 4).clamp(0, frames - 1);
        let frac = if self.scaled_step {
            i32::from(self.cursor as u16 & 0xF)
        } else {
            0
        };
        PropPoseKey {
            cursor: (frame * 16 + frac) as i16,
            hold_at_end: frac != 0 && frame == frames - 1 && self.flags & ANIM_CLAMP != 0,
        }
    }

    /// True once the cursor has reached an end of the clip and latched
    /// [`ANIM_END`] - what a script's "wait for the animation" spin tests.
    pub fn at_end(&self) -> bool {
        self.flags & ANIM_END != 0
    }

    /// One frame of the retail anim tick: consume a restart request, step the
    /// cursor unless held, then wrap (loop) or clamp (one-shot) at either end,
    /// latching [`ANIM_END`] when an end is hit.
    ///
    /// REF: FUN_800204F8
    pub fn tick(&mut self) {
        self.tick_scaled(1);
    }

    /// [`Self::tick`] for a game frame `scale` vsyncs long: the anim tick
    /// multiplies its step by the frame-step scalar `DAT_1F800393` before it
    /// adds it (`mult a0,v0` at `0x80020660` / `0x80020680`), so one retail
    /// tick moves the cursor `step * scale` units.
    ///
    /// PORT: FUN_800204F8
    pub fn tick_scaled(&mut self, scale: u16) {
        if self.anim_id == 0 || self.frames == 0 {
            return;
        }
        let span = (self.frames as i32) * 16;
        // The clip's own per-frame step scaling (`clip[1] & 1` selects it,
        // `clip[6]` divides). The same bit is the frame blender's gate, so a
        // prop on this arm is also posed between keyframes. A minority of the
        // disc's posed props take it (`tests/field_prop_pose_sampler_disc.rs`
        // counts them).
        let step = legaia_engine_vm::actor_tick::clip_step(
            self.rate as u16,
            self.scaled_step,
            self.step_div,
        )
        .wrapping_mul(scale.max(1)) as i16;
        if self.flags & ANIM_RESTART != 0 {
            self.flags &= !ANIM_RESTART;
            self.cursor = if self.flags & ANIM_REVERSE == 0 {
                0
            } else {
                (span - 16).max(0) as i16
            };
        }
        // Retail re-reads the flag word here, so the restart bit it just
        // cleared is not what the hold test below sees.
        let flags = self.flags;
        self.flags &= !ANIM_END;
        if flags & ANIM_HOLD == 0 {
            self.cursor = if flags & ANIM_REVERSE == 0 {
                self.cursor.wrapping_add(step)
            } else {
                self.cursor.wrapping_sub(step)
            };
        }
        if self.cursor < 0 {
            self.cursor = if self.flags & ANIM_CLAMP == 0 {
                self.cursor.wrapping_add(span as i16)
            } else {
                0
            };
            self.flags |= ANIM_END;
        }
        let last = (span - 1) as i16;
        if last <= self.cursor {
            self.cursor = if self.flags & ANIM_CLAMP == 0 {
                0
            } else {
                last
            };
            self.flags |= ANIM_END;
        }
    }
}

/// The two inputs of the frame blender `FUN_8001BE80` a posed prop's draw
/// depends on, the cursor (`actor+0x68`) and the clamp bit (`actor+0x62 & 8`),
/// normalised by [`PropAnim::pose_key`]. Equal keys pose identically, which
/// is what lets a host skip a re-pose (and the page carry the key across the
/// wasm boundary as one `i32`, [`Self::to_i32`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PropPoseKey {
    /// Frame cursor in 1/16-frame units.
    pub cursor: i16,
    /// The clamp bit: on the last frame, blend toward the frame itself rather
    /// than wrapping to frame 0.
    pub hold_at_end: bool,
}

impl PropPoseKey {
    /// Frame 0, no fraction - the rest pose every prop is baked in.
    pub const REST: Self = Self {
        cursor: 0,
        hold_at_end: false,
    };

    /// Whether this is [`Self::REST`].
    pub fn is_rest(self) -> bool {
        self == Self::REST
    }

    /// Pack into a non-negative `i32`: the cursor's 16 bits, the clamp bit at
    /// bit 16.
    pub fn to_i32(self) -> i32 {
        i32::from(self.cursor as u16) | (i32::from(self.hold_at_end) << 16)
    }

    /// Inverse of [`Self::to_i32`].
    pub fn from_i32(v: i32) -> Self {
        Self {
            cursor: (v & 0xFFFF) as u16 as i16,
            hold_at_end: v & 0x1_0000 != 0,
        }
    }
}

/// Per-bone `(translation, rotation)` offsets of scene ANM record
/// `anim_id - 1` posed at `key` - the transform list both hosts' posed-prop
/// rebuilds (`tmd_to_*_mesh_posed_rot` on native, the page's hybrid env mesh)
/// feed their mesh builders. One kernel so the two hosts cannot pose a prop
/// differently. A bone that does not resolve poses at the origin, as the
/// hosts' single-frame decode did. `None` when `anim_id` is `0`.
///
/// REF: FUN_8001B964 (the per-part loop: one blender call per part)
/// REF: FUN_8001BE80 (the two-frame sampler, ported at
/// `PlayerAnmBundle::sample_bone`)
pub fn prop_bone_offsets(
    bundle: &legaia_actor_formats::player_anm::PlayerAnmBundle,
    anim_id: u8,
    key: PropPoseKey,
    bones: usize,
) -> Option<Vec<([i16; 3], [i16; 3])>> {
    let rec = (anim_id as usize).checked_sub(1)?;
    Some(
        (0..bones)
            .map(
                |b| match bundle.sample_bone(rec, key.cursor, key.hold_at_end, b) {
                    Some(t) => (
                        [t.t_x as i16, t.t_y as i16, t.t_z as i16],
                        [t.r_x as i16, t.r_y as i16, t.r_z as i16],
                    ),
                    None => ([0; 3], [0; 3]),
                },
            )
            .collect(),
    )
}

/// [`prop_bone_offsets`] under retail's **count-equality contract**: the draw
/// walker (`FUN_8001B964`) refuses to pose a prop whose mesh chain and clip
/// disagree on the part count, so `None` - draw the raw mesh - unless scene
/// ANM record `anim_id - 1` has exactly `object_count` bones. The one gate
/// every host's posed-prop build goes through.
///
/// REF: FUN_8001B964
pub fn posed_prop_offsets(
    bundle: &legaia_actor_formats::player_anm::PlayerAnmBundle,
    anim_id: u8,
    key: PropPoseKey,
    object_count: usize,
) -> Option<Vec<([i16; 3], [i16; 3])>> {
    let rec = bundle.record((anim_id as usize).checked_sub(1)?).ok()?;
    let bones = rec.bone_count as usize;
    if bones != object_count {
        return None;
    }
    prop_bone_offsets(bundle, anim_id, key, bones)
}
