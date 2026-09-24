//! The **battle per-actor draw** `FUN_80048A08` (SCUS): every per-object
//! decision the draw makes before it hands a TMD object to a renderer leaf,
//! as pure functions a host applies to its own GPU path.
//!
//! Source: `ghidra/scripts/funcs/80048a08.txt` (disassembly, read end to end).
//!
//! # What the draw does, in order
//!
//! 1. **Actor matrix.** `RotMatrix(actor+0x24)` with translation
//!    `actor+0x2C/+0x30/+0x34`, composed under the camera matrix saved at
//!    `0x1F8003C8` (`0x80048A40..0x80048A78`). Then `FUN_800495C8(actor)` and
//!    the pose decoder `FUN_8004998C(actor)` (ported in [`crate::anim_vm`]).
//! 2. **Render scale.** `actor+0x72 != 0x1000` scales the actor matrix (and,
//!    for an object-effect actor, the second one at `0x1F8002F4`) by that q12
//!    factor on all three axes (`0x80048ABC..0x80048B18`) - [`render_scale`].
//! 3. **Per object** `s3 = 0 .. *(*(actor+0x4C)+0x88)` (a zero count skips
//!    the loop): translate by the decoded pose's translation, compose
//!    `RotMatrixZ`, `Y`, `X` of its Euler triple (each only when non-zero),
//!    then:
//!    * reload the colour word `gp[+0x9D8] = actor+0x74` and the blend
//!      `gp[+0x9DC] = actor+0x78` **per object** (`0x80048BEC..0x80048C00`),
//!      and for a party seat override both on a rotted limb
//!      ([`object_draw_words`]);
//!    * bias the ordering-table depth by `+0x50` while `actor+0x10` carries
//!      bit `0x00800000` ([`depth_bias`]);
//!    * pick the renderer leaf ([`draw_path`]).
//! 4. **Ground shadow** unless `actor+0x6A` is set ([`shadow_plan`]).
//! 5. `FUN_80049858(actor)`, then restore the camera matrix.
//!
//! # Rotted limbs are the one per-object colour rule
//!
//! For a party seat (`actor+0x5A < 3`) the draw reads the seated battle
//! actor's status word `+0x16E` (through the actor table `0x801C9370`) and
//! the character's row of a five-byte table at `0x80077998`, indexed by
//! `0x8007BD10[seat] - 1` (the 1-based roster id). The three Rot limb bits
//! each own an **object-index range** of the character's battle mesh:
//!
//! | bit | objects dimmed | test |
//! |---|---|---|
//! | `0x08` | `row[0] ..= row[1]` | `0x80048C2C..0x80048C7C` |
//! | `0x10` | `row[2] ..= row[3]` | `0x80048CA8..0x80048CF8` |
//! | `0x20` | `row[4] ..` (no upper bound) | `0x80048D24..0x80048D60` |
//!
//! A dimmed object draws with the colour word **quartered on R and G and
//! halved on B** - `((c & 0xFEFCFC) >> 1)`, then `& 0x7F0000` for B and
//! `(& 0x7E7E) >> 1` for G/R, with the top byte of `actor+0x74` ORed back -
//! and the blend forced to `0xC00` ([`rot_dim_colour`], [`ROT_DIM_IR0`]).
//! The renderer stages that pair as the GTE far colour and `IR0` (the
//! dispatcher `FUN_80043390` writes `(byte << 4) & 0xFFFE` into
//! `RFC/GFC/BFC` whenever the blend is non-zero), so a rotted limb is the
//! body's own packet colour pushed three quarters of the way to a dark blue
//! - [`dpcs_packet`].
//!
//! # The ground shadow
//!
//! With `actor+0x6A == 0` the draw re-enters the camera matrix, projects the
//! actor's position with its height `+0x16` zeroed (the point under it), and
//! has the procedural mesh builder `FUN_80028158` build a **24-segment
//! disc** (case `1`) into the asset buffer at `_DAT_8007B85C + 0x62400`,
//! radius `(actor+0x58 * 4) / 10`, x/z scale `0x1000`. It is drawn through
//! `FUN_80043390` with flag word `0x8A000000` - semi-transparent, blend mode
//! `2` (subtractive) - and only while the actor is neither pitched nor
//! rolled (`actor+0x24 == 0 && actor+0x28 == 0`, `0x80049284..0x8004929C`).
//! Its centre / rim colours are fixed greys, darker under a translucent
//! actor, or derived from the actor's own colour while it is airborne.

