//! Three of the field-to-battle transition's five per-frame style emitters:
//! the two particle-field ticks and the screen-strip curtain.
//!
//! | Retail | Here | Style |
//! |---|---|---|
//! | `FUN_801CFDA0` | [`tick_particle_field`] with [`PARTICLE_TICK_A`] | scatter, `>> 1` velocity |
//! | `FUN_801D0370` | [`tick_particle_field`] with [`PARTICLE_TICK_B`] | scatter with spin-up and colour decay |
//! | `FUN_801D11D0` | [`tick_curtain`] | the screen sliced into rows and columns and stretched apart |
//!
//! The remaining two live in [`crate::battle_intro_tiles`] and
//! [`crate::battle_intro_swirl`].
//!
//! ## The two particle ticks share a record layout, and it is not the one the
//! seeders' field names suggest
//!
//! Both walk [`PARTICLE_TICK_COUNT`] records of
//! [`crate::battle_intro_particles::PARTICLE_STRIDE`] bytes out of the entity's
//! `+0x48` block - the block
//! [`crate::battle_intro_particles::seed_particle_grid`] fills - and both read
//! it the same way:
//!
//! | offset | the tick's use |
//! |---|---|
//! | `+0x04` | packed colour; bits `31..24` non-zero **skips the particle entirely** |
//! | `+0x08` / `+0x0A` / `+0x0C` | translation vector, integrated by `+0x20` / `+0x22` / `+0x24` |
//! | `+0x10` / `+0x12` / `+0x14` | rotation vector, integrated by `+0x18` / `+0x1A` / `+0x1C` |
//! | `+0x1E` | spawn delay, held against the scaled entity clock |
//! | `+0x28` | high bits pick the texture page, low six the u |
//! | `+0x2A` | the v |
//!
//! That is what pins the seeder's `+0x20` / `+0x22` / `+0x24` triple as a
//! **translation velocity** rather than as a sprite size and a flag word. See
//! the note on [`crate::battle_intro_particles::IntroParticle`].
//!
//! ## `_DAT_8007B6CC` is written twice in `FUN_801D11D0` and the second wins
//!
//! `801d11fc`..`801d1224` computes `elapsed != 0` into the flag through a
//! two-arm branch and then stores **zero** over it unconditionally at
//! `801d1224`, on the merge point both arms reach. The curtain therefore always
//! reports "first frame", unlike the other four styles. The dead pair is
//! retail's; [`CurtainTick`] carries only the surviving write.
//!
//! Provenance: `see ghidra/scripts/funcs/overlay_field_battle_intro_801cfda0.txt`,
//! `..._801d0370.txt` and `..._801d11d0.txt` - disassembly, not the C.

use crate::battle_intro_particles::IntroParticle;
use crate::battle_intro_transition::{
    IntroQuad, IntroQuadAnchor, IntroQuadDesc, IntroQuadRequest, build_intro_quad,
};

// ---------------------------------------------------------------------------
// FUN_801CFDA0 / FUN_801D0370 - the two particle-field ticks
// ---------------------------------------------------------------------------

/// Records both ticks walk (`slti v0,s5,0x488`).
///
/// It is **not** the seeders' 1280: the grid is `0x500` records and the ticks
/// visit `0x488` of them, so the last 120 are seeded and never drawn. Both
/// ticks agree on the bound, so it is a property of the style, not a slip.
pub const PARTICLE_TICK_COUNT: usize = 0x488;

/// Screen-space accept window applied to the projected corner `0`, exclusive at
/// both ends (`-8 < x < 0x148`, `-8 < y < 0xF8`).
pub const SCREEN_MIN: i16 = -8;
/// See [`SCREEN_MIN`].
pub const SCREEN_MAX_X: i16 = 0x148;
/// See [`SCREEN_MIN`].
pub const SCREEN_MAX_Y: i16 = 0xF8;

/// The `+0x28` field's high bits are shifted down by six and biased by this to
/// form the texture-page word (`sra v0,v0,0x16; addiu v0,v0,0x135`).
pub const PARTICLE_TPAGE_BIAS: i16 = 0x135;

