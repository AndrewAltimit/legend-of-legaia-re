//! The **field-to-battle transition's own parts**: the draw-kind-4 move-VM
//! parts the transition init `FUN_801CE8CC` seats beside each style, ticked
//! and drawn the way the field actor list ticks and draws every part.
//!
//! ## Where they come from
//!
//! The init's style switch (jump table `0x801CE840`, arms
//! `0x801CED58..0x801CF10C`) runs each style's seeder and then seats parts
//! through the SCUS stager `FUN_80021B04(pos, rot, record, 0x1000)` at
//! position `(0, 0, 0x3C0)` with no rotation (`sp+0x18..0x24`). The records
//! sit in the intro overlay's own image (PROT 0979, base `0x801CE818`), and
//! every one is a transform node (`model_sel = -1`) whose program sets a
//! colour word (op `0x0C`, mode byte `0xC9`: additive, double-sided; `0xCA`
//! on two of the swirl's), makes it a default-arm draw-kind-4 node (op
//! `0x13`, the procedural ring / crown / star builder `FUN_80028158`), then
//! waits, sets its channel, level and spin rates and waits out its life:
//!
//! | Style | Records |
//! |---|---|
//! | scatter | `0x801D1F14`, `0x801D1F60`, `0x801D1FB0`; sub-style `> 0` adds `0x801D2000` three times (tinted `0xA00000` / `0xA000` / `0xA0`) and `0x801D2050`; sub-style `>= 2` adds `0x801D2000`, `0x801D2050` and `0x801D20A0` twice |
//! | spin-up | `0x801D20F0`, `0x801D2134` |
//! | tile shatter | `0x801D2178` |
//! | curtain | none (the arm calls the inert stub `FUN_801D1CD4`) |
//! | swirl | `0x801D222C` five times (waits `0` / `0x28` / `0x50` / `0x78` / `0xA0`), `0x801D2380`, `0x801D23D8`, `0x801D2278`, `0x801D22D0`, `0x801D2328` |
//!
//! The scatter arm's tints and the staggered waits are stores the init makes
//! into the parts it was just handed, after their first VM run (several in
//! the delay slot of the next spawn's `jal`, so each lands on the part the
//! previous call returned).
//!
//! The scatter set is the white starburst with a blue core a PCSX-Redux film
//! of formation `[136]` grows at the screen centre: a probe on the packets
//! there names `FUN_80043390` under the actor walk `FUN_8001ADA4` (called
//! from `FUN_8001D140`), and the seven actors it draws carry these records at
//! `+0x44`. None of the style bodies `FUN_801CF5BC` runs draws them.
//!
//! ## How they tick and draw
//!
//! Each frame the part tick `FUN_80021DF4` runs the wait drain, the mode-2
//! channel block, the motion block and the VM gate at `delta = frame step x
//! 8` ([`legaia_engine_vm::move_vm`], [`crate::part_motion`]). The walk
//! skips a part with flag `0x2` or `0x8` (`andi v0,v0,0xa` at `0x8001AE54`),
//! so a halted part draws nothing; case 4 builds the node's mesh
//! ([`crate::effect_default_arm`]) and falls into the model draw, which
//! turns it by `FUN_80026988` of the banks `+0x24..+0x28`, scales it by
//! `+0x72`, and projects it through the GTE file the walk inherits: identity
//! rotation, zero translation, `H = 0x80`, centre `(160, 114)` - read live
//! at `FUN_8001ADA4`'s entry on every scatter frame of the film (the same
//! file the style bodies project with, `INTRO_H`). The rays' far corners in
//! the film's ordering table land where that file puts the part state the
//! tick log reads. The
//! dispatcher `FUN_80043390` depth-cues the packet colours by `+0x78` toward
//! the colour word's far colour and ORs its ABE / ABR into the packets.
//!
//! The builder's untextured modes sample the resident system patch (page
//! `0x1F`, CLUT `0x7F84`, texel index `2`), BGR555 grey `14` with its mask
//! bit set. The port draws the packets as gouraud flat quads at that texel's
//! modulation (`colour * 112 / 128`), semi-transparent on the colour word's
//! ABR, so the draw does not depend on the host's VRAM holding the patch.
//!
//! PORT: FUN_801CE8CC (`0x801CED58..0x801CF10C`, the part seats of the style
//! switch)
//! REF: FUN_80021B04, FUN_80021DF4, FUN_8001ADA4, FUN_80028158, FUN_80043390

use std::sync::Arc;

