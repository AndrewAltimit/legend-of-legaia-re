//! Script-cutscene *element* handlers: the per-frame actor bodies a cutscene
//! script installs at `actor+0x0C` alongside the camera controller.
//!
//! Each element is an ordinary field actor whose handler runs once per frame
//! and drives some **other** object - the "linked object" pointer at `+0x90`.
//! The three handlers ported here are the position tween, the teardown, and
//! the ambient particle emitter.
//!
//! Two more of the same family sit at the end of the file: the party-leader
//! swap controller [`LeaderSwap`] (`FUN_801D27E0`) and the geometry colour
//! grade [`shift_primitive_colours`] (`FUN_801D5E20`).
//!
//! The four `PORT` tags for the handlers above live on the items that
//! implement them - [`PositionTween::step`], [`ElementTeardown::step`],
//! [`AmbientEmitter::step`] and [`flash_element_spawn`] - rather than on this
//! module. A module-level tag makes the whole file the reachability anchor, and
//! the file's other items *are* reachable, which reports a wired port that is
//! not one. A tag on a `const` degrades the same way, which is why
//! `FUN_801D841C`'s three constants are wrapped in a function instead.
//!
//! NOT WIRED: there is no element-actor dispatch to hang these off.
//! `crate::cutscene` is the FMV dispatch-table lookup and nothing else; the
//! engine's scripted scenes run through [`crate::cutscene_timeline`], which
//! interprets the record's cross-context yields (walk / rotate / channel
//! waits) **directly** instead of spawning an object whose `+0x0C` holds a
//! handler and whose `+0x90` points at a linked object. Wiring these four
//! needs that element-actor channel first: a pool of spawned elements, each
//! carrying a linked-object pointer, ticked once per frame with the linked
//! object's done bit `+0x10 & 8` as the entry gate. Until then a call site
//! would have nothing to link to and nothing to retire.
//!
//! REF: FUN_801E45BC - the vector midpoint/lerp helper the tween calls.
//! REF: FUN_801D629C - the particle spawn primitive the emitter calls.
//! REF: FUN_801DB510, FUN_801DAA50 - the camera restore pair the teardown calls.
//! REF: FUN_80020DE0 - the descriptor spawner, hosted in
//! [`crate::actor_alloc_host`].
//!
//! Read off `ghidra/scripts/funcs/overlay_cutscene_dialogue_801d5c08.txt`,
//! `..._801d5d60.txt`, `..._801d6058.txt` and `..._801d841c.txt` -
//! disassembly, not the C.
//!
//! `0x801D841C` is **VA-aliased**: the root dump `801d841c.txt` resolves
//! `entry=801D8308` in a different overlay image. The body ported here is the
//! one the cutscene / field band holds, which is the 13-instruction spawner
//! both `overlay_cutscene_dialogue_801d841c.txt` and
//! `overlay_cutscene_mapview_801d841c.txt` attest.

/// The 8-byte position blob these handlers move around.
///
/// The tween copies **eight bytes** at a time with `lwl`/`lwr` +
/// `swl`/`swr` pairs (`0x801D5C7C..0x801D5C98`), so the unit of transfer is
/// four halfwords, not three. Component 1 is the one the facing/sort write
/// negates.
///
/// The `x`/`y`/`z` naming follows the actor position layout the field
/// locomotion controller drives (`docs/subsystems/field-locomotion.md`); this
/// function itself only distinguishes component 1 from the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ElementVec {
    /// `+0x00` - X.
    pub x: i16,
    /// `+0x02` - Y. The component `+0x8E` is written the negation of.
    pub y: i16,
    /// `+0x04` - Z.
    pub z: i16,
    /// `+0x06` - the fourth halfword the 8-byte copy carries along.
    pub w: i16,
}

impl ElementVec {
    /// The linear reading of the `FUN_801E45BC` blend at parameter `t`
    /// (`0..=0x1000`).
    ///
    /// The retail helper's exact rounding is **not** pinned from this
    /// function's disassembly - `FUN_801E45BC` is only reached through a
    /// `jal`, and the tween passes it `(out, start, end, t)` without looking
    /// at what it does. This is the straight-line reading; treat it as the
    /// engine's choice, not as a decoded retail formula.
    pub fn blend(start: Self, end: Self, t: i16) -> Self {
        let f = |a: i16, b: i16| -> i16 {
            let a = i32::from(a);
            let b = i32::from(b);
            (a + ((b - a) * i32::from(t)) / 0x1000) as i16
        };
        Self {
            x: f(start.x, end.x),
            y: f(start.y, end.y),
            z: f(start.z, end.z),
            w: f(start.w, end.w),
        }
    }
}

/// The `FUN_801D5C08` element record, in the fields the handler touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PositionTween {
    /// `+0x14` - start vector.
    pub start: ElementVec,
    /// `+0x24` - end vector.
    pub end: ElementVec,
    /// `+0x9C` - the blend parameter accumulator, `0..=0x1000`.
    pub t: i16,
    /// `+0x9E` - per-frame increment of `t`, multiplied by the frame step.
    pub rate: i16,
    /// `+0x10` bit `8` - the element's own done bit.
    pub done: bool,
}

/// What one [`PositionTween::step`] resolved to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TweenStep {
    /// The **linked** object was already done (`linked[+0x10] & 8`), so the
    /// handler skipped straight to setting its own done bit. Nothing was
    /// written to the linked object.
    LinkedAlreadyDone,
    /// Still blending. `pos` goes to `linked+0x14..+0x1B`.
    Blending { pos: ElementVec },
    /// `t` reached `0x1000`: the end vector is written verbatim and the
    /// element's own done bit is now set.
    Snapped { pos: ElementVec },
}

impl PositionTween {
    /// One frame of `FUN_801D5C08`.
    ///
    /// `frame_step` is `DAT_1F800393` (the adaptive vsync step);
    /// `linked_done` is the linked object's `+0x10 & 8`.
    ///
    /// The accumulate is `t = (u16)t + (i16)rate * frame_step` **stored back
    /// as a halfword** (`sh v0,0x9c(s0)`) and only then sign-extended for the
    /// `< 0x1000` test, so the wrap is 16-bit.
    ///
    /// PORT: FUN_801D5C08
    ///
    /// NOT WIRED: no element-actor dispatch - see the module docs.
    pub fn step(&mut self, frame_step: u8, linked_done: bool) -> TweenStep {
        if linked_done {
            self.done = true;
            return TweenStep::LinkedAlreadyDone;
        }
        let acc = (self.t as u16)
            .wrapping_add((i32::from(self.rate) * i32::from(frame_step)) as u16)
            as i16;
        self.t = acc;
        if acc < 0x1000 {
            TweenStep::Blending {
                pos: ElementVec::blend(self.start, self.end, acc),
            }
        } else {
            self.t = 0x1000;
            self.done = true;
            TweenStep::Snapped { pos: self.end }
        }
    }