/// The colour delta `FUN_801D0370` applies to a moving particle's `+0x04`
/// every frame (`lui 0xfffa; ori 0xfafb`, i.e. `-0x50505`): five off each of
/// the three channels, so a particle fades to black as it flies.
pub const PARTICLE_COLOUR_DECAY: i32 = -0x0005_0505;

/// The constants that separate the two particle ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParticleTickStyle {
    /// Half-extent of the quad the projector is handed. `FUN_801CFDA0` builds
    /// a `0x100` unit sprite, `FUN_801D0370` a `0x40` one.
    pub quad_size: i16,
    /// Multiplier on the entity clock before it is compared with a particle's
    /// `+0x1E` delay: `0x6E` for `FUN_801CFDA0`, `0x40` for `FUN_801D0370`.
    pub delay_scale: i32,
    /// Right shift on the rotation integration: `1` for `FUN_801CFDA0`, `4`
    /// for `FUN_801D0370` (whose z component uses `3` instead - see
    /// [`ParticleTickStyle::rot_z_shift`]).
    pub rot_shift: u32,
    /// Right shift on the rotation vector's **z** component only.
    pub rot_z_shift: u32,
    /// Whether the tick pre-divides the rotation vector by 8 before handing it
    /// to `RotMatrix`. `FUN_801D0370` does (`sra ...,0x13` on a sign-extended
    /// halfword); `FUN_801CFDA0` passes `+0x10` straight through.
    pub rot_prescale_shift: u32,
    /// Whether a moving particle's spin accelerates by `+= (v >> 3) + (v >> 2)`
    /// - a `1.375x` per frame ramp. Only `FUN_801D0370` does this.
    pub spin_up: bool,
    /// Whether a moving particle's colour decays by [`PARTICLE_COLOUR_DECAY`].
    pub colour_decay: bool,
    /// Whether the tick writes `1` into the particle's `+0x16` when it moves.
    /// Only `FUN_801CFDA0` does.
    pub stamp_field_16: bool,
    /// Whether the projected depth has to fall inside `0x81..=0x3FDB`
    /// (`FUN_801CFDA0`'s `d - 0x81 <u 0x3F5B`) or merely exceed `0x80`
    /// (`FUN_801D0370`'s `slti 0x81`).
    pub bounded_depth: bool,
    /// Whether a particle that moved this frame links one OT bucket nearer
    /// (`s6 = -1`, used as `OT + 400 + s6 * 4`). Only `FUN_801D0370` does; the
    /// other always links at `OT + 400`.
    pub moved_links_nearer: bool,
}

/// `FUN_801CFDA0`'s constants.
pub const PARTICLE_TICK_A: ParticleTickStyle = ParticleTickStyle {
    quad_size: 0x100,
    delay_scale: 0x6E,
    rot_shift: 1,
    rot_z_shift: 1,
    rot_prescale_shift: 0,
    spin_up: false,
    colour_decay: false,
    stamp_field_16: true,
    bounded_depth: true,
    moved_links_nearer: false,
};

/// `FUN_801D0370`'s constants.
pub const PARTICLE_TICK_B: ParticleTickStyle = ParticleTickStyle {
    quad_size: 0x40,
    delay_scale: 0x40,
    rot_shift: 4,
    rot_z_shift: 3,
    rot_prescale_shift: 3,
    spin_up: true,
    colour_decay: true,
    stamp_field_16: false,
    bounded_depth: false,
    moved_links_nearer: true,
};

/// One particle's outcome for a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParticleStep {
    /// `+0x04`'s top byte was non-zero, so retail skipped the particle before
    /// the projection. Nothing was integrated and nothing is drawn.
    Masked,
    /// The particle is live. `moved` is the `+0x1E < scaled_clock` gate.
    Live {
        /// The delay gate let the integration run.
        moved: bool,
        /// The rotation vector handed to `RotMatrix`, after the style's
        /// optional pre-divide.
        rot: (i16, i16, i16),
        /// Texture-page word, from `+0x28`.
        tpage: i16,
        /// Top-left texel, `(+0x28 & 0x3F, +0x2A)`. `+0x2A` is a halfword the
        /// packet build stores as a byte (`sb`), so only its low eight bits
        /// reach the GPU. The quad's other three corners are this pair with
        /// `+8` on one or both axes.
        texel: (u8, u8),
    },
}