use legaia_engine_vm::move_vm::{self, ActorState, ActorTickOutcome, MoveHost};
use legaia_render_kernels::battle_intro::{
    INTRO_H, INTRO_OFX, INTRO_OFY, IntroPartRuntime, euler_rot_psx, intro_overlay_as_loaded,
};
use legaia_render_kernels::battle_intro_styles::IntroStyle;
use legaia_render_kernels::gte::{GteVec3, ScreenXY, gte_divide, gte_persp_term};
use legaia_render_kernels::screen_prim::{FlatQuad, ScreenPrim};
use legaia_render_kernels::sprite_arm::ColourWord;

use crate::effect_default_arm;

/// Where the init seats every part (`sp+0x18..0x1C`: `0`, `0`, `0x3C0`).
pub const SEAT_POS: [i16; 3] = [0, 0, 0x3C0];
/// The render scale the init passes (`li a3,0x1000`).
pub const SEAT_SCALE: u16 = 0x1000;
/// The part tick's speed byte `DAT_1F80037D` (the field's `8`).
pub const SPEED_SCALAR: u16 = 8;
/// Words read per record. The longest record runs `0x33` words to its
/// `HALT`; retail hands the VM a pointer, so the window only has to cover it.
pub const RECORD_WINDOW_WORDS: usize = 0x40;
/// The resident patch's texel, 8-bit (`0xB9CE`: grey `14` of `31`).
pub const PATCH_TEXEL_8: u16 = 14 << 3;

const PART_BUDGET: usize = 256;

/// The store the init makes into a part after its spawn, beside the record.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Seat {
    /// The record's VA.
    pub va: u32,
    /// `+0x54` (the wait timer), raw.
    pub wait: Option<i16>,
    /// `+0xA0`, the inner colour word.
    pub colour: Option<u32>,
    /// `+0xB4` / `+0xB6` (inner / outer radius).
    pub radius: Option<(i16, i16)>,
    /// `+0xC0`, the inner radius rate.
    pub rate_c0: Option<i16>,
}

const fn seat(va: u32) -> Seat {
    Seat {
        va,
        wait: None,
        colour: None,
        radius: None,
        rate_c0: None,
    }
}

/// The parts one style's arm seats, in spawn order, with the stores the init
/// makes into them.
pub fn style_seats(style: IntroStyle, sub_style: i32) -> Vec<Seat> {
    match style {
        IntroStyle::ScatterParticles => {
            // `bltz v0` at `0x801CED6C`: a negative sub-style seats nothing.
            if sub_style < 0 {
                return Vec::new();
            }
            let mut v = vec![seat(0x801D_1F14), seat(0x801D_1F60), seat(0x801D_1FB0)];
            // `blez v0` at `0x801CEDCC`.
            if sub_style > 0 {
                v.push(Seat {
                    colour: Some(0x00A0_0000),
                    radius: Some((0, 0x1E)),
                    ..seat(0x801D_2000)
                });
                v.push(Seat {
                    colour: Some(0x0000_A000),
                    radius: Some((0xF, 0x28)),
                    ..seat(0x801D_2000)
                });
                v.push(Seat {
                    colour: Some(0x0000_00A0),
                    radius: Some((0x1E, 0x3C)),
                    ..seat(0x801D_2000)
                });
                v.push(seat(0x801D_2050));
            }
            // `slti v0,v0,0x2` at `0x801CEE8C`.
            if sub_style >= 2 {
                // The first part takes `0x404040` and then, in the next
                // spawn's delay slot, `0xA0` over it (`0x801CEED8` /
                // `0x801CEEF4`).
                v.push(Seat {
                    colour: Some(0x0000_00A0),
                    radius: Some((0, 0x3C)),
                    rate_c0: Some(0),
                    ..seat(0x801D_2000)
                });
                v.push(Seat {
                    wait: Some(0x14),
                    colour: Some(0x00B0_4040),
                    ..seat(0x801D_2050)
                });
                v.push(Seat {
                    wait: Some(0x3C),
                    colour: Some(0x0040_B040),
                    ..seat(0x801D_20A0)
                });
                v.push(Seat {
                    wait: Some(0x50),
                    colour: Some(0x0040_40B0),
                    ..seat(0x801D_20A0)
                });
            }
            v
        }
        IntroStyle::SpinUpParticles => vec![seat(0x801D_20F0), seat(0x801D_2134)],
        IntroStyle::TileShatter => vec![seat(0x801D_2178)],
        IntroStyle::Curtain => Vec::new(),
        IntroStyle::Swirl => {
            let disc = |wait: Option<i16>| Seat {
                wait,
                ..seat(0x801D_222C)
            };
            vec![
                disc(None),
                disc(Some(0x28)),
                disc(Some(0x50)),
                disc(Some(0x78)),
                disc(Some(0xA0)),
                seat(0x801D_2380),
                seat(0x801D_23D8),
                seat(0x801D_2278),
                seat(0x801D_22D0),
                seat(0x801D_2328),
            ]
        }
    }
}

