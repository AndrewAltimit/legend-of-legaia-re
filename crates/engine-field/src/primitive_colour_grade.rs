//! The geometry colour grade [`shift_primitive_colours`] (`FUN_801D5E20`): an
//! HSV shift applied to a TMD's packed primitive colour words.
//!
//! The `PORT` tag lives on the item that implements the address, so the
//! liveness audit sees one anchor per port site.

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
/// REPLACED-BY: the renderer's prologue palette mode, whose packet-colour law
/// (`legaia_engine_render::psx_light::prologue_sepia_word` and the page
/// shader's twin) is this routine's two-op result in closed form, keyed on
/// `World::scene_color_grade` for exactly the three scenes whose scripts
/// issue the op.
///
/// The sole `jal` to `0x801D5E20` in the corpus is `0x801D82F8`, inside the
/// `REF`'d walker `FUN_801D8280`, which is the field VM's op `0x4C`
/// outer-nibble-E sub-6 arm (`FieldActorSpawnHost::hsv_grade_resident_meshes`, three
/// `i16` operands, which the walker forwards as this routine's `a1..a3` - the
/// hue / saturation / value deltas, `addu`-ed onto the `FUN_8001A78C` result
/// at `0x801D5F10` / `0x801D5F20` / `0x801D5F30`). The disc issues that op in
/// exactly three carriers (`asset field-op-census --only "4C E6"`): partition
/// 1 record 0 of `opdeene`, `opstati` and `opurud`, the same two ops in each -
/// `4C E6 00 00 00 FF 00 00` (saturation `-0x100`, clamped to `0`: every
/// resident mesh colour greyed at its `max`, capped at `0xF8` by
/// `FUN_8001A6C8`) followed by `4C E6 38 00 90 00 E2 FF` (hue `+0x38`,
/// saturation `+0x90`, value `-0x1E`). The second pass starts from a grey,
/// so a word's result depends on its `max` alone:
/// `(V, V * 246 >> 8, V * 112 >> 8)`, `V = min(max, 0xF8) - 30`. A retail
/// `opdeene` state (`s1_newgame_field`) holds every resident colour word on
/// that curve.
///
/// The walker's table (`DAT_8007C018`, every resident TMD) has no engine
/// counterpart, so rather than rewriting uploaded mesh bytes the renderer
/// applies the closed form per packet colour; a host that ran the op itself
/// would draw the same pixels.
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
}
