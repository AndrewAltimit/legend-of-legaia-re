//! The particle fields, the subtractive wash and the backdrop prim.

use super::*;
use crate::battle_intro_styles as styles;
use crate::battle_intro_styles::ParticleTickStyle;
use crate::gte::GteVec3;
use crate::screen_prim::{ScreenPrim, ScreenQuad, display_rect_flat_quad};

// ---------------------------------------------------------------------------
// The particle fields (FUN_801CFDA0 / FUN_801D0370)
// ---------------------------------------------------------------------------

/// OT bucket a particle quad links at, in the raw-OTZ scale the dispatcher
/// buckets use (a dispatcher word index is `otz >> 2`, and both particle
/// ticks link at OT byte offset `400` - word `100` - so the equivalent OTZ is
/// `400`). Nearer than the spin-up ring's projected depth (`~0x200` at view z
/// `0x800`), farther than the fade quad's `1..=2`.
pub const PARTICLE_OT: u32 = 400;
/// `FUN_801D0370` links a particle that moved this frame **one OT word
/// nearer** (`s6 = -1`, `OT + 400 + s6 * 4`).
pub const PARTICLE_OT_MOVED: u32 = 396;

/// The full-screen wash `FUN_801CFDA0` arms on every frame after the first,
/// and `FUN_801D1888` arms once the swirl clock has passed its late-phase
/// frame (both `func_0x8004695C(0x101010)` - near-black).
pub const PARTICLE_WASH_RGB: u32 = 0x0010_1010;

/// A `FUN_8004695C` wash as a primitive.
///
/// `FUN_8004695C` only *arms* it (`gp+0x9D4 = 1`, `gp+0x9D0 = rgb`, and it
/// clears `_DAT_8007B6CC` on the way past). The drain is `FUN_80046978`,
/// which scales each channel by the scratchpad brightness byte at
/// `0x1F800393`, clamps to `0xFF`, clears the armed flag - so it is a
/// **one-shot per arm**, which is why `FUN_801CFDA0` re-arms it every frame -
/// and hands it to `FUN_80024EE4(otlen - 1, 2, rgb)`. Both arguments matter:
///
/// * `a0 = otlen - 1` is the **farthest** OT bucket, so this draws before
///   everything else in the frame,
/// * `a1 = 2` is ABR mode `B - F`, so it **subtracts** rather than fills.
///
/// Retail is therefore darkening whatever the framebuffer already holds, not
/// painting an opaque background. The port models the packet faithfully; what
/// this *primitive* cannot reproduce is the accumulation the effect rides on,
/// because each port frame is composed from scratch rather than from the
/// previous frame's pixels. Over [`BACKDROP_RGB`] the subtraction is a
/// near-no-op, which is the honest outcome rather than a fabricated one.
///
/// The **curtain** no longer relies on this: its per-frame `0x80808` wash is
/// carried as the CPU display model's decay
/// ([`CURTAIN_DISPLAY_DECAY_5`], see the module docs), which is why its tick
/// emits no wash primitive at all. The two `0x101010` washes this function
/// still draws - the scatter field's and the swirl's late phase - keep the
/// from-scratch caveat: their styles redraw nearly the whole display every
/// frame, so the residual is the absence of a short motion trail behind the
/// flying patches, not a wrongly-lit frame.
pub fn wash_prim(gp0_rgb_word: u32) -> ScreenPrim {
    display_rect_flat_quad(
        [
            (gp0_rgb_word & 0xFF) as u8,
            ((gp0_rgb_word >> 8) & 0xFF) as u8,
            ((gp0_rgb_word >> 16) & 0xFF) as u8,
            0xFF,
        ],
        true,
        WASH_ABR_SUBTRACT,
        u32::MAX,
    )
}

/// The ABR mode `FUN_80046978` passes for the armed wash: `2`, i.e. `B - F`.
pub const WASH_ABR_SUBTRACT: u8 = 2;