/// The move-VM host a transition part runs under: the records reach no hook
/// (no spawn, no extension op, no bytecode write).
struct PartHost;

impl MoveHost for PartHost {}

/// One live transition part.
#[derive(Debug, Clone)]
pub struct IntroPart {
    /// The record's VA.
    pub va: u32,
    words: Arc<[u16]>,
    /// The part's actor fields.
    pub state: ActorState,
    /// Set on the tick its VM halts; the walk draws nothing from then on.
    pub halted: bool,
}

impl IntroPart {
    /// Seat `words` (the record from `+0`) at [`SEAT_POS`] and run its first
    /// VM slice, then apply the init's stores.
    ///
    /// PORT: FUN_80021B04 - the transform-node seat (`model_sel < 0`): draw
    /// kind and render mode cleared, flag `0x2` set, PC `2`, `+0xC4` /
    /// `+0xC6` cleared, flag `0x4000`, the rotation banks and scale, and the
    /// first VM call ungated by the wait timer.
    pub fn spawn(seat: &Seat, words: Arc<[u16]>) -> Self {
        let mut st = ActorState::new();
        [st.world_x, st.world_y, st.world_z] = SEAT_POS;
        st.move_substate = 0;
        st.move_submode = 0;
        st.flags |= 0x2;
        st.pc = 2;
        st.set_actor_u16(0xC4, 0);
        st.set_actor_u16(0xC6, 0);
        st.flags |= 0x4000;
        st.anim_3c = 0;
        st.anim_3e = 0;
        st.anim_40 = 0;
        st.render_24 = 0;
        st.render_26 = 0;
        st.render_28 = 0;
        st.field_72 = SEAT_SCALE;
        let _ = move_vm::run_until_break(&mut PartHost, &mut st, &words, PART_BUDGET);
        st.world_y_mirror = st.world_y;
        if let Some(w) = seat.wait {
            st.wait_timer = w;
        }
        if let Some(c) = seat.colour {
            st.set_actor_u32(0xA0, c);
        }
        if let Some((inner, outer)) = seat.radius {
            st.set_actor_u16(0xB4, inner as u16);
            st.set_actor_u16(0xB6, outer as u16);
        }
        if let Some(r) = seat.rate_c0 {
            st.set_actor_u16(0xC0, r as u16);
        }
        let halted = st.flags & 0x8 != 0;
        IntroPart {
            va: seat.va,
            words,
            state: st,
            halted,
        }
    }

    /// One part tick at `delta = DAT_1F800393 * DAT_1F80037D`.
    ///
    /// PORT: FUN_80021DF4 (the wait drain, the mode-2 channel block, the
    /// motion block, the VM gate and the post-VM clamps - the blocks a
    /// draw-kind-4 transform node reaches)
    pub fn tick(&mut self, delta: u16) {
        if self.halted {
            return;
        }
        let st = &mut self.state;
        move_vm::decrement_wait_timer(st, delta);
        move_vm::integrate_draw_channels(st, delta);
        if crate::part_motion::runs_motion_block(st) {
            crate::part_motion::motion_block(st, delta);
        }
        let outcome = move_vm::actor_tick(&mut PartHost, st, &self.words, PART_BUDGET);
        if matches!(
            outcome,
            ActorTickOutcome::Halted | ActorTickOutcome::EndOfBuffer { .. }
        ) || st.flags & 0x8 != 0
        {
            self.halted = true;
        }
        crate::part_motion::clamp_levels(st);
    }