    /// The `+0x8E` write that accompanies every position write.
    ///
    /// `0x801D5CA8` / `0x801D5D38`: when the linked object is **not** the
    /// camera object `_DAT_8007C364`, the handler stores `-pos.y` into
    /// `linked+0x8E`. The camera is the sole exception, and it is compared by
    /// pointer identity, not by any flag.
    pub fn linked_field_8e(pos: ElementVec, linked_is_camera: bool) -> Option<i16> {
        if linked_is_camera {
            None
        } else {
            Some(pos.y.wrapping_neg())
        }
    }
}

/// The `FUN_801D5D60` element record.
///
/// The handler has two independent halves: an unconditional camera restore
/// (run every frame while armed) and a one-shot flag teardown that waits for
/// the linked object's done bit. The decompiled C renders the first half as a
/// single `&&` condition, which is exactly what the disassembly does
/// (`0x801D5D78` / `0x801D5D88` both branch to the same skip label).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ElementTeardown {
    /// `+0x5C` - the "restore armed" halfword.
    pub restore_armed: i16,
    /// `+0x50` - the "owns the camera" halfword. Gates both the camera
    /// restore and the second flag clear.
    pub owns_camera: i16,
    /// `+0x74` - the flag mask this element installed, cleared on teardown.
    pub flag_mask: u32,
    /// `+0x10` bit `8` - the element's own done bit.
    pub done: bool,
}

/// What one [`ElementTeardown::step`] asks the host to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TeardownActions {
    /// Call the camera restore pair `FUN_801DB510(_DAT_8007C364)` then
    /// `FUN_801DAA50()`.
    pub restore_camera: bool,
    /// Clear `flag_mask` out of the `+0x94` object's flag word `+0x10`.
    pub clear_target_flags: bool,
    /// Clear `flag_mask` out of the camera object's flag word as well.
    pub clear_camera_flags: bool,
}

impl ElementTeardown {
    /// One frame of `FUN_801D5D60`. `linked_done` is `linked[+0x10] & 8`.
    ///
    /// PORT: FUN_801D5D60
    ///
    /// NOT WIRED: no element-actor dispatch - see the module docs.
    pub fn step(&mut self, linked_done: bool) -> TeardownActions {
        let mut out = TeardownActions {
            restore_camera: self.restore_armed != 0 && self.owns_camera != 0,
            ..Default::default()
        };
        if linked_done {
            out.clear_target_flags = true;
            out.clear_camera_flags = self.owns_camera != 0;
            self.done = true;
        }
        out
    }
}

/// The spawn descriptor `FUN_801D841C` hands to `FUN_80020DE0`.
///
/// The whole function is thirteen instructions:
/// `FUN_80020DE0(0x800706BC, _DAT_8007C34C)` followed by a single
/// `sh 1,0x5c(v0)` on the returned actor. `0x800706BC` sits inside the
/// 24-byte spawn-descriptor family at `0x800705FC..0x80070763` that
/// `docs/subsystems/asset-loader.md` already documents (the scene-transition
/// streaming actor uses `0x80070734` out of the same family) - it is **not** a
/// row of the mode table at `0x8007078C`.
pub const FLASH_ELEMENT_DESCRIPTOR: u32 = 0x8007_06BC;

/// The pool discriminator, `_DAT_8007C34C` - the system actor pool the same
/// family of one-shot elements spawns into.
pub const FLASH_ELEMENT_POOL: u32 = 0x8007_C34C;

/// The one field `FUN_801D841C` writes on the actor it just spawned:
/// `+0x5C = 1`.
pub const FLASH_ELEMENT_FIELD_5C: i16 = 1;

/// The whole of `FUN_801D841C`, which is a spawn and one store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlashElementSpawn {
    /// First argument of `FUN_80020DE0` - [`FLASH_ELEMENT_DESCRIPTOR`].
    pub descriptor: u32,
    /// Second argument - [`FLASH_ELEMENT_POOL`].
    pub pool: u32,
    /// Written to `+0x5C` of the actor the spawn returns.
    pub field_5c: i16,
}

/// Build the spawn `FUN_801D841C` performs.
///
/// PORT: FUN_801D841C
///
/// NOT WIRED: no element-actor dispatch - see the module docs. The pool
/// spawner itself is [`crate::actor_alloc_host`]'s port of `FUN_80020DE0`;
/// what is missing is a caller that wants a flash element at all.
pub fn flash_element_spawn() -> FlashElementSpawn {
    FlashElementSpawn {
        descriptor: FLASH_ELEMENT_DESCRIPTOR,
        pool: FLASH_ELEMENT_POOL,
        field_5c: FLASH_ELEMENT_FIELD_5C,
    }
}

/// One particle the ambient emitter asks `FUN_801D629C` to spawn.
///
/// The names are positional: retail passes four arguments, a coordinate pair
/// and a velocity pair, over the **two** components the emitter samples. Which
/// world axes those two are is not established from this function - the point
/// arm reads them out of actor `+0x14` and `+0x18`, and never touches `+0x16`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AmbientParticle {
    /// First argument - spawn coordinate from the `+0x14` component.
    pub x: i32,
    /// Second argument - spawn coordinate from the `+0x18` component.
    pub y: i32,
    /// Third argument - velocity along the first component.
    pub vx: i32,
    /// Fourth argument - velocity along the second component.
    pub vy: i32,
}

/// The two-parameter set `FUN_801D6058` picks between on `_DAT_1F800394 & 1`.
///
/// `0x801D60A4`: bit clear leaves `(bias, y_offset) = (2, 1)`; bit set
/// replaces them with `(6, 0x0E)`. Both are used only by the scene-wide
/// burst arm - the single-particle arm ignores them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AmbientProfile {
    /// `s2` - added to the second-component span before it is halved.
    pub span_bias: i32,
    /// `s8` - subtracted from each particle's second-component offset.
    pub y_offset: i32,
}

impl AmbientProfile {
    /// `_DAT_1F800394 & 1 == 0`.
    pub const LIGHT: Self = Self {
        span_bias: 2,
        y_offset: 1,
    };
    /// `_DAT_1F800394 & 1 != 0`.
    pub const HEAVY: Self = Self {
        span_bias: 6,
        y_offset: 0x0E,
    };

    /// Pick the profile the way `0x801D60A0` does.
    pub fn select(dense: bool) -> Self {
        if dense { Self::HEAVY } else { Self::LIGHT }
    }
}