/// The colour the transition's own frame starts from.
///
/// The transition **owns the whole frame**: its init routine writes game mode
/// `9` into `_DAT_8007B83C` (`0x801CF180`/`0x801CF188`, the "efect" mode) and
/// the field's mode-3 renderer never runs again for the rest of the window.
/// The field's last frame is captured once, at init, into the texture page at
/// VRAM `(320, 256)` - and from there on every visible pixel is a transition
/// primitive sampling that page.
///
/// That is what makes the base colour black rather than "the field". Each
/// particle packet is semi-transparent with the page's own ABR `1` (`B + F`,
/// additive), so a record still at its rest pose reproduces its captured
/// patch **exactly** only when what is under it is black; the un-moved grid
/// then reconstructs the frame, and every patch that flies away leaves the
/// base colour behind it.
pub const BACKDROP_RGB: u32 = 0x0000_0000;

/// The frame the transition composes onto: an opaque display-rect quad at the
/// farthest OT bucket, in [`BACKDROP_RGB`].
///
/// Retail has no such primitive, and does not need one - it has a game mode
/// with no field renderer in it. The port composes the transition's screen
/// primitives *over a live scene* (the native renderer's
/// `SceneWithScreenPrims` target),
/// which is a port artifact, so the field kept rendering underneath. Two
/// things came out of that, both visible in a screenshot sweep: every patch
/// still at its rest pose was drawn additively over an identical live copy of
/// itself and read at double brightness, and once the last particle expired
/// the transition emitted nothing at all - leaving the remaining ~54 frames
/// of the window showing a clean, untouched, still-animating field.
///
/// This quad is the port's stand-in for "the field is not in the ordering
/// table". It is emitted on every frame of the window, including the frames a
/// style draws nothing on, which is also what keeps the host's target choice
/// on the compositing arm for the whole transition.
pub fn backdrop_prim() -> ScreenPrim {
    display_rect_flat_quad(
        [
            (BACKDROP_RGB & 0xFF) as u8,
            ((BACKDROP_RGB >> 8) & 0xFF) as u8,
            ((BACKDROP_RGB >> 16) & 0xFF) as u8,
            0xFF,
        ],
        false,
        0,
        u32::MAX,
    )
}

/// A GP0 colour word (`R` in the low byte) as [`ScreenQuad::color`]'s
/// `0x00RRGGBB`.
fn gp0_rgb(word: u32) -> u32 {
    (word & 0xFF) << 16 | (word & 0xFF00) | ((word >> 16) & 0xFF)
}

