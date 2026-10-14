//! The field overlay's ambient particle emitter [`AmbientEmitter`]
//! (`FUN_801D6058`): scene ambience, not a cutscene element.
//!
//! The `PORT` tag lives on the item that implements the address, so the
//! liveness audit sees one anchor per port site.
//!
//! Its descriptor is `0x801F271C` in the field overlay's own plain-template
//! table - the `0x18`-byte
//! `[u32 0][u16 0][u16 0xFFFF][u32 handler][u32 0][u32 0][u32 0]` shape the
//! floor-ladder oscillator (`0x801F27EC`), the eased move (`0x801F2840`) and
//! the shutter bars (`0x801F2858`) also use - and its one spawn site is
//! `0x801D6FD8`, inside the field overlay's MAIN INIT `FUN_801D6704`, behind a
//! `bnez` on `_DAT_8007B8B8`. `legaia_engine_core::world::cutscene_elements` is its seat:
//! `World::install_field_scene_elements` spawns it on every cold field entry
//! and `World::tick_cutscene_elements` runs it each frame on both hosts.
//!
//! REF: FUN_801D629C - the particle spawn primitive the emitter calls.
//!
//! Read off `ghidra/scripts/funcs/overlay_cutscene_dialogue_801d6058.txt` -
//! disassembly, not the C.

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
    /// The collecting wrapper; the port tag sits on [`Self::step_with`].
    ///
    /// WIRED: `legaia_engine_core::world::World::tick_cutscene_elements` runs this every
    /// frame on both hosts, off the same master-driver gate the other
    /// plain-template families ride, and
    /// `legaia_engine_core::world::World::install_field_scene_elements` installs the one
    /// element it runs on at every cold field entry (descriptor `0x801F271C`,
    /// spawn site `0x801D6FD8`). Each particle it names goes to the fog
    /// spawner `crate::fog_particles::FogPool::spawn` (`FUN_801D629C`),
    /// which is what retail's `jal 0x801d629c` at `0x801D6148` /
    /// `0x801D6248` reach.
    pub fn step(
        &self,
        scene: &AmbientScene,
        mut rand: impl FnMut() -> u32,
    ) -> Vec<AmbientParticle> {
        let mut out = Vec::new();
        self.step_with(scene, &mut rand, |p, _| out.push(p));
        out
    }

    /// [`Self::step`] with the spawn call in place: `sink` runs where retail
    /// calls `FUN_801D629C`, **between** the emitter's own random draws, and
    /// is handed the same `rand` so the two routines interleave on one stream the way retail's single `FUN_80056798` does.
    /// PORT: FUN_801D6058 (the body the world tick runs)
    pub fn step_with(
        &self,
        scene: &AmbientScene,
        rand: &mut dyn FnMut() -> u32,
        mut sink: impl FnMut(AmbientParticle, &mut dyn FnMut() -> u32),
    ) {
        if !scene.enabled {
            return;
        }
        let profile = AmbientProfile::select(scene.dense);

        if self.state == 0 {
            if rand() & 0xF != 0 {
                return;
            }
            let (x, y) = self.point_origin();
            let vx = ((rand() & 0xF) as i32 - 8) * 2;
            let vy = ((rand() & 0xF) as i32 - 8) * 2;
            sink(AmbientParticle { x, y, vx, vy }, rand);
            return;
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
                sink(AmbientParticle { x, y, vx, vy }, rand);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