/// The scene bounds the burst arm samples inside: the four **signed bytes**
/// `DAT_1F8003E8..DAT_1F8003EB` (`lb`, not `lbu`).
///
/// The emitter pairs them `(E8, EA)` and `(E9, EB)` - two spans, one per
/// sampled component - so the layout is `[min0, min1, max0, max1]`, not two
/// consecutive `(min, max)` pairs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SceneSpan {
    /// `DAT_1F8003E8` - minimum of the first component.
    pub x_min: i8,
    /// `DAT_1F8003E9` - minimum of the second component.
    pub y_min: i8,
    /// `DAT_1F8003EA` - maximum of the first component.
    pub x_max: i8,
    /// `DAT_1F8003EB` - maximum of the second component.
    pub y_max: i8,
}

/// Scene-wide state the ambient emitter reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AmbientScene {
    /// `_DAT_8007B854` - the master gate. Zero makes the whole handler a
    /// no-op.
    pub enabled: bool,
    /// `_DAT_1F800394 & 1` - the density selector.
    pub dense: bool,
    /// The `DAT_1F8003E8..EB` span.
    pub span: SceneSpan,
    /// `_DAT_80089118` - the origin the burst arm re-centres the first
    /// component on. Read as a word and arithmetic-shifted right by 7, so it
    /// is in the same units as the actor position after its own `>> 7`.
    pub camera_x: i32,
    /// `_DAT_80089120` - the same for the second component.
    pub camera_y: i32,
}

/// The ambient emitter, `FUN_801D6058`.
///
/// The actor's state halfword `+0x1A` picks between two completely different
/// arms:
///
/// * `0` - the **point** arm. One in sixteen frames (`rand & 0xF == 0`) it
///   spawns exactly one particle at the actor's own position, arithmetic-
///   shifted right by 7, with a velocity of `((rand & 0xF) - 8) * 2` per
///   axis.
/// * anything else - the **scene** arm. Twenty-four independent draws, each
///   with a one-in-sixteen chance of firing a burst of `(rand & 3) + 1`
///   particles at one sampled point.
///
/// The C rendering of the burst count guard (`if ((uVar1 & 3) != 0xffffffff)`)
/// is a decompiler artifact: the disassembly is `addiu s2,v0,1` followed by
/// `beq s2,zero`, i.e. a `count == 0` test on a value that is always
/// `1..=4`. The burst therefore always runs at least once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AmbientEmitter {
    /// `+0x1A` - arm selector.
    pub state: i16,
    /// `+0x14` - the actor's first position component (point arm only).
    pub actor_x: u16,
    /// `+0x18` - the actor's second position component (point arm only).
    /// `+0x16`, the component between them, is never read here.
    pub actor_y: u16,
}

/// Number of independent draws the scene arm makes per frame (`slti s7,0x18`).
pub const AMBIENT_SCENE_DRAWS: usize = 0x18;

impl AmbientEmitter {
    /// The actor position the point arm spawns at: `(i16)pos << 16 >> 23`,
    /// i.e. an arithmetic shift right by 7 of the sign-extended halfword.
    pub fn point_origin(&self) -> (i32, i32) {
        (
            i32::from(self.actor_x as i16) >> 7,
            i32::from(self.actor_y as i16) >> 7,
        )
    }

