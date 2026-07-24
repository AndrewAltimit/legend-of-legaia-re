//! Script-cutscene *element* handlers: the per-frame actor bodies a cutscene
//! script installs at `actor+0x0C` alongside the camera controller.
//!
//! Each element is an ordinary field actor whose handler runs once per frame
//! and drives some **other** object - the "linked object" pointer at `+0x90`.
//! The three handlers ported here are the position tween, the teardown, and
//! the ambient particle emitter.
//!
//! PORT: FUN_801D5C08 - per-frame position tween step.
//! PORT: FUN_801D5D60 - scripted-element teardown.
//! PORT: FUN_801D6058 - ambient particle emitter.
//! PORT: FUN_801D841C - the fade/flash element spawn.
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