    /// The part's packets for this frame, projected through the transition's
    /// GTE file. Empty for a hidden or halted part or one off the default
    /// arm.
    pub fn emit(&self, prims: &mut Vec<ScreenPrim>) {
        let st = &self.state;
        if self.halted || st.flags & 0xA != 0 || !effect_default_arm::is_default_arm(st) {
            return;
        }
        let Some((mode, packed, src)) = effect_default_arm::call_args(st) else {
            return;
        };
        // Shapes 4 / 5 draw `rand()`; no transition record uses them.
        let Some(b) = effect_default_arm::build(
            0x8000_0000 + effect_default_arm::SCRATCH_OFFSET,
            mode,
            packed,
            &src,
            &crate::effect_ribbon::RetailTrig,
            || 0,
            0,
        ) else {
            return;
        };
        let d = effect_default_arm::decode(&b);
        let colour = ColourWord::of(st);
        let rot = euler_rot_psx((st.render_24, st.render_26, st.render_28));
        let scale = i32::from(st.field_72);
        let tr = GteVec3::new(
            i32::from(st.world_x),
            i32::from(st.world_y),
            i32::from(st.world_z),
        );
        let project = |v: [i16; 3]| -> (ScreenXY, i32) {
            let s = |c: i16| (i32::from(c) * scale) >> 12;
            let r = rot.mul_vec(GteVec3::new(s(v[0]), s(v[1]), s(v[2])));
            let (x, y, z) = (r.x + tr.x, r.y + tr.y, r.z + tr.z);
            let sz = z.clamp(0, 0xFFFF);
            let (recip, _) = gte_divide(INTRO_H, sz as u16);
            let ir = |v: i32| v.clamp(i32::from(i16::MIN), i32::from(i16::MAX));
            let sx = INTRO_OFX + gte_persp_term(ir(x), recip) as i32;
            let sy = INTRO_OFY + gte_persp_term(ir(y), recip) as i32;
            (ScreenXY::new(sx, sy).saturate_sxy(), sz)
        };
        for p in &d.packets {
            let pc: [(ScreenXY, i32); 4] = std::array::from_fn(|c| project(d.verts[p.verts[c]]));
            // A quad links at its corners' summed depth `>> 8` - the bucket
            // the film's ray packets sit in at view depth `0x3C0`.
            let otz = pc.iter().map(|c| c.1).sum::<i32>() >> 8;
            let col = |c: usize| -> [u8; 4] {
                let cued = colour.cue(p.rgb[c]);
                let m = |k: u8| ((u16::from(k) * PATCH_TEXEL_8) >> 7).min(255) as u8;
                [m(cued[0]), m(cued[1]), m(cued[2]), 0xFF]
            };
            prims.push(ScreenPrim::Flat(FlatQuad {
                xy: std::array::from_fn(|c| (pc[c].0.x as i16, pc[c].0.y as i16)),
                color: col(0),
                gouraud: Some(std::array::from_fn(col)),
                semi_transparent: colour.semi,
                abr_mode: colour.abr,
                ot_index: otz.max(0) as u32,
                depth: None,
            }));
        }
    }
}

/// The parts one transition seated.
#[derive(Debug, Clone, Default)]
pub struct IntroParts {
    parts: Vec<IntroPart>,
}

impl IntroParts {
    /// Seat `style`'s parts out of the raw PROT 0979 entry. `None` when the
    /// image does not relocate or a record falls outside it.
    pub fn seat(style: IntroStyle, sub_style: i32, overlay_0979: &[u8]) -> Option<Self> {
        let (img, base) = intro_overlay_as_loaded(overlay_0979)?;
        Self::seat_as_loaded(style, sub_style, &img, base)
    }

    /// [`Self::seat`] over an image already relocated to `base`.
    pub fn seat_as_loaded(
        style: IntroStyle,
        sub_style: i32,
        img: &[u8],
        base: u32,
    ) -> Option<Self> {
        let mut parts = Vec::new();
        for s in style_seats(style, sub_style) {
            let off = s.va.checked_sub(base)? as usize;
            let bytes = img.get(off..off + RECORD_WINDOW_WORDS * 2)?;
            let words: Arc<[u16]> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes(*c))
                .collect();
            parts.push(IntroPart::spawn(&s, words));
        }
        Some(Self { parts })
    }

    /// The live parts.
    pub fn parts(&self) -> &[IntroPart] {
        &self.parts
    }
}

impl IntroPartRuntime for IntroParts {
    fn step(&mut self, frame_step: u8) {
        let delta = u16::from(frame_step.max(1)) * SPEED_SCALAR;
        for p in &mut self.parts {
            p.tick(delta);
        }
    }

    fn emit(&self, prims: &mut Vec<ScreenPrim>) {
        for p in &self.parts {
            p.emit(prims);
        }
    }

    fn clone_box(&self) -> Box<dyn IntroPartRuntime> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests;