/// What one [`tick_particle_field`] frame reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ParticleFieldTick {
    /// `_DAT_8007B6CC` - `elapsed != 0` on entry.
    pub not_first_frame: bool,
    /// `FUN_801CFDA0` washes the screen with `0x101010` on every frame after
    /// the first; `FUN_801D0370` never does.
    pub late_wash: bool,
    /// Particles the top-byte mask skipped.
    pub masked: usize,
    /// Particles whose delay had expired.
    pub moved: usize,
}

fn shr_toward_zero(v: i32, bits: u32) -> i32 {
    let bias = (1i32 << bits) - 1;
    if v < 0 { v + bias } else { v }.wrapping_shr(bits)
}

/// One particle, one frame - the body both `FUN_801CFDA0` and `FUN_801D0370`
/// run inside their `0x488` loop.
///
/// PORT: FUN_801CFDA0
/// PORT: FUN_801D0370
///
/// NOT WIRED: called only by [`tick_particle_field`], which is itself inert -
/// see the tag there.
pub fn step_particle(
    p: &mut IntroParticle,
    style: &ParticleTickStyle,
    frame_step: u8,
    scaled_clock: i32,
) -> ParticleStep {
    if (p.tint & 0xFF00_0000) != 0 {
        return ParticleStep::Masked;
    }
    let step = i32::from(frame_step);
    let moved = i32::from(p.delay) < scaled_clock;
    if moved {
        if style.colour_decay {
            p.tint = (p.tint as i32).wrapping_add(PARTICLE_COLOUR_DECAY) as u32;
        }
        if style.spin_up {
            let ramp = |v: i16| -> i16 {
                let v32 = i32::from(v);
                (v32 + (v32 >> 3) + (v32 >> 2)) as i16
            };
            p.spin = (ramp(p.spin.0), ramp(p.spin.1), ramp(p.spin.2));
        }
        let add = |acc: i16, v: i16, bits: u32| -> i16 {
            (acc as u16).wrapping_add(shr_toward_zero(i32::from(v) * step, bits) as u16) as i16
        };
        p.rot = (
            add(p.rot.0, p.spin.0, style.rot_shift),
            add(p.rot.1, p.spin.1, style.rot_shift),
            add(p.rot.2, p.spin.2, style.rot_z_shift),
        );
        if style.stamp_field_16 {
            p.field_16 = 1;
        }
        // The translation always integrates with a `>> 1`, in both ticks.
        p.trans = (
            add(p.trans.0, p.trans_vel.0, 1),
            add(p.trans.1, p.trans_vel.1, 1),
            add(p.trans.2, p.trans_vel.2, 1),
        );
    }
    let rot = if style.rot_prescale_shift == 0 {
        p.rot
    } else {
        let s = style.rot_prescale_shift;
        (p.rot.0 >> s, p.rot.1 >> s, p.rot.2 >> s)
    };
    ParticleStep::Live {
        moved,
        rot,
        tpage: (p.texel_page >> 6).wrapping_add(PARTICLE_TPAGE_BIAS),
        texel: ((p.texel_page as u16 & 0x3F) as u8, p.texel_v as u8),
    }
}

/// Whether a projected particle quad survives both accept tests.
///
/// `depth` is `FUN_8005BAC8`'s return; `corner0` is the first projected `SXY`.
/// `FUN_801CFDA0` bounds the depth at both ends, `FUN_801D0370` only below -
/// so the scatter style culls far particles and the spin-up style does not.
pub fn particle_quad_accepted(style: &ParticleTickStyle, depth: i32, corner0: (i16, i16)) -> bool {
    let depth_ok = if style.bounded_depth {
        (depth.wrapping_sub(0x81) as u32) < 0x3F5B
    } else {
        depth > 0x80
    };
    depth_ok
        && corner0.0 > SCREEN_MIN
        && corner0.0 < SCREEN_MAX_X
        && corner0.1 > SCREEN_MIN
        && corner0.1 < SCREEN_MAX_Y
}