/// One frame of a particle-field style, integration and packet emission
/// together. `FUN_801CFDA0` / `FUN_801D0370`.
///
/// Retail's per-particle packet is a 10-word `POLY_FT4` (tag `0x09000000`,
/// colour code `0x2C`, `|= 2` - semi-transparent, ABR 1 additive off the
/// page's TSB - once the particle's delay has expired) sampling an **8 x 8
/// texel patch of the captured field frame**: texture page
/// `(rec[+0x28] >> 6) + 0x135` with `u = rec[+0x28] & 0x3F`,
/// `v = rec[+0x2A]`. Pages `0x135..=0x139` are 15-bpp pages at
/// `(320 + 64k, 256)`, and u stays below `0x40`, so the five pages tile
/// exactly the 320 captured columns - the seeded grid *is* the screen cut
/// into 8 x 8 patches (40 columns x 29 visited rows = 320 x 232 of the 240
/// rows, which is why the ticks stop at `0x488` records). At the seeded rest
/// pose the projection maps a cell straight back onto its own patch
/// (`z = 0x1000` under `H = 0x80` is a 1/32 scale for style A; style B's
/// `>> 3` pre-divide against `z = 0x2000 >> 3` lands the same 8-px cell), so
/// the un-moved field reconstructs the captured frame and particles peel off
/// it as their delays expire.
///
/// The packet is authored from the record's **frame-entry** state: the colour
/// word, UVs and both pose matrices are latched before the integration block
/// runs (`0x801CFEA4..0x801CFF3C` all precede the moved arm), so a decaying or
/// accelerating particle draws this frame's starting pose and colour. The
/// pose chain is the per-record `SetRotMatrix(view)` →
/// `FUN_8003D344(rec+0x10)` → `FUN_80026988(rec+0x08)` sequence: **`+0x10` is
/// the position triple and `+0x08` the Euler triple** (`FUN_80026988` is the
/// Euler-to-matrix kernel and it is handed `+0x08`), the reverse of the
/// labels the seeder-era notes used - the same position/rotation inversion
/// the tile record went through. The kernel field names
/// (`IntroParticle::rot` / `IntroParticle::trans`) still carry the old
/// labels; the arithmetic is unaffected because integration pairs are
/// unchanged.
///
/// Projection is `FUN_8005BAC8` (RotTransPers4): corners `(0,0)`, `(s,0)`,
/// `(0,s)`, `(s,s)` at the style's quad size, depth = `SZ3 >> 2`, then the
/// screen-window accept
/// ([`crate::battle_intro_styles::particle_quad_accepted`]) and a
/// fixed OT bucket ([`PARTICLE_OT`]). `emit` is the first-frame gate the
/// caller derives from the transition clock - see the tile arm's
/// stale-view-matrix note.
///
/// PORT: FUN_801CFDA0
/// PORT: FUN_801D0370
/// REF: FUN_8005BAC8 (RotTransPers4; returns `SZ3 >> 2`)
pub fn emit_particle_field(
    grid: &mut [crate::battle_intro_particles::IntroParticle],
    style: &ParticleTickStyle,
    elapsed: &mut i16,
    frame_step: u8,
    emit: bool,
    prims: &mut Vec<ScreenPrim>,
) -> bool {
    let scaled = i32::from(*elapsed) * style.delay_scale;
    let mut drawn = false;
    for p in grid.iter_mut().take(styles::PARTICLE_TICK_COUNT) {
        // Latch the packet inputs before the integration mutates the record.
        let color = gp0_rgb(p.tint);
        let u = (p.texel_page as u16 & 0x3F) as u8;
        let v = p.texel_v as u8;
        let tpage = (p.texel_page >> 6).wrapping_add(styles::PARTICLE_TPAGE_BIAS) as u16;
        let pos = p.rot; // +0x10: the position triple (see the doc above)
        let ang = p.trans; // +0x08: the Euler triple
        let step = styles::step_particle(p, style, frame_step, scaled);
        let styles::ParticleStep::Live { moved, .. } = step else {
            continue;
        };
        if !emit {
            continue;
        }
        let s = style.rot_prescale_shift;
        let tr = GteVec3::new(
            i32::from(pos.0) >> s,
            i32::from(pos.1) >> s,
            i32::from(pos.2) >> s,
        );
        let rot = euler_rot_psx(ang);
        let q = style.quad_size;
        let corners = [(0, 0), (q, 0), (0, q), (q, q)];
        let pc: [ProjCorner; 4] = std::array::from_fn(|i| {
            project_intro_corner(&rot, tr, (corners[i].0, corners[i].1, 0))
        });
        // FUN_8005BAC8 returns the fourth corner's SZ quartered.
        let depth = pc[3].sz >> 2;
        if !styles::particle_quad_accepted(style, depth, (pc[0].xy.x as i16, pc[0].xy.y as i16)) {
            continue;
        }
        let ot = if moved && style.moved_links_nearer {
            PARTICLE_OT_MOVED
        } else {
            PARTICLE_OT
        };
        prims.push(ScreenPrim::Textured(ScreenQuad {
            xy: std::array::from_fn(|i| (pc[i].xy.x as i16, pc[i].xy.y as i16)),
            uv: [
                (u, v),
                (u.wrapping_add(8), v),
                (u, v.wrapping_add(8)),
                (u.wrapping_add(8), v.wrapping_add(8)),
            ],
            clut: 0,
            tpage,
            color,
            gouraud: None,
            semi_transparent: moved,
            ot_index: ot,
            depth: None,
        }));
        drawn = true;
    }
    *elapsed = (*elapsed as u16).wrapping_add(u16::from(frame_step)) as i16;
    drawn
}