    /// One frame of `FUN_801D6058`.
    ///
    /// `rand` yields the successive `FUN_80056798` results in call order; the
    /// port consumes exactly as many as retail does on the path it takes, so
    /// the RNG stream stays aligned.
    ///
    /// PORT: FUN_801D6058
    ///
    /// NOT WIRED: no element-actor dispatch - see the module docs.
    pub fn step(
        &self,
        scene: &AmbientScene,
        mut rand: impl FnMut() -> u32,
    ) -> Vec<AmbientParticle> {
        let mut out = Vec::new();
        if !scene.enabled {
            return out;
        }
        let profile = AmbientProfile::select(scene.dense);

        if self.state == 0 {
            if rand() & 0xF != 0 {
                return out;
            }
            let (x, y) = self.point_origin();
            let vx = ((rand() & 0xF) as i32 - 8) * 2;
            let vy = ((rand() & 0xF) as i32 - 8) * 2;
            out.push(AmbientParticle { x, y, vx, vy });
            return out;
        }

        // 0x801D6158: the two spans, each clamped to a positive modulus. The
        // X span is clamped to 1; the Y span takes the profile bias whether
        // or not it was clamped (the `addu s3,s3,s2` sits in the branch's
        // delay slot and is re-issued on the clamped path).
        let mut x_span = i32::from(scene.span.x_max) - i32::from(scene.span.x_min) - 1;
        if x_span <= 0 {
            x_span = 1;
        }
        let mut y_span = i32::from(scene.span.y_max) - i32::from(scene.span.y_min) - 1;
        if y_span <= 0 {
            y_span = 1;
        }
        y_span += profile.span_bias;

        let base_x = -(scene.camera_x >> 7);
        let base_y = -(scene.camera_y >> 7) - (y_span >> 1);

        for _ in 0..AMBIENT_SCENE_DRAWS {
            let rx = rand() as i32;
            let ry = rand() as i32;
            let x = base_x + (rx % x_span - (x_span >> 1));
            let y = base_y + (ry % y_span - profile.y_offset);
            if rand() & 0xF != 0 {
                continue;
            }
            let count = (rand() & 3) + 1;
            for _ in 0..count {
                let vx = (rand() & 0x1F) as i32 - 0xF;
                let vy = (rand() & 0x1F) as i32 - 0xF;
                out.push(AmbientParticle { x, y, vx, vy });
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// FUN_801D27E0 - the field party-leader swap
// ---------------------------------------------------------------------------

/// The story-flag index the swap controller requires before it will run at all
/// (`func_0x8003CE64(0xD)`). Clear retires the controller outright.
pub const LEADER_SWAP_ENABLE_FLAG: u16 = 0x0D;

/// Story flags `0x10`, `0x11` and `0x12` encode *which* of the three party
/// characters currently leads: the swap clears all three and sets
/// `LEADER_FLAG_BASE + new_leader`.
pub const LEADER_FLAG_BASE: u16 = 0x10;

/// Party slots the controller cycles through (`sltiu v0,s2,0x3`).
pub const LEADER_SLOTS: u8 = 3;

/// Frames each of the two fade halves runs for (`li s5,0x20`) - it is both the
/// fade template's duration and the `+0x9E` counter's bound.
pub const LEADER_SWAP_FADE_FRAMES: u16 = 0x20;

/// Bit the controller raises on the camera object's flag word `+0x10` while a
/// swap is in flight, and lowers again in the final state.
pub const CAMERA_BUSY_BIT: u32 = 0x0008_0000;

/// Pad-word bit (`_DAT_1F800394 & 0x400`) that, like the camera busy bit,
/// routes the arm test through the story flags rather than through
/// `_DAT_8007B874`.
pub const LEADER_SWAP_PAD_BIT: u32 = 0x0400;

/// Bit of `_DAT_8007B874` that requests a swap when neither the camera busy bit
/// nor [`LEADER_SWAP_PAD_BIT`] is set.
pub const LEADER_SWAP_REQUEST_BIT: u32 = 0x80;

/// The value written into the outgoing leader actor's `+0x14` and `+0x18` once
/// the camera has taken its pose (`li v1,0x3f80`).
pub const LEADER_ACTOR_POSE_SENTINEL: i16 = 0x3F80;

/// Shift from world units to the walkability grid's tile units, applied to the
/// camera's x and z before the field-grid calls (`sll 0x10; sra 0x17`, i.e. a
/// sign-extended `>> 7`). It is the same 128-unit tile
/// `docs/subsystems/field-locomotion.md` describes.
pub const WORLD_TO_TILE_SHIFT: u32 = 7;

/// One party actor's cached pose - the `0x10`-byte record state `0` writes for
/// each of the three slots into the table at `0x800845E4`. Each component is
/// stored as a **word**, sign-extended from the actor's halfword.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PartyActorPose {
    /// From actor `+0x14`.
    pub x: i32,
    /// From actor `+0x16`.
    pub y: i32,
    /// From actor `+0x18`.
    pub z: i32,
    /// From actor `+0x26`.
    pub facing: i32,
}

/// What the swap controller asks its host to do this frame, in the order retail
/// issues them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaderSwapEffect {
    /// State `0`: cache all three party actors' poses into the `0x800845E4`
    /// table. Runs before any gate, on every state-0 frame.
    CachePartyPoses,
    /// State `0`: spawn the fade-to-white and keep the object it returns in
    /// `_DAT_801F3490`. The template is `crate::fade::FadeTemplate` kind `2`,
    /// `0x20` frames, black to white.
    SpawnFadeOut,
    /// State `2`: write the camera object's current pose back onto the
    /// **outgoing** leader's actor, so it stands where the camera left it.
    StoreOutgoingPose {
        /// The slot being left.
        slot: u8,
    },
    /// State `2`: the leader index changed. The host writes it to
    /// `_DAT_8007B8F8`, `DAT_80084597` and `DAT_80084598`, clears story flags
    /// `0x10..=0x12` and sets [`LEADER_FLAG_BASE`]` + slot`.
    CommitLeader {
        /// The slot taking over.
        slot: u8,
    },
    /// State `2`: `FUN_801DE190()` - rebuild the party display.
    RefreshParty,
    /// State `2`: copy the incoming leader's pose onto the camera object
    /// (`+0x14`/`+0x1C`, `+0x16`/`+0x1E`, `+0x18`/`+0x20`, `+0x26`) and
    /// re-anchor the field grid on it - `func_0x80017EC8`, `FUN_801DE3E0`, the
    /// negated origin at `_DAT_80089118` / `_DAT_80089120`, then `FUN_801DB8EC`
    /// and `FUN_801DAA50`.
    RecentreCamera {
        /// The slot taking over.
        slot: u8,
    },
    /// State `2`: stamp [`LEADER_ACTOR_POSE_SENTINEL`] into the incoming leader
    /// actor's `+0x14` and `+0x18`.
    ClearIncomingPose {
        /// The slot taking over.
        slot: u8,
    },
    /// State `2`: spawn the fade-back-in (white to black), then
    /// `func_0x8003BDE0(x, z, +0x72, 1)` with the tile-space camera position.
    SpawnFadeIn,
    /// State `3`: OR bit `3` into the spawned fade object's `+0x10`.
    ReleaseFadeObject,
    /// State `4`: clear [`CAMERA_BUSY_BIT`] from the camera object's `+0x10`.
    ClearCameraBusy,
    /// State `5`: clear bit `3` of the controller's own `+0x10` - it retires.
    RetireController,
}

/// The controller's `+0x54` phase and `+0x9E` counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LeaderSwap {
    /// `+0x54` - the six-way phase. The switch is bounded by `sltiu v1,0x6`, so
    /// anything higher runs no body at all.
    pub phase: i16,
    /// `+0x9E` - the fade counter, reset at each phase transition.
    pub counter: u16,
    /// `+0x50` - the story-flag base the presence tests are made against.
    pub flag_base: u16,
}

/// Everything the controller reads that it does not own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LeaderSwapWorld {
    /// `DAT_80084597` - the live leader slot.
    pub leader: u8,
    /// `_DAT_8007B6B4` - the first suppressor. The arm needs it zero.
    pub suppress_a: i32,
    /// `_DAT_8007B6B0` - the second suppressor. The arm needs it `<= 0`.
    pub suppress_b: i32,
    /// The camera object's flag word `+0x10`.
    pub camera_flags: u32,
    /// `_DAT_1F800394` - the pad word.
    pub pad: u32,
    /// `_DAT_8007B874`.
    pub request_byte: u32,
}

/// Result of one controller frame.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LeaderSwapTick {
    /// Host calls in retail order.
    pub effects: Vec<LeaderSwapEffect>,
    /// The leader slot the swap settled on, when state `2` ran.
    pub new_leader: Option<u8>,
}