/// One frame of a particle-field style. `FUN_801CFDA0` / `FUN_801D0370`.
///
/// The delay gate is the entity clock times [`ParticleTickStyle::delay_scale`],
/// computed once before the loop; the clock then advances by the frame step.
///
/// PORT: FUN_801CFDA0
/// PORT: FUN_801D0370
///
/// NOT WIRED: `legaia_engine_core::World::battle_intro` is the transition's
/// phase counter and nothing else - it owns no particle block, and
/// `legaia-engine-render` has no pass that projects 1160 sprite quads through
/// the GTE into an ordering table. Both halves have to exist before ticking a
/// field that nothing draws is worth the cost.
pub fn tick_particle_field(
    particles: &mut [IntroParticle],
    style: &ParticleTickStyle,
    elapsed: &mut i16,
    frame_step: u8,
) -> ParticleFieldTick {
    let mut out = ParticleFieldTick {
        not_first_frame: *elapsed != 0,
        late_wash: style.stamp_field_16 && *elapsed != 0,
        ..Default::default()
    };
    let scaled_clock = i32::from(*elapsed) * style.delay_scale;
    for p in particles.iter_mut().take(PARTICLE_TICK_COUNT) {
        match step_particle(p, style, frame_step, scaled_clock) {
            ParticleStep::Masked => out.masked += 1,
            ParticleStep::Live { moved, .. } => out.moved += usize::from(moved),
        }
    }
    *elapsed = (*elapsed as u16).wrapping_add(u16::from(frame_step)) as i16;
    out
}

// ---------------------------------------------------------------------------
// FUN_801D11D0 - the curtain
// ---------------------------------------------------------------------------

/// Screen rows the curtain slices (`slti v0,s3,0xf0`).
pub const CURTAIN_ROWS: i32 = 0xF0;
/// Screen columns the curtain slices (`slti v0,s2,0x140`).
pub const CURTAIN_COLS: i32 = 0x140;
/// Vertical centre the row warp pivots about (`addiu v1,s3,-0x78`).
pub const CURTAIN_ROW_CENTRE: i32 = 0x78;
/// Horizontal centre the column warp pivots about (`addiu v0,s2,-0xa0`).
pub const CURTAIN_COL_CENTRE: i32 = 0xA0;
/// Divisor and clock bias of both warps (`addiu v1,a1,0x1c`, then `/ 0x1c`).
pub const CURTAIN_WARP_DIVISOR: i32 = 0x1C;

/// Descriptor-table index the row pass patches and draws (`param_4 == 3`).
pub const CURTAIN_ROW_DESC: usize = 3;
/// Descriptor-table index the column pass patches and draws (`param_4 == 2`).
pub const CURTAIN_COL_DESC: usize = 2;

/// Width of the row pass' left strip, and the x its right strip starts at.
pub const CURTAIN_LEFT_W: u8 = 0xC0;
/// Width of the row pass' right strip. `0xC0 + 0x80 == 0x140`, the whole
/// screen.
pub const CURTAIN_RIGHT_W: u8 = 0x80;

/// Texture page the row pass' left strip samples (`li v0,0x105`).
pub const CURTAIN_ROW_TPAGE_LEFT: u16 = 0x105;
/// Texture page the row pass' right strip samples (`li v0,0x108`).
pub const CURTAIN_ROW_TPAGE_RIGHT: u16 = 0x108;
/// Texture page the column pass samples for columns `< 0xC0` (`li v0,0x115`).
pub const CURTAIN_COL_TPAGE_LEFT: u16 = 0x115;
/// Texture page the column pass samples for columns `>= 0xC0` (`li v0,0x118`),
/// whose u is additionally biased by `0x40`.
pub const CURTAIN_COL_TPAGE_RIGHT: u16 = 0x118;
/// Column at which the column pass swaps page and biases u.
pub const CURTAIN_COL_SPLIT: i32 = 0xC0;