/// VA of the per-character Rot limb object-range table in `SCUS_942.54`.
pub const ROT_LIMB_TABLE_VA: u32 = 0x8007_7998;
/// Bytes per character row of that table.
pub const ROT_LIMB_ROW_LEN: usize = 5;
/// Rows the port reads: roster ids `1..=4` (Vahn, Noa, Gala and the
/// AI-companion id `4`).
pub const ROT_LIMB_ROWS: usize = 4;

/// Status bits (`+0x16E`) the draw tests, in table-column order.
pub const ROT_BIT_LIMB_A: u16 = 0x0008;
/// Second limb bit - objects `row[2] ..= row[3]`.
pub const ROT_BIT_LIMB_B: u16 = 0x0010;
/// Third limb bit - objects from `row[4]` up.
pub const ROT_BIT_LIMB_C: u16 = 0x0020;

/// The blend (`IR0`, q12) a rotted object draws with (`li v1,0xc00` at
/// `0x80048D70`).
pub const ROT_DIM_IR0: u16 = 0x0C00;

/// `actor+0x10` bit that pushes every object `+0x50` deeper in the ordering
/// table (`lui v1,0x80` at `0x80048E64`).
pub const FLAG_DEPTH_BIAS: u32 = 0x0080_0000;
/// The depth bias itself (`addiu v0,v0,0x50` at `0x80048E7C`).
pub const DEPTH_BIAS: i32 = 0x50;

/// Neutral render scale (`li v0,0x1000` at `0x80048AC0`).
pub const SCALE_ONE: u16 = 0x1000;

/// Shadow disc segment count (`li a2,0x18` at `0x80049240`).
pub const SHADOW_SEGMENTS: u8 = 0x18;
/// Shadow disc flag word (`lui a1,0x8a00` at `0x800492A8`).
pub const SHADOW_FLAG_WORD: u32 = 0x8A00_0000;

/// The per-character object ranges the draw dims under Rot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RotLimbTable {
    /// `rows[id - 1]` for roster id `id`.
    pub rows: [[u8; ROT_LIMB_ROW_LEN]; ROT_LIMB_ROWS],
}

impl RotLimbTable {
    /// Decode the table from a `SCUS_942.54` image. `None` when the image is
    /// not a PSX-EXE or is too short to hold the rows.
    pub fn from_scus(scus: &[u8]) -> Option<Self> {
        let off = legaia_asset::new_game::scus_file_offset(scus, ROT_LIMB_TABLE_VA)?;
        let bytes = scus.get(off..off + ROT_LIMB_ROW_LEN * ROT_LIMB_ROWS)?;
        let mut rows = [[0u8; ROT_LIMB_ROW_LEN]; ROT_LIMB_ROWS];
        for (row, chunk) in rows.iter_mut().zip(bytes.as_chunks::<ROT_LIMB_ROW_LEN>().0) {
            *row = *chunk;
        }
        Some(Self { rows })
    }

    /// The row for a 1-based roster id, `None` outside `1..=4` (retail
    /// indexes `id - 1` unchecked; the seats that reach this read always
    /// hold a party member).
    pub fn row(&self, roster_id: u8) -> Option<[u8; ROT_LIMB_ROW_LEN]> {
        self.rows
            .get(usize::from(roster_id).checked_sub(1)?)
            .copied()
    }
}

/// Whether object `object` of a character whose table row is `row` draws
/// dimmed under the status word `status` - the three range tests at
/// `0x80048C2C..0x80048D64`.
pub fn limb_object_dimmed(row: [u8; ROT_LIMB_ROW_LEN], status: u16, object: usize) -> bool {
    let s = object;
    let within = |lo: u8, hi: u8| usize::from(lo) <= s && s <= usize::from(hi);
    (status & ROT_BIT_LIMB_A != 0 && within(row[0], row[1]))
        || (status & ROT_BIT_LIMB_B != 0 && within(row[2], row[3]))
        || (status & ROT_BIT_LIMB_C != 0 && s >= usize::from(row[4]))
}

/// The dimmed colour word for a rotted object (`0x80048D68..0x80048DA8`):
/// the low 24 bits of `colour` quartered on R and G and halved on B, with
/// `colour`'s top byte carried through.
pub fn rot_dim_colour(colour: u32) -> u32 {
    let v = (colour & 0x00FE_FCFC) >> 1;
    let low = (v & 0x007F_0000) | ((v & 0x7E7E) >> 1);
    low | (colour & 0xFF00_0000)
}