impl LeaderSwap {
    /// One frame of `FUN_801D27E0` - the field controller that switches which
    /// party member walks the map.
    ///
    /// `flag` tests the story-flag bank `DAT_80085758` (`func_0x8003CE64`). It
    /// is consulted for the enable flag, for each of the three
    /// `flag_base + n` presence flags, and again inside the search that picks
    /// the next leader.
    ///
    /// | state | body |
    /// |---|---|
    /// | `0` | Cache the three poses, run the arm gate, and on success spawn the fade-out and advance. |
    /// | `1` | Hold for [`LEADER_SWAP_FADE_FRAMES`], then advance. |
    /// | `2` | Perform the swap, spawn the fade-in, advance. |
    /// | `3` | Release the fade object, advance. |
    /// | `4` | Hold for [`LEADER_SWAP_FADE_FRAMES`], then clear the busy bit and return to state `0`. |
    /// | `5` | Retire the controller. |
    ///
    /// The arm gate has three parts, in order. **All three** presence flags set
    /// means there is nothing to swap to and the frame ends. Exactly two set
    /// additionally requires `flag_base + leader`. Then either the camera busy
    /// bit or the pad bit routes the final test through `flag_base + leader`,
    /// while their absence falls back to [`LEADER_SWAP_REQUEST_BIT`] of
    /// `_DAT_8007B874`.
    ///
    /// PORT: FUN_801D27E0
    /// REF: FUN_801DE190, FUN_801DE3E0, FUN_801DB8EC, FUN_801DAA50
    ///
    /// NOT WIRED: `legaia_engine_core::World` has one field leader and no
    /// per-slot party actor objects to swap between - the field renders the
    /// leader's mesh from the party record rather than from three resident
    /// actors, so nothing can service
    /// [`LeaderSwapEffect::StoreOutgoingPose`] or
    /// [`LeaderSwapEffect::ClearIncomingPose`]. Wiring it needs those three
    /// resident actors first, and the `0x800845E4` pose table with them.
    pub fn step(
        &mut self,
        world: &LeaderSwapWorld,
        frame_step: u8,
        mut flag: impl FnMut(u16) -> bool,
    ) -> LeaderSwapTick {
        let mut out = LeaderSwapTick::default();
        match self.phase {
            0 => {
                out.effects.push(LeaderSwapEffect::CachePartyPoses);
                if world.suppress_a != 0 || world.suppress_b > 0 {
                    return out;
                }
                if !flag(LEADER_SWAP_ENABLE_FLAG) {
                    self.phase = 5;
                    return out;
                }
                let present = (0..LEADER_SLOTS)
                    .filter(|n| flag(self.flag_base + u16::from(*n)))
                    .count();
                if present == 3 {
                    return out;
                }
                let leader_flag = self.flag_base + u16::from(world.leader);
                if present == 2 && !flag(leader_flag) {
                    return out;
                }
                let armed = if (world.camera_flags & CAMERA_BUSY_BIT) != 0
                    || (world.pad & LEADER_SWAP_PAD_BIT) != 0
                {
                    flag(leader_flag)
                } else {
                    (world.request_byte & LEADER_SWAP_REQUEST_BIT) != 0
                };
                if !armed {
                    return out;
                }
                out.effects.push(LeaderSwapEffect::SpawnFadeOut);
                self.counter = 0;
                self.phase += 1;
            }
            1 => {
                self.counter = self.counter.wrapping_add(u16::from(frame_step));
                if self.counter < LEADER_SWAP_FADE_FRAMES {
                    return out;
                }
                self.counter = 0;
                self.phase += 1;
            }
            2 => {
                out.effects
                    .push(LeaderSwapEffect::StoreOutgoingPose { slot: world.leader });
                // Step past the current slot, wrapping at three, until a
                // presence flag reads clear. The `addiu s2,s2,-1` at
                // `801d2ae4` undoes the increment that ran in the branch's
                // delay slot, which is why the loop settles *on* the clear
                // slot rather than one past it.
                let mut slot = world.leader;
                loop {
                    slot = if slot + 1 >= LEADER_SLOTS {
                        0
                    } else {
                        slot + 1
                    };
                    if !flag(self.flag_base + u16::from(slot)) {
                        break;
                    }
                }
                out.new_leader = Some(slot);
                out.effects.push(LeaderSwapEffect::CommitLeader { slot });
                out.effects.push(LeaderSwapEffect::RefreshParty);
                out.effects.push(LeaderSwapEffect::RecentreCamera { slot });
                out.effects
                    .push(LeaderSwapEffect::ClearIncomingPose { slot });
                out.effects.push(LeaderSwapEffect::SpawnFadeIn);
                self.counter = 0;
                self.phase += 1;
            }
            3 => {
                out.effects.push(LeaderSwapEffect::ReleaseFadeObject);
                self.counter = 0;
                self.phase += 1;
            }
            4 => {
                self.counter = self.counter.wrapping_add(u16::from(frame_step));
                if self.counter >= LEADER_SWAP_FADE_FRAMES {
                    out.effects.push(LeaderSwapEffect::ClearCameraBusy);
                    self.phase = 0;
                }
            }
            5 => out.effects.push(LeaderSwapEffect::RetireController),
            _ => {}
        }
        out
    }
}

// ---------------------------------------------------------------------------
// FUN_801D5E20 - the geometry colour grade
// ---------------------------------------------------------------------------

/// Hue wrap the colour grade applies - `0x167`, which is **359**, not 360.
///
/// `801d5f28`..`801d5f54` is `if (h > 0x167) h -= 0x167;` then
/// `if (h < 0) h += 0x167;`, so a hue of exactly `360` folds to `1` rather than
/// to `0` and a full rotation of the shift walks the palette one step per turn.
/// The off-by-one is retail's.
pub const HUE_WRAP: i32 = 0x167;

/// Saturation and value are clamped to `0..=0xFF` (`li t0,0xff` at `801d5e70`,
/// reloaded across both helper calls).
pub const CHANNEL_MAX: i32 = 0xFF;

/// Address of the per-primitive colour-count table the walker indexes with
/// `group.flags >> 1`. It answers "how many packed RGB words does a primitive
/// of this mode carry", and a zero skips the group's body.
///
/// The selector matters: the TMD renderer `FUN_8002735C` indexes *its* per-mode
/// table at `DAT_8007326C` with `((flags >> 1) - 8) >> 1`, so the two tables are
/// addressed differently and are not interchangeable.
pub const COLOUR_COUNT_TABLE_ADDR: u32 = 0x801F_26F0;

/// The HSV delta applied to every colour word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HsvShift {
    /// Added to the hue, then folded by [`HUE_WRAP`].
    pub hue: i32,
    /// Added to the saturation, then clamped to `0..=`[`CHANNEL_MAX`].
    pub saturation: i32,
    /// Added to the value, then clamped the same way.
    pub value: i32,
}

impl HsvShift {
    /// Apply the shift to one HSV triple, with retail's fold and clamps.
    pub fn apply(&self, h: i32, s: i32, v: i32) -> (i32, i32, i32) {
        let mut h = h + self.hue;
        if h > HUE_WRAP {
            h -= HUE_WRAP;
        }
        if h < 0 {
            h += HUE_WRAP;
        }
        let clamp = |x: i32| -> i32 { x.clamp(0, CHANNEL_MAX) };
        (h, clamp(s + self.saturation), clamp(v + self.value))
    }
}

/// The RGB/HSV conversion pair the grade calls out to (`func_0x8001A78C`
/// RGB -> HSV, `func_0x8001A6C8` HSV -> RGB). They are SCUS leaf helpers, so
/// the grade injects them rather than re-deriving them.
pub trait HsvCodec {
    /// `func_0x8001A78C(r, g, b, &h, &s, &v)`.
    fn to_hsv(&mut self, rgb: [u8; 3]) -> (i32, i32, i32);
    /// `func_0x8001A6C8(h, s, v, &r, &g, &b)`.
    fn to_rgb(&mut self, hsv: (i32, i32, i32)) -> [u8; 3];
}