/// OT depth the row pass writes into `DAT_801D245C` before every quad.
pub const CURTAIN_ROW_OT_DEPTH: u32 = 0x12C;
/// OT depth the column pass writes.
pub const CURTAIN_COL_OT_DEPTH: u32 = 0x1C2;

/// Colour intensity both passes pass as `param_5`.
pub const CURTAIN_INTENSITY: i32 = 0x80;
/// x offset added to a column's warped position before it is drawn
/// (`addiu a1,a1,0x1e0`), while the visibility test uses `+ 0xA0`.
pub const CURTAIN_COL_DRAW_BIAS: i32 = 0x1E0;

/// The screen wash the curtain opens with (`func_0x8004695C(0x80808)`).
pub const CURTAIN_WASH_RGB: u32 = 0x0008_0808;

/// One quad the curtain emitted, with the request that produced it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CurtainQuad {
    /// Descriptor-table index this quad came from.
    pub desc_index: usize,
    /// The arguments `FUN_801CF1B0` was called with.
    pub request: IntroQuadRequest,
    /// The built primitive.
    pub quad: IntroQuad,
}

/// What one curtain frame emitted.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CurtainTick {
    /// `_DAT_8007B6CC`, which this style always clears - see the module docs.
    pub not_first_frame: bool,
    /// The quads, in retail emission order: `2 * `[`CURTAIN_ROWS`] row strips
    /// first, then the visible columns.
    pub quads: Vec<CurtainQuad>,
    /// Columns the visibility test rejected.
    pub culled_columns: usize,
}

/// The signed `/ 0x1C` both warps use, truncating toward zero.
fn warp(offset: i32, elapsed: i32) -> i32 {
    (offset * (elapsed + CURTAIN_WARP_DIVISOR)) / CURTAIN_WARP_DIVISOR
}