/// The `(colour, blend)` pair one object of the actor draws with - the port
/// of the per-object reload at `0x80048BEC` and the Rot override after it.
///
/// `colour` / `blend` are the actor's `+0x74` / `+0x78`; `seat` is `+0x5A`;
/// `status` is the seated battle actor's `+0x16E`; `row` is the character's
/// [`RotLimbTable`] row (`None` for a seat the table does not cover).
// PORT: FUN_80048A08 (battle per-actor draw - per-object colour words, draw-path select, depth bias, render scale, ground-shadow plan)
pub fn object_draw_words(
    colour: u32,
    blend: u16,
    seat: i16,
    status: u16,
    row: Option<[u8; ROT_LIMB_ROW_LEN]>,
    object: usize,
) -> (u32, u16) {
    if seat < 3
        && let Some(row) = row
        && limb_object_dimmed(row, status, object)
    {
        return (rot_dim_colour(colour), ROT_DIM_IR0);
    }
    (colour, blend)
}

/// One GTE `DPCS` of a packet colour toward the far colour byte triple
/// `far` by `ir0` (q12), with the dispatcher's far-colour staging
/// (`(byte << 4) & 0xFFFE`, `0x800434B0..0x800434C4`) and the colour FIFO's
/// `>> 4` and `0..=255` saturation.
pub fn dpcs_packet(rgb: [u8; 3], far: [u8; 3], ir0: u16) -> [u8; 3] {
    let mut out = [0u8; 3];
    for i in 0..3 {
        let c = i32::from(rgb[i]) << 4;
        let fc = (i32::from(far[i]) << 4) & 0xFFFE;
        let mac = c + (((fc - c) * i32::from(ir0)) >> 12);
        out[i] = (mac >> 4).clamp(0, 255) as u8;
    }
    out
}

/// Low three bytes of a colour word as `[r, g, b]`.
pub fn colour_rgb(colour: u32) -> [u8; 3] {
    [colour as u8, (colour >> 8) as u8, (colour >> 16) as u8]
}

/// One actor's Rot dimming, resolved for the whole mesh: which objects dim
/// and the far colour / blend they dim with. Build it with
/// [`LimbDimPlan::resolve`]; `None` there means nothing on the actor dims.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LimbDimPlan {
    /// `dimmed[object]` - object indices past the end are not dimmed.
    pub dimmed: Vec<bool>,
    /// The dimmed colour word's `[r, g, b]` - the far colour.
    pub far: [u8; 3],
    /// The forced blend, [`ROT_DIM_IR0`].
    pub ir0: u16,
}

impl LimbDimPlan {
    /// Resolve the plan for an actor with `object_count` mesh objects.
    /// `None` when the seat is not a party seat, the row is missing, or no
    /// object falls in a set limb bit's range.
    pub fn resolve(
        colour: u32,
        seat: i16,
        status: u16,
        row: Option<[u8; ROT_LIMB_ROW_LEN]>,
        object_count: usize,
    ) -> Option<Self> {
        let dimmed: Vec<bool> = (0..object_count)
            .map(|o| object_draw_words(colour, 0, seat, status, row, o).1 == ROT_DIM_IR0)
            .collect();
        // `blend` is passed as 0 above, so a `ROT_DIM_IR0` result can only be
        // the override - an undimmed object keeps the 0.
        if !dimmed.iter().any(|&d| d) {
            return None;
        }
        Some(Self {
            dimmed,
            far: colour_rgb(rot_dim_colour(colour)),
            ir0: ROT_DIM_IR0,
        })
    }

    /// Whether `object` dims.
    pub fn is_dimmed(&self, object: u32) -> bool {
        self.dimmed.get(object as usize).copied().unwrap_or(false)
    }

    /// A key that changes whenever the plan's visible result does - lets a
    /// host re-upload a colour stream only on a change.
    pub fn key(&self) -> u64 {
        let mut k: u64 = 0xCBF2_9CE4_8422_2325;
        let mut mix = |b: u8| {
            k ^= u64::from(b);
            k = k.wrapping_mul(0x0000_0100_0000_01B3);
        };
        for &d in &self.dimmed {
            mix(u8::from(d));
        }
        for b in self.far {
            mix(b);
        }
        mix(self.ir0 as u8);
        mix((self.ir0 >> 8) as u8);
        k
    }