/// One group header of a Legaia TMD primitive block, in the three fields the
/// grade reads. Layout per `docs/formats/tmd.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PrimGroupHeader {
    /// `+0x00` - primitives in the group.
    pub count: u16,
    /// `+0x02` - the mode/flags word; `flags >> 1` indexes the colour-count
    /// table.
    pub flags: u16,
    /// `+0x05` - words per primitive; the byte stride is `ilen * 4`.
    pub ilen: u8,
}

/// How far the walker's cursor advances over one group, in bytes.
///
/// **It is one primitive too far.** The stride add `ilen * 4` runs once per
/// primitive (`801d5FF8`) *and* once more after the loop (`801d6010`), and the
/// `count == 0` arm jumps straight to that trailing add. Against
/// `docs/formats/tmd.md`'s `count x ilen*4` body that over-runs by one
/// primitive per group. It is reproduced because it is what the bytes say: a
/// consumer that "fixes" it walks a different set of colours than retail does.
pub fn colour_walk_group_stride(header: &PrimGroupHeader) -> usize {
    8 + (usize::from(header.count) + 1) * usize::from(header.ilen) * 4
}

/// Rotate the hue of every packed colour word in a TMD primitive block.
/// `FUN_801D5E20`.
///
/// The block is walked group by group until a group header whose whole first
/// **word** is zero. Within a group, `colour_count(flags >> 1)` decides how many
/// four-byte colour words each primitive carries; `0` skips the group's body
/// entirely. Each colour is the first three bytes of its word - the fourth byte
/// is left alone.
///
/// The cursor arithmetic is [`colour_walk_group_stride`]; read its docs before
/// wiring this to a real block.
///
/// PORT: FUN_801D5E20
/// REF: FUN_801D8280 - the `DAT_8007C018` resident-object table walker that
/// calls this on every object's primitive block.
///
/// NOT WIRED: no caller. The engine's colour grading is per render node
/// (`crate::fade::ColorGrade` / `crate::fade::SceneTintRamp`, a multiply applied
/// at draw time), not a destructive rewrite of the mesh's own colour words, and
/// it has no equivalent of the `DAT_8007C018` resident-object table. Wiring it
/// needs that table plus a decision to mutate parsed TMD data in place, which is
/// a different grading model from the one the renderer already has.
pub fn shift_primitive_colours(
    groups: &mut [(PrimGroupHeader, Vec<[u8; 4]>)],
    shift: &HsvShift,
    colour_count: &dyn Fn(u16) -> u8,
    codec: &mut dyn HsvCodec,
) -> usize {
    let mut touched = 0;
    for (header, prims) in groups.iter_mut() {
        if header.count == 0 {
            continue;
        }
        let per_prim = usize::from(colour_count(header.flags >> 1));
        if per_prim == 0 {
            continue;
        }
        let limit = usize::from(header.count) * per_prim;
        for prim in prims.iter_mut().take(limit) {
            let hsv = codec.to_hsv([prim[0], prim[1], prim[2]]);
            let rgb = codec.to_rgb(shift.apply(hsv.0, hsv.1, hsv.2));
            prim[0] = rgb[0];
            prim[1] = rgb[1];
            prim[2] = rgb[2];
            touched += 1;
        }
    }
    touched
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_done_linked_object_ends_the_tween_without_moving_anything() {
        let mut t = PositionTween {
            end: ElementVec {
                x: 100,
                ..Default::default()
            },
            rate: 0x100,
            ..Default::default()
        };
        assert_eq!(t.step(1, true), TweenStep::LinkedAlreadyDone);
        assert!(t.done, "0x801D5CC0 sets the element's own bit 8");
        assert_eq!(t.t, 0, "the accumulator is never touched on that path");
    }

    #[test]
    fn the_tween_snaps_to_the_end_vector_and_latches_done() {
        let end = ElementVec {
            x: 0x40,
            y: 0x20,
            z: 0x10,
            w: 0,
        };
        let mut t = PositionTween {
            start: ElementVec::default(),
            end,
            t: 0x0F00,
            rate: 0x100,
            done: false,
        };
        // 0x0F00 + 0x100 * 1 == 0x1000, which is NOT < 0x1000 -> snap.
        assert_eq!(t.step(1, false), TweenStep::Snapped { pos: end });
        assert_eq!(t.t, 0x1000);
        assert!(t.done);
    }

    #[test]
    fn the_frame_step_multiplies_the_rate() {
        let mut t = PositionTween {
            rate: 0x100,
            ..Default::default()
        };
        assert!(matches!(t.step(3, false), TweenStep::Blending { .. }));
        assert_eq!(t.t, 0x300, "rate * DAT_1F800393");
    }

    #[test]
    fn only_the_camera_is_exempt_from_the_facing_write() {
        let pos = ElementVec {
            y: 0x123,
            ..Default::default()
        };
        assert_eq!(PositionTween::linked_field_8e(pos, false), Some(-0x123));
        assert_eq!(PositionTween::linked_field_8e(pos, true), None);
    }

    #[test]
    fn the_camera_restore_needs_both_halfwords() {
        for (armed, owns, want) in [(0, 0, false), (1, 0, false), (0, 1, false), (1, 1, true)] {
            let mut e = ElementTeardown {
                restore_armed: armed,
                owns_camera: owns,
                ..Default::default()
            };
            assert_eq!(e.step(false).restore_camera, want);
        }
    }

    #[test]
    fn the_flag_clear_waits_for_the_linked_done_bit() {
        let mut e = ElementTeardown {
            owns_camera: 1,
            flag_mask: 0x40,
            ..Default::default()
        };
        let idle = e.step(false);
        assert!(!idle.clear_target_flags && !idle.clear_camera_flags);
        assert!(!e.done);

        let fire = e.step(true);
        assert!(fire.clear_target_flags && fire.clear_camera_flags);
        assert!(e.done);
    }

    #[test]
    fn the_camera_flag_clear_is_gated_but_the_target_clear_is_not() {
        let mut e = ElementTeardown {
            flag_mask: 0x40,
            ..Default::default()
        };
        let fire = e.step(true);
        assert!(fire.clear_target_flags);
        assert!(!fire.clear_camera_flags, "+0x50 == 0 skips 0x801D5DE8");
    }

    #[test]
    fn the_master_gate_makes_the_emitter_a_no_op() {
        let e = AmbientEmitter::default();
        let scene = AmbientScene::default();
        let mut calls = 0;
        let out = e.step(&scene, || {
            calls += 1;
            0
        });
        assert!(out.is_empty());
        assert_eq!(calls, 0, "_DAT_8007B854 is read before anything else");
    }

    #[test]
    fn the_point_arm_spawns_one_particle_on_a_sixteenth_of_frames() {
        let e = AmbientEmitter {
            state: 0,
            actor_x: (0x400_u16 as i16) as u16,
            actor_y: ((-0x400_i16) as u16),
        };
        let scene = AmbientScene {
            enabled: true,
            ..Default::default()
        };

        // First draw non-zero low nibble -> nothing, one RNG call consumed.
        let mut n = 0;
        let out = e.step(&scene, || {
            n += 1;
            1
        });
        assert!(out.is_empty());
        assert_eq!(n, 1);

        let mut seq = [0u32, 0, 0].into_iter();
        let out = e.step(&scene, || seq.next().unwrap());
        assert_eq!(
            out,
            vec![AmbientParticle {
                x: 8,
                y: -8,
                vx: -16,
                vy: -16
            }]
        );
    }

    #[test]
    fn the_scene_arm_always_bursts_at_least_one_particle() {
        let e = AmbientEmitter {
            state: 1,
            ..Default::default()
        };
        let scene = AmbientScene {
            enabled: true,
            dense: false,
            span: SceneSpan {
                x_min: 0,
                y_min: 0,
                x_max: 32,
                y_max: 32,
            },
            camera_x: 0,
            camera_y: 0,
        };
        // All zeros: every draw fires, and `(0 & 3) + 1` is one particle.
        let out = e.step(&scene, || 0);
        assert_eq!(out.len(), AMBIENT_SCENE_DRAWS);
        assert!(
            out.iter().all(|p| p.vx == -0xF && p.vy == -0xF),
            "(rand & 0x1F) - 0xF"
        );
    }

    #[test]
    fn the_flash_element_descriptor_is_inside_the_spawn_descriptor_family() {
        assert!((0x8007_05FC..=0x8007_0763).contains(&FLASH_ELEMENT_DESCRIPTOR));
        assert_ne!(
            FLASH_ELEMENT_DESCRIPTOR, 0x8007_078C,
            "not a mode-table row"
        );
        assert_eq!(FLASH_ELEMENT_FIELD_5C, 1);
        assert_eq!(FLASH_ELEMENT_POOL, 0x8007_C34C);
        assert_eq!(
            flash_element_spawn(),
            FlashElementSpawn {
                descriptor: FLASH_ELEMENT_DESCRIPTOR,
                pool: FLASH_ELEMENT_POOL,
                field_5c: FLASH_ELEMENT_FIELD_5C,
            }
        );
    }

    // --- FUN_801D27E0 -----------------------------------------------------

    fn swap_world(leader: u8) -> LeaderSwapWorld {
        LeaderSwapWorld {
            leader,
            request_byte: LEADER_SWAP_REQUEST_BIT,
            ..Default::default()
        }
    }

    /// Two of three characters are in the party: slots 0 and 2.
    fn two_present(base: u16) -> impl FnMut(u16) -> bool {
        move |f| f == LEADER_SWAP_ENABLE_FLAG || f == base || f == base + 2
    }

    #[test]
    fn the_swap_caches_the_poses_before_any_gate_can_stop_it() {
        let mut c = LeaderSwap {
            flag_base: 0x40,
            ..Default::default()
        };
        let w = LeaderSwapWorld {
            suppress_a: 1,
            ..swap_world(0)
        };
        let tick = c.step(&w, 1, |_| true);
        assert_eq!(tick.effects, vec![LeaderSwapEffect::CachePartyPoses]);
        assert_eq!(c.phase, 0);
    }

    #[test]
    fn a_clear_enable_flag_retires_the_controller() {
        let mut c = LeaderSwap {
            flag_base: 0x40,
            ..Default::default()
        };
        c.step(&swap_world(0), 1, |_| false);
        assert_eq!(c.phase, 5);
        let tick = c.step(&swap_world(0), 1, |_| false);
        assert_eq!(tick.effects, vec![LeaderSwapEffect::RetireController]);
        assert_eq!(c.phase, 5, "state 5 does not advance");
    }

    #[test]
    fn a_full_party_has_nothing_to_swap_to() {
        let mut c = LeaderSwap {
            flag_base: 0x40,
            ..Default::default()
        };
        let tick = c.step(&swap_world(0), 1, |_| true);
        assert_eq!(tick.effects, vec![LeaderSwapEffect::CachePartyPoses]);
        assert_eq!(c.phase, 0);
    }

    #[test]
    fn two_present_needs_the_leader_flag_as_well() {
        let mut c = LeaderSwap {
            flag_base: 0x40,
            ..Default::default()
        };
        // Leader is slot 1, whose flag is clear -> the extra test fails.
        let tick = c.step(&swap_world(1), 1, two_present(0x40));
        assert_eq!(tick.effects, vec![LeaderSwapEffect::CachePartyPoses]);
        assert_eq!(c.phase, 0);

        // Leader is slot 0, whose flag is set.
        let mut c = LeaderSwap {
            flag_base: 0x40,
            ..Default::default()
        };
        let tick = c.step(&swap_world(0), 1, two_present(0x40));
        assert!(tick.effects.contains(&LeaderSwapEffect::SpawnFadeOut));
        assert_eq!(c.phase, 1);
    }

    #[test]
    fn the_camera_busy_bit_reroutes_the_arm_test_to_the_flags() {
        // Request byte clear, so the fallback path would refuse.
        let mut c = LeaderSwap {
            flag_base: 0x40,
            ..Default::default()
        };
        let w = LeaderSwapWorld {
            request_byte: 0,
            camera_flags: CAMERA_BUSY_BIT,
            ..swap_world(0)
        };
        assert!(
            c.step(&w, 1, two_present(0x40))
                .effects
                .contains(&LeaderSwapEffect::SpawnFadeOut)
        );

        // The pad bit does the same job.
        let mut c = LeaderSwap {
            flag_base: 0x40,
            ..Default::default()
        };
        let w = LeaderSwapWorld {
            request_byte: 0,
            pad: LEADER_SWAP_PAD_BIT,
            ..swap_world(0)
        };
        assert!(
            c.step(&w, 1, two_present(0x40))
                .effects
                .contains(&LeaderSwapEffect::SpawnFadeOut)
        );

        // Neither set and the request byte clear: nothing happens.
        let mut c = LeaderSwap {
            flag_base: 0x40,
            ..Default::default()
        };
        let w = LeaderSwapWorld {
            request_byte: 0,
            ..swap_world(0)
        };
        assert_eq!(c.step(&w, 1, two_present(0x40)).effects.len(), 1);
    }

    #[test]
    fn the_two_hold_states_wait_the_fade_out() {
        let mut c = LeaderSwap {
            phase: 1,
            flag_base: 0x40,
            ..Default::default()
        };
        for _ in 0..0x1F {
            assert!(c.step(&swap_world(0), 1, |_| true).effects.is_empty());
            assert_eq!(c.phase, 1);
        }
        c.step(&swap_world(0), 1, |_| true);
        assert_eq!(c.phase, 2);
        assert_eq!(c.counter, 0, "the counter resets on the transition");
    }

    #[test]
    fn the_swap_picks_the_next_absent_slot_and_wraps() {
        let mut c = LeaderSwap {
            phase: 2,
            flag_base: 0x40,
            ..Default::default()
        };
        // Slots 0 and 2 present, leader 0 -> the search stops on slot 1.
        let tick = c.step(&swap_world(0), 1, two_present(0x40));
        assert_eq!(tick.new_leader, Some(1));
        assert_eq!(
            tick.effects,
            vec![
                LeaderSwapEffect::StoreOutgoingPose { slot: 0 },
                LeaderSwapEffect::CommitLeader { slot: 1 },
                LeaderSwapEffect::RefreshParty,
                LeaderSwapEffect::RecentreCamera { slot: 1 },
                LeaderSwapEffect::ClearIncomingPose { slot: 1 },
                LeaderSwapEffect::SpawnFadeIn,
            ]
        );
        assert_eq!(c.phase, 3);

        // From leader 2 the search wraps through 0 (present) onto 1.
        let mut c = LeaderSwap {
            phase: 2,
            flag_base: 0x40,
            ..Default::default()
        };
        assert_eq!(
            c.step(&swap_world(2), 1, two_present(0x40)).new_leader,
            Some(1)
        );
    }

    #[test]
    fn the_tail_states_release_the_fade_and_clear_the_busy_bit() {
        let mut c = LeaderSwap {
            phase: 3,
            flag_base: 0x40,
            ..Default::default()
        };
        assert_eq!(
            c.step(&swap_world(0), 1, |_| true).effects,
            vec![LeaderSwapEffect::ReleaseFadeObject]
        );
        assert_eq!(c.phase, 4);
        for _ in 0..0x1F {
            assert!(c.step(&swap_world(0), 1, |_| true).effects.is_empty());
        }
        let tick = c.step(&swap_world(0), 1, |_| true);
        assert_eq!(tick.effects, vec![LeaderSwapEffect::ClearCameraBusy]);
        assert_eq!(c.phase, 0);
    }

    #[test]
    fn an_out_of_range_phase_runs_no_body() {
        let mut c = LeaderSwap {
            phase: 9,
            ..Default::default()
        };
        assert!(c.step(&swap_world(0), 1, |_| true).effects.is_empty());
        assert_eq!(c.phase, 9);
    }

    // --- FUN_801D5E20 -----------------------------------------------------

    struct Codec;
    impl HsvCodec for Codec {
        fn to_hsv(&mut self, rgb: [u8; 3]) -> (i32, i32, i32) {
            (i32::from(rgb[0]), i32::from(rgb[1]), i32::from(rgb[2]))
        }
        fn to_rgb(&mut self, hsv: (i32, i32, i32)) -> [u8; 3] {
            [hsv.0 as u8, hsv.1 as u8, hsv.2 as u8]
        }
    }

    #[test]
    fn the_hue_wrap_is_359_not_360() {
        let s = HsvShift {
            hue: 1,
            ..Default::default()
        };
        assert_eq!(s.apply(HUE_WRAP, 0, 0).0, 1, "0x168 folds to 1, not 0");
        assert_eq!(s.apply(HUE_WRAP - 1, 0, 0).0, HUE_WRAP);
        let back = HsvShift {
            hue: -1,
            ..Default::default()
        };
        assert_eq!(back.apply(0, 0, 0).0, HUE_WRAP - 1);
    }

    #[test]
    fn saturation_and_value_clamp_to_the_byte_range() {
        let s = HsvShift {
            hue: 0,
            saturation: 0x40,
            value: -0x40,
        };
        assert_eq!(s.apply(0, 0xF0, 0x10), (0, CHANNEL_MAX, 0));
    }

    #[test]
    fn a_zero_colour_count_skips_a_whole_group() {
        let mut groups = vec![
            (
                PrimGroupHeader {
                    count: 2,
                    flags: 4,
                    ilen: 3,
                },
                vec![[1u8, 2, 3, 4]; 4],
            ),
            (
                PrimGroupHeader {
                    count: 2,
                    flags: 8,
                    ilen: 3,
                },
                vec![[1u8, 2, 3, 4]; 4],
            ),
        ];
        let shift = HsvShift {
            hue: 10,
            ..Default::default()
        };
        // flags 4 -> index 2 -> one colour; flags 8 -> index 4 -> none.
        let n = shift_primitive_colours(
            &mut groups,
            &shift,
            &|idx| if idx == 2 { 1 } else { 0 },
            &mut Codec,
        );
        assert_eq!(n, 2, "count * per_prim of the first group only");
        assert_eq!(groups[0].1[0][0], 11);
        assert_eq!(groups[0].1[2], [1, 2, 3, 4], "past count * per_prim");
        assert_eq!(groups[1].1[0], [1, 2, 3, 4]);
    }

    #[test]
    fn the_fourth_byte_of_a_colour_word_is_untouched() {
        let mut groups = vec![(
            PrimGroupHeader {
                count: 1,
                flags: 2,
                ilen: 2,
            },
            vec![[0x10u8, 0x20, 0x30, 0xAB]],
        )];
        shift_primitive_colours(
            &mut groups,
            &HsvShift {
                hue: 1,
                saturation: 1,
                value: 1,
            },
            &|_| 1,
            &mut Codec,
        );
        assert_eq!(groups[0].1[0], [0x11, 0x21, 0x31, 0xAB]);
    }

    #[test]
    fn the_group_stride_over_advances_by_one_primitive() {
        let h = PrimGroupHeader {
            count: 4,
            flags: 0,
            ilen: 3,
        };
        // The documented body is 4 * 12 == 48 bytes plus the 8-byte header.
        assert_eq!(colour_walk_group_stride(&h), 8 + 5 * 12);
        // Even an empty group advances one primitive's worth.
        let empty = PrimGroupHeader { count: 0, ..h };
        assert_eq!(colour_walk_group_stride(&empty), 8 + 12);
    }

    #[test]
    fn the_density_bit_swaps_both_profile_constants() {
        assert_eq!(AmbientProfile::select(false), AmbientProfile::LIGHT);
        assert_eq!(AmbientProfile::select(true), AmbientProfile::HEAVY);
        assert_eq!(
            (
                AmbientProfile::HEAVY.span_bias,
                AmbientProfile::HEAVY.y_offset
            ),
            (6, 0x0E)
        );
    }
}