/// One frame of the curtain style. `FUN_801D11D0`.
///
/// Two passes over the descriptor table at overlay VA `0x801D1EC4`, each of
/// which **patches the descriptor in place** before every call rather than
/// carrying a per-strip record:
///
/// * **Rows.** For each of [`CURTAIN_ROWS`] scanlines, descriptor
///   [`CURTAIN_ROW_DESC`] gets `v0 = row`, `u0 = 0`, and a `(width, tpage)`
///   pair per half; the strip is drawn at
///   `y = (row - 120) * (elapsed + 28) / 28 + 120`, a vertical stretch about
///   the screen centre that opens as the clock runs.
/// * **Columns.** For each of [`CURTAIN_COLS`], descriptor
///   [`CURTAIN_COL_DESC`] gets `u0 = col` (`+ 0x40` past
///   [`CURTAIN_COL_SPLIT`]) and its page; the strip is drawn only when its
///   warped x still falls inside the screen, stretched vertically by
///   `(|col - 160| * elapsed) >> 5` and lifted by `120/4096` of that.
///
/// Retail also emits four rectangle primitives around the two passes and calls
/// `FUN_801D1D9C(0x1EA, 2, 0x808080)` between them. Those are draw-list work
/// with no state, and stay with the renderer.
///
/// PORT: FUN_801D11D0
/// REF: FUN_801CF1B0 (the quad builder), FUN_801D1D9C (the mid-pass emitter)
///
/// NOT WIRED: the descriptor table lives inside PROT 0979, which the engine
/// never loads, and the strips texture a captured field framebuffer the engine
/// does not produce. This is nonetheless the function that gives
/// [`build_intro_quad`] its retail caller: everything below the table read is
/// ported, and a host that supplies the parsed table gets the whole style.
pub fn tick_curtain(table: &mut [IntroQuadDesc], elapsed: &mut i16, frame_step: u8) -> CurtainTick {
    let mut out = CurtainTick::default();
    let clock = i32::from(*elapsed);

    for row in 0..CURTAIN_ROWS {
        let y = warp(row - CURTAIN_ROW_CENTRE, clock) + CURTAIN_ROW_CENTRE;
        for (x, w, tpage) in [
            (0i32, CURTAIN_LEFT_W, CURTAIN_ROW_TPAGE_LEFT),
            (
                i32::from(CURTAIN_LEFT_W),
                CURTAIN_RIGHT_W,
                CURTAIN_ROW_TPAGE_RIGHT,
            ),
        ] {
            let Some(desc) = table.get_mut(CURTAIN_ROW_DESC) else {
                return out;
            };
            desc.u0 = 0;
            desc.v0 = row as u8;
            desc.w = w;
            desc.tpage = tpage;
            let request = IntroQuadRequest {
                anchor: IntroQuadAnchor::TopLeft,
                x: x as i16,
                y: y as i16,
                key: CURTAIN_ROW_DESC as i32,
                intensity: CURTAIN_INTENSITY,
                scale_x: 0x1000,
                scale_y: 0x1000,
                ot_depth: CURTAIN_ROW_OT_DEPTH,
            };
            if let Some(quad) = build_intro_quad(&request, table) {
                out.quads.push(CurtainQuad {
                    desc_index: CURTAIN_ROW_DESC,
                    request,
                    quad,
                });
            }
        }
    }

    for col in 0..CURTAIN_COLS {
        let right = col >= CURTAIN_COL_SPLIT;
        let Some(desc) = table.get_mut(CURTAIN_COL_DESC) else {
            return out;
        };
        desc.u0 = (if right { col + 0x40 } else { col }) as u8;
        desc.tpage = if right {
            CURTAIN_COL_TPAGE_RIGHT
        } else {
            CURTAIN_COL_TPAGE_LEFT
        };

        let off = col - CURTAIN_COL_CENTRE;
        let warped = warp(off, clock);
        let stretch = shr_toward_zero(off.abs() * clock, 5);
        // The visibility test re-centres on 0xA0; the draw uses 0x1E0.
        if ((warped + CURTAIN_COL_CENTRE) as u32) >= CURTAIN_COLS as u32 {
            out.culled_columns += 1;
            continue;
        }
        let lift = shr_toward_zero(stretch * 0x78, 12);
        let request = IntroQuadRequest {
            anchor: IntroQuadAnchor::TopLeft,
            x: (warped + CURTAIN_COL_DRAW_BIAS) as i16,
            y: (-lift) as i16,
            key: CURTAIN_COL_DESC as i32,
            intensity: CURTAIN_INTENSITY,
            scale_x: 0x1000,
            scale_y: stretch + 0x1000,
            ot_depth: CURTAIN_COL_OT_DEPTH,
        };
        if let Some(quad) = build_intro_quad(&request, table) {
            out.quads.push(CurtainQuad {
                desc_index: CURTAIN_COL_DESC,
                request,
                quad,
            });
        }
    }

    *elapsed = (*elapsed as u16).wrapping_add(u16::from(frame_step)) as i16;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn particle() -> IntroParticle {
        IntroParticle {
            tint: 0x0080_8080,
            trans: (0, 0, 0),
            rot: (0x100, 0x200, 0x300),
            spin: (0x10, 0x20, 0x40),
            delay: 0,
            trans_vel: (0x40, 0x40, 0x20),
            texel_page: 0x0080,
            texel_v: 4,
            field_16: 0,
        }
    }

    #[test]
    fn the_ticks_visit_fewer_records_than_the_seeders_fill() {
        assert_eq!(PARTICLE_TICK_COUNT, 0x488);
        assert_eq!(
            crate::battle_intro_particles::PARTICLE_COUNT - PARTICLE_TICK_COUNT,
            120,
            "the seeders fill 1280 and the ticks visit 1160"
        );
    }

    #[test]
    fn a_masked_particle_is_skipped_before_anything_moves() {
        let mut p = IntroParticle {
            tint: 0x0100_0000,
            ..particle()
        };
        let before = p;
        assert_eq!(
            step_particle(&mut p, &PARTICLE_TICK_A, 4, i32::MAX),
            ParticleStep::Masked
        );
        assert_eq!(p, before);
    }

    #[test]
    fn the_delay_gate_holds_a_particle_but_still_draws_it() {
        let mut p = IntroParticle {
            delay: 100,
            ..particle()
        };
        let before = p;
        let step = step_particle(&mut p, &PARTICLE_TICK_A, 1, 0);
        assert!(matches!(step, ParticleStep::Live { moved: false, .. }));
        assert_eq!(p, before, "nothing integrated");
    }

    #[test]
    fn style_a_integrates_by_one_and_stamps_field_16() {
        let mut p = particle();
        let step = step_particle(&mut p, &PARTICLE_TICK_A, 2, i32::MAX);
        assert!(matches!(step, ParticleStep::Live { moved: true, .. }));
        // rot += (spin * 2) >> 1 == spin
        assert_eq!(p.rot, (0x110, 0x220, 0x340));
        // trans += (trans_vel * 2) >> 1 == trans_vel
        assert_eq!(p.trans, (0x40, 0x40, 0x20));
        assert_eq!(p.field_16, 1);
        assert_eq!(p.tint, 0x0080_8080, "style A leaves the colour alone");
        assert_eq!(p.spin, (0x10, 0x20, 0x40), "and does not ramp the spin");
    }

    #[test]
    fn style_b_ramps_the_spin_and_decays_the_colour() {
        let mut p = particle();
        step_particle(&mut p, &PARTICLE_TICK_B, 1, i32::MAX);
        // 0x10 + (0x10 >> 3) + (0x10 >> 2) == 0x16
        assert_eq!(p.spin.0, 0x16);
        assert_eq!(
            p.tint,
            0x0080_8080u32.wrapping_add(PARTICLE_COLOUR_DECAY as u32)
        );
        assert_eq!(p.field_16, 0, "style B never stamps +0x16");
    }

    #[test]
    fn style_b_pre_divides_the_rotation_vector_it_hands_the_matrix() {
        let mut p = IntroParticle {
            spin: (0, 0, 0),
            rot: (0x800, 0x400, 0x200),
            ..particle()
        };
        let ParticleStep::Live { rot, .. } = step_particle(&mut p, &PARTICLE_TICK_B, 1, 0) else {
            panic!()
        };
        assert_eq!(rot, (0x100, 0x80, 0x40));
        assert_eq!(
            p.rot,
            (0x800, 0x400, 0x200),
            "the record itself is unscaled"
        );
    }

    #[test]
    fn the_texel_and_page_come_out_of_one_halfword() {
        let mut p = IntroParticle {
            texel_page: 0x0C25,
            texel_v: 0x30,
            ..particle()
        };
        let ParticleStep::Live { tpage, texel, .. } = step_particle(&mut p, &PARTICLE_TICK_A, 1, 0)
        else {
            panic!()
        };
        assert_eq!(tpage, (0x0C25 >> 6) + PARTICLE_TPAGE_BIAS);
        assert_eq!(texel, (0x25, 0x30));
    }

    #[test]
    fn only_style_a_bounds_the_depth_above() {
        assert!(particle_quad_accepted(&PARTICLE_TICK_A, 0x81, (0, 0)));
        assert!(!particle_quad_accepted(&PARTICLE_TICK_A, 0x80, (0, 0)));
        assert!(!particle_quad_accepted(&PARTICLE_TICK_A, 0x3FDC, (0, 0)));
        assert!(particle_quad_accepted(&PARTICLE_TICK_B, 0x3FDC, (0, 0)));
        assert!(!particle_quad_accepted(&PARTICLE_TICK_B, 0x80, (0, 0)));
    }

    #[test]
    fn the_screen_window_is_exclusive_at_both_ends() {
        assert!(!particle_quad_accepted(&PARTICLE_TICK_B, 0x100, (-8, 0)));
        assert!(particle_quad_accepted(&PARTICLE_TICK_B, 0x100, (-7, 0)));
        assert!(!particle_quad_accepted(&PARTICLE_TICK_B, 0x100, (0, 0xF8)));
        assert!(particle_quad_accepted(
            &PARTICLE_TICK_B,
            0x100,
            (0x147, 0xF7)
        ));
    }

    #[test]
    fn the_field_tick_advances_the_clock_and_counts_the_gates() {
        let mut field: Vec<IntroParticle> = (0..PARTICLE_TICK_COUNT + 8)
            .map(|i| IntroParticle {
                delay: (i % 200) as i16,
                tint: if i % 100 == 0 {
                    0xFF00_0000
                } else {
                    0x0080_8080
                },
                ..particle()
            })
            .collect();
        let mut elapsed = 2i16;
        let tick = tick_particle_field(&mut field, &PARTICLE_TICK_A, &mut elapsed, 3);
        assert!(tick.not_first_frame && tick.late_wash);
        assert_eq!(elapsed, 5);
        assert!(tick.masked > 0 && tick.moved > 0);
        // The tail past 0x488 is untouched.
        assert_eq!(field[PARTICLE_TICK_COUNT].rot, particle().rot);
    }

    fn quad_table() -> Vec<IntroQuadDesc> {
        vec![
            IntroQuadDesc {
                size_q12: 0x1000,
                w: 8,
                h: 8,
                ..Default::default()
            };
            8
        ]
    }

    #[test]
    fn the_row_pass_slices_the_whole_screen_width_in_two() {
        assert_eq!(
            i32::from(CURTAIN_LEFT_W) + i32::from(CURTAIN_RIGHT_W),
            CURTAIN_COLS
        );
        let mut table = quad_table();
        let mut elapsed = 0i16;
        let tick = tick_curtain(&mut table, &mut elapsed, 1);
        let rows: Vec<_> = tick
            .quads
            .iter()
            .filter(|q| q.desc_index == CURTAIN_ROW_DESC)
            .collect();
        assert_eq!(rows.len(), 2 * CURTAIN_ROWS as usize);
        assert_eq!(rows[0].request.x, 0);
        assert_eq!(rows[1].request.x, i16::from(CURTAIN_LEFT_W));
        assert_eq!(elapsed, 1);
        assert!(!tick.not_first_frame, "the style always clears the flag");
    }

    #[test]
    fn at_clock_zero_the_row_warp_is_the_identity() {
        let mut table = quad_table();
        let mut elapsed = 0i16;
        let tick = tick_curtain(&mut table, &mut elapsed, 1);
        for row in 0..CURTAIN_ROWS {
            assert_eq!(tick.quads[row as usize * 2].request.y, row as i16);
        }
    }

    #[test]
    fn the_row_warp_pushes_away_from_the_centre_as_the_clock_runs() {
        let mut table = quad_table();
        let mut elapsed = 28i16;
        let tick = tick_curtain(&mut table, &mut elapsed, 1);
        // (0 - 120) * (28 + 28) / 28 + 120 == -240 + 120 == -120.
        assert_eq!(tick.quads[0].request.y, -120);
        // The centre row never moves.
        let centre = CURTAIN_ROW_CENTRE as usize * 2;
        assert_eq!(tick.quads[centre].request.y, CURTAIN_ROW_CENTRE as i16);
    }

    #[test]
    fn every_column_is_visible_at_clock_zero_and_most_are_culled_later() {
        let mut table = quad_table();
        let mut elapsed = 0i16;
        let tick = tick_curtain(&mut table, &mut elapsed, 1);
        assert_eq!(tick.culled_columns, 0);
        let cols: Vec<_> = tick
            .quads
            .iter()
            .filter(|q| q.desc_index == CURTAIN_COL_DESC)
            .collect();
        assert_eq!(cols.len(), CURTAIN_COLS as usize);
        assert_eq!(cols[0].request.x, -CURTAIN_COL_CENTRE as i16 + 0x1E0);
        assert_eq!(cols[0].request.scale_y, 0x1000, "no stretch at clock zero");

        let mut elapsed = 100i16;
        let later = tick_curtain(&mut table, &mut elapsed, 1);
        assert!(later.culled_columns > 0);
    }

    #[test]
    fn the_column_pass_swaps_page_and_biases_u_past_the_split() {
        let mut table = quad_table();
        let mut elapsed = 0i16;
        tick_curtain(&mut table, &mut elapsed, 1);
        // The last column processed leaves its patch behind.
        assert_eq!(table[CURTAIN_COL_DESC].tpage, CURTAIN_COL_TPAGE_RIGHT);
        assert_eq!(
            table[CURTAIN_COL_DESC].u0,
            ((CURTAIN_COLS - 1 + 0x40) as u8)
        );
    }
}