    /// Apply the plan to a per-vertex `[r, g, b, a]` packet-colour stream
    /// index-parallel with `object_ids`. A length mismatch leaves the stream
    /// untouched (the two were not built from the same vertex walk).
    pub fn apply_rgba(&self, rgba: &mut [u8], object_ids: &[u32]) {
        if rgba.len() != object_ids.len() * 4 {
            return;
        }
        for (px, &obj) in rgba.as_chunks_mut::<4>().0.iter_mut().zip(object_ids) {
            if self.is_dimmed(obj) {
                let d = dpcs_packet([px[0], px[1], px[2]], self.far, self.ir0);
                px[..3].copy_from_slice(&d);
            }
        }
    }

    /// Apply the plan to a per-vertex `[r, g, b]` packet-colour stream
    /// index-parallel with `object_ids` (the native VRAM-mesh layout). A
    /// length mismatch leaves the stream untouched.
    pub fn apply_rgb(&self, rgb: &mut [[u8; 3]], object_ids: &[u32]) {
        if rgb.len() != object_ids.len() {
            return;
        }
        for (px, &obj) in rgb.iter_mut().zip(object_ids) {
            if self.is_dimmed(obj) {
                *px = dpcs_packet(*px, self.far, self.ir0);
            }
        }
    }
}

/// Ordering-table depth offset the draw applies to every object of an actor
/// whose flag word `+0x10` is `flags` (`0x80048E60..0x80048E80`, undone at
/// `0x80048FEC..0x8004900C`).
pub fn depth_bias(flags: u32) -> i32 {
    if flags & FLAG_DEPTH_BIAS != 0 {
        DEPTH_BIAS
    } else {
        0
    }
}

/// The uniform scale the draw composes onto the actor matrix, as a factor
/// (`actor+0x72`, q12; `0x1000` skips the scale call).
pub fn render_scale(scale_q12: u16) -> f32 {
    f32::from(scale_q12) / f32::from(SCALE_ONE)
}

/// Which renderer leaf draws each object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawPath {
    /// `actor+0x42 != 0` - the object-effect transform `FUN_8001C204`, then
    /// (while `+0x42` is still set after it) the animated TMD renderer
    /// `FUN_8002735C`.
    ObjectEffect,
    /// `actor+0x7A != 0` - the light-source sibling `FUN_80029888`, with
    /// the 12-byte row `0x8007BE60 + actor+0x6D * 12` staged into the
    /// scratchpad and `a3 = (row word 8 << 16) | actor+0x7A`.
    Lit {
        /// `actor+0x6D` - the row index.
        row: u8,
    },
    /// Neither - the per-prim dispatcher `FUN_80043390`.
    Dispatch,
}

/// The renderer leaf for an actor with `+0x42 = effect_kind`,
/// `+0x7A = light_word` and `+0x6D = light_row` (`0x80048E84..0x80048FE4`).
/// `+0x42` wins; the `+0x7A` test runs only when it is zero.
pub fn draw_path(effect_kind: i16, light_word: i16, light_row: u8) -> DrawPath {
    if effect_kind != 0 {
        DrawPath::ObjectEffect
    } else if light_word != 0 {
        DrawPath::Lit { row: light_row }
    } else {
        DrawPath::Dispatch
    }
}

/// The ground-shadow disc one actor draws this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShadowPlan {
    /// Centre colour word (`0x1F800004`).
    pub inner: u32,
    /// Rim colour word (`0x1F800008`).
    pub outer: u32,
    /// Disc radius (`0x1F80001A`).
    pub radius: i16,
    /// Segment count, [`SHADOW_SEGMENTS`].
    pub segments: u8,
    /// Whether the built disc is also **drawn** - `false` while the actor
    /// is pitched or rolled (`+0x24` / `+0x28` non-zero).
    pub drawn: bool,
}

/// The shadow pass for an actor (`0x80049034..0x800492BC`); `None` when
/// `+0x6A` (`no_shadow`) skips it.
///
/// * `colour` - `+0x74`;
/// * `height` - `+0x16`, the actor's height above the ground;
/// * `size` - `+0x58`;
/// * `pitch` / `roll` - `+0x24` / `+0x28`.
pub fn shadow_plan(
    no_shadow: i16,
    colour: u32,
    height: i16,
    size: i16,
    pitch: i16,
    roll: i16,
) -> Option<ShadowPlan> {
    if no_shadow != 0 {
        return None;
    }
    let (inner, outer) = if height != 0 {
        ((colour >> 1) & 0x003F_3F3F, (colour >> 5) & 0x0007_0707)
    } else if colour & 0x8300_0000 != 0 {
        (0x0020_2020, 0x0004_0404)
    } else {
        (0x0040_4040, 0x0008_0808)
    };
    // `(size << 2) * 0x66666667`, high word `>> 2`, minus the sign: a
    // truncating signed divide by ten.
    let radius = ((i32::from(size) << 2) / 10) as i16;
    Some(ShadowPlan {
        inner,
        outer,
        radius,
        segments: SHADOW_SEGMENTS,
        drawn: pitch == 0 && roll == 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROW: [u8; 5] = [2, 4, 5, 7, 10];

    #[test]
    fn limb_ranges_are_inclusive_and_the_third_is_open_ended() {
        assert!(!limb_object_dimmed(ROW, ROT_BIT_LIMB_A, 1));
        assert!(limb_object_dimmed(ROW, ROT_BIT_LIMB_A, 2));
        assert!(limb_object_dimmed(ROW, ROT_BIT_LIMB_A, 4));
        assert!(!limb_object_dimmed(ROW, ROT_BIT_LIMB_A, 5));
        assert!(limb_object_dimmed(ROW, ROT_BIT_LIMB_B, 5));
        assert!(limb_object_dimmed(ROW, ROT_BIT_LIMB_B, 7));
        assert!(!limb_object_dimmed(ROW, ROT_BIT_LIMB_B, 8));
        assert!(!limb_object_dimmed(ROW, ROT_BIT_LIMB_C, 9));
        assert!(limb_object_dimmed(ROW, ROT_BIT_LIMB_C, 10));
        assert!(limb_object_dimmed(ROW, ROT_BIT_LIMB_C, 200));
        // A bit outside the three (0x40 is inside Rot's guard mask but no
        // range test reads it) dims nothing.
        assert!(!(0..32).any(|o| limb_object_dimmed(ROW, 0x0040, o)));
    }

    #[test]
    fn dim_colour_quarters_red_and_green_and_halves_blue() {
        assert_eq!(rot_dim_colour(0x0080_8080), 0x0040_2020);
        assert_eq!(rot_dim_colour(0x00FF_FFFF), 0x007F_3F3F);
        // The top byte rides through from the actor word.
        assert_eq!(rot_dim_colour(0x8A80_8080), 0x8A40_2020);
    }

    #[test]
    fn only_party_seats_dim() {
        let status = ROT_BIT_LIMB_A;
        assert_eq!(
            object_draw_words(0x0080_8080, 0, 0, status, Some(ROW), 3),
            (0x0040_2020, ROT_DIM_IR0)
        );
        assert_eq!(
            object_draw_words(0x0080_8080, 0x123, 3, status, Some(ROW), 3),
            (0x0080_8080, 0x123)
        );
        // Undimmed objects keep the actor's own blend.
        assert_eq!(
            object_draw_words(0x0080_8080, 0x123, 0, status, Some(ROW), 9),
            (0x0080_8080, 0x123)
        );
    }

    #[test]
    fn dpcs_pushes_three_quarters_toward_far() {
        // Neutral 0x80 toward far (0x20, 0x20, 0x40) at 0.75.
        assert_eq!(
            dpcs_packet([0x80; 3], [0x20, 0x20, 0x40], ROT_DIM_IR0),
            [0x38, 0x38, 0x50]
        );
        assert_eq!(dpcs_packet([0x55; 3], [0x10; 3], 0), [0x55; 3]);
        // The far staging drops the low fraction bit, which the `>> 4` hides.
        assert_eq!(dpcs_packet([0x00; 3], [0xFF; 3], 0x1000), [0xFF; 3]);
    }

    #[test]
    fn plan_resolves_and_applies_only_to_dimmed_objects() {
        assert!(LimbDimPlan::resolve(0x0080_8080, 0, 0, Some(ROW), 12).is_none());
        let plan = LimbDimPlan::resolve(0x0080_8080, 1, ROT_BIT_LIMB_B, Some(ROW), 12).unwrap();
        assert_eq!(plan.far, [0x20, 0x20, 0x40]);
        assert!(plan.is_dimmed(6) && !plan.is_dimmed(4) && !plan.is_dimmed(99));
        let ids = [4u32, 6, 6];
        let mut rgba = [0x80u8, 0x80, 0x80, 0xFF].repeat(3);
        plan.apply_rgba(&mut rgba, &ids);
        assert_eq!(&rgba[0..4], &[0x80, 0x80, 0x80, 0xFF]);
        assert_eq!(&rgba[4..8], &[0x38, 0x38, 0x50, 0xFF]);
        let mut rgb = vec![[0x80u8; 3]; 3];
        plan.apply_rgb(&mut rgb, &ids);
        assert_eq!(rgb[2], [0x38, 0x38, 0x50]);
        // A stream from a different vertex walk is left alone.
        let mut short = vec![[0x80u8; 3]; 2];
        plan.apply_rgb(&mut short, &ids);
        assert_eq!(short, vec![[0x80u8; 3]; 2]);
        // The key follows the visible result.
        let other = LimbDimPlan::resolve(0x0080_8080, 1, ROT_BIT_LIMB_A, Some(ROW), 12).unwrap();
        assert_ne!(plan.key(), other.key());
    }

    #[test]
    fn a_party_seat_outside_three_never_resolves() {
        assert!(LimbDimPlan::resolve(0x0080_8080, 3, 0x38, Some(ROW), 12).is_none());
        assert!(LimbDimPlan::resolve(0x0080_8080, 0, 0x38, None, 12).is_none());
    }

    #[test]
    fn depth_bias_scale_and_path() {
        assert_eq!(depth_bias(0x0080_0000), 0x50);
        assert_eq!(depth_bias(0x0040_0000), 0);
        assert_eq!(render_scale(0x1000), 1.0);
        assert_eq!(render_scale(0x0800), 0.5);
        assert_eq!(draw_path(1, 1, 3), DrawPath::ObjectEffect);
        assert_eq!(draw_path(0, 2, 3), DrawPath::Lit { row: 3 });
        assert_eq!(draw_path(0, 0, 3), DrawPath::Dispatch);
    }

    #[test]
    fn shadow_colours_radius_and_draw_gate() {
        assert_eq!(shadow_plan(1, 0, 0, 100, 0, 0), None);
        let s = shadow_plan(0, 0x0080_8080, 0, 100, 0, 0).unwrap();
        assert_eq!(
            (s.inner, s.outer, s.radius, s.segments, s.drawn),
            (0x404040, 0x080808, 40, 24, true)
        );
        let t = shadow_plan(0, 0x0180_8080, 0, 100, 0, 0).unwrap();
        assert_eq!((t.inner, t.outer), (0x202020, 0x040404));
        let air = shadow_plan(0, 0x0080_8080, -50, 100, 0, 0).unwrap();
        // `(c >> 1) & 0x3F3F3F` clears bit 6 of each channel, so a neutral
        // 0x808080 actor's airborne centre is black - the retail arithmetic.
        assert_eq!((air.inner, air.outer), (0x000000, 0x040404));
        let tinted = shadow_plan(0, 0x0070_7070, 1, 100, 0, 0).unwrap();
        assert_eq!((tinted.inner, tinted.outer), (0x383838, 0x030303));
        // Truncating signed divide by ten.
        assert_eq!(shadow_plan(0, 0, 0, -7, 0, 0).unwrap().radius, -2);
        assert_eq!(shadow_plan(0, 0, 0, 7, 0, 0).unwrap().radius, 2);
        assert!(!shadow_plan(0, 0, 0, 7, 0x100, 0).unwrap().drawn);
        assert!(!shadow_plan(0, 0, 0, 7, 0, 0x100).unwrap().drawn);
    }

    #[test]
    fn table_from_scus_reads_four_rows() {
        // Synthetic PSX-EXE: header + a data segment covering the table.
        let t_addr: u32 = 0x8007_7000;
        let mut exe = vec![0u8; 0x800 + 0x1000];
        exe[..8].copy_from_slice(b"PS-X EXE");
        exe[0x18..0x1C].copy_from_slice(&t_addr.to_le_bytes());
        exe[0x1C..0x20].copy_from_slice(&0x1000u32.to_le_bytes());
        let off = 0x800 + (ROT_LIMB_TABLE_VA - t_addr) as usize;
        for (i, b) in exe[off..off + 20].iter_mut().enumerate() {
            *b = i as u8;
        }
        let t = RotLimbTable::from_scus(&exe).expect("table");
        assert_eq!(t.row(1), Some([0, 1, 2, 3, 4]));
        assert_eq!(t.row(4), Some([15, 16, 17, 18, 19]));
        assert_eq!(t.row(0), None);
        assert_eq!(t.row(5), None);
        assert!(RotLimbTable::from_scus(b"not an exe").is_none());
    }
}
