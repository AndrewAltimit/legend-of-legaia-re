//! The geometry colour grade [`shift_primitive_colours`] (`FUN_801D5E20`): an
//! HSV shift applied to a TMD's packed primitive colour words, and the walk
//! that hands it every object of every resident TMD
//! ([`shift_resident_tmds`], `FUN_801D8280`).
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

/// The table at [`COLOUR_COUNT_TABLE_ADDR`], selectors `0..=0x13`: none for the
/// two light-source rows (group flags `0x10..=0x17`), which carry no colour
/// block, then one word for a flat primitive, three for a gouraud triangle and
/// four for a gouraud quad - rows `0x18..=0x1F` untextured and `0x20..=0x27`
/// textured.
pub const COLOUR_COUNTS: [u8; 0x14] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 3, 4, 1, 1, 3, 4];

/// Colour words a primitive of group selector `flags >> 1` carries. Past the
/// table retail reads whatever bytes follow it in the overlay's data; no
/// shipped group has such a selector, and the port answers `0`.
pub fn colour_count(selector: u16) -> u8 {
    COLOUR_COUNTS
        .get(usize::from(selector))
        .copied()
        .unwrap_or(0)
}

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

/// Bytes of a TMD header, which every `*_top` offset is relative to the end of.
const TMD_HEADER: usize = 12;

/// Bytes per TMD object-table entry.
const TMD_OBJECT: usize = 0x1C;

fn word(bytes: &[u8], at: usize) -> Option<u32> {
    bytes
        .get(at..at + 4)
        .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
}

/// Shift the hue, saturation and value of every packed colour word in one
/// object's primitive section, in place. `FUN_801D5E20`.
///
/// `block` starts at the object's first group header. The section is walked
/// group by group until a header whose whole first **word** is zero
/// (`0x801D5E60`, `0x801D6018`). A group is its 8-byte header, `count` records
/// of `ilen * 4` bytes and one more record's worth of footer, which is the
/// layout every Legaia TMD has (`legaia_tmd::legaia_prims`): the stride add
/// runs once per record (`0x801D5FF8`) and once after the loop (`0x801D6010`),
/// and an empty group jumps straight to the second.
///
/// Within a record the first [`colour_count`]`(flags >> 1)` words are its
/// colours, and a count of `0` leaves the group's records alone. Each colour is
/// the first three bytes of its word; the fourth, the GP0 code byte, is not
/// written. The shift goes through the SCUS pair `FUN_8001A78C` /
/// `FUN_8001A6C8`, so every channel it writes is capped at `0xF8`.
///
/// Returns how many colour words it rewrote. A walk that runs off `block`
/// stops there, where retail would read on.
///
/// PORT: FUN_801D5E20
///
/// REPLACED-BY: the renderer's prologue palette mode, whose packet-colour law
/// (`legaia_engine_render::psx_light::prologue_sepia_word` and the page
/// shader's twin) is this routine's two-op result in closed form, keyed on
/// `World::scene_color_grade` for exactly the three scenes whose scripts
/// issue the op.
///
/// The sole `jal` to `0x801D5E20` in the corpus is `0x801D82F8`, inside the
/// walker [`shift_resident_tmds`], which is the field VM's op `0x4C`
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
pub fn shift_primitive_colours(block: &mut [u8], shift: &HsvShift) -> usize {
    let mut touched = 0;
    let mut at = 0usize;
    while let Some(first) = word(block, at) {
        if first == 0 {
            break;
        }
        let count = usize::from(first as u16);
        let selector = (first >> 16) as u16 >> 1;
        let Some(&ilen) = block.get(at + 5) else {
            break;
        };
        let stride = usize::from(ilen) * 4;
        at += 8;
        let per_record = usize::from(colour_count(selector));
        for _ in 0..count {
            for k in 0..per_record {
                let Some(rgb) = block.get_mut(at + k * 4..at + k * 4 + 3) else {
                    return touched;
                };
                let (h, s, v) = legaia_engine_vm::move_vm::rgb_to_hsv(
                    i32::from(rgb[0]),
                    i32::from(rgb[1]),
                    i32::from(rgb[2]),
                );
                let (h, s, v) = shift.apply(h, s, v);
                let (r, g, b) = legaia_engine_vm::move_vm::hsv_to_rgb(h, s, v);
                rgb[0] = r.min(0xF8) as u8;
                rgb[1] = g.min(0xF8) as u8;
                rgb[2] = b.min(0xF8) as u8;
                touched += 1;
            }
            at += stride;
        }
        at += stride;
    }
    touched
}

/// Shift every object of one TMD, in place: object `i`'s primitive section is
/// at `prim_top` (object entry `+0x10`) past the 12-byte header. Returns the
/// colour words rewritten.
///
/// This is the body of [`shift_resident_tmds`]'s outer loop
/// (`0x801D82D0..0x801D8310`) over a TMD in its file form, offsets not yet
/// patched to addresses: retail's loop reads the object count at `+8` and
/// steps `0x1C` from `+0x0C`, and `FUN_801D5E20` reads each entry's `+0x10`.
pub fn shift_tmd_colours(tmd: &mut [u8], shift: &HsvShift) -> usize {
    let Some(objects) = word(tmd, 8) else {
        return 0;
    };
    let mut touched = 0;
    for i in 0..objects as usize {
        let Some(prim_top) = word(tmd, TMD_HEADER + i * TMD_OBJECT + 0x10) else {
            break;
        };
        // The registrar `FUN_800268DC` makes the offset an address as
        // `header end + (top & !3)`.
        let Some(block) = tmd.get_mut(TMD_HEADER + (prim_top & !3) as usize..) else {
            continue;
        };
        touched += shift_primitive_colours(block, shift);
    }
    touched
}

/// Shift every resident TMD: `FUN_801D8280(dh, ds, dv)`, the walk of the
/// resident-object table `DAT_8007C018[0..=DAT_8007BB38]` behind the field
/// VM's `4C E6`. The count word is the table's last index, so `-1` walks
/// nothing (`addiu v0,v0,1` / `beqz` at `0x801D82BC`). Returns the colour words
/// rewritten.
///
/// PORT: FUN_801D8280
/// REPLACED-BY: the renderer's prologue palette mode - see
/// [`shift_primitive_colours`]. No host keeps resident TMD bytes to rewrite.
pub fn shift_resident_tmds<'a>(
    tmds: impl IntoIterator<Item = &'a mut [u8]>,
    shift: &HsvShift,
) -> usize {
    tmds.into_iter()
        .map(|tmd| shift_tmd_colours(tmd, shift))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// One group: header, `records` of `ilen` words, and the footer slot.
    fn group(count: u16, flags: u16, ilen: u8, fill: [u8; 4]) -> Vec<u8> {
        let mut g = Vec::new();
        g.extend_from_slice(&count.to_le_bytes());
        g.extend_from_slice(&flags.to_le_bytes());
        g.extend_from_slice(&[0, ilen, 0, 0]);
        for _ in 0..(usize::from(count) + 1) * usize::from(ilen) {
            g.extend_from_slice(&fill);
        }
        g
    }

    const GREY: HsvShift = HsvShift {
        hue: 0,
        saturation: -0x100,
        value: 0,
    };

    #[test]
    fn a_record_s_leading_words_are_its_colours() {
        // A gouraud triangle group (flags 0x1C -> selector 0xE -> three
        // words) of two five-word records, then a flat group.
        let mut block = group(2, 0x1C, 5, [0x80, 0x40, 0x20, 0x30]);
        block.extend(group(1, 0x18, 4, [0x10, 0x90, 0x10, 0x20]));
        block.extend_from_slice(&[0; 4]);
        let n = shift_primitive_colours(&mut block, &GREY);
        assert_eq!(n, 2 * 3 + 1);
        for record in 0..2 {
            let at = 8 + record * 20;
            for w in 0..5 {
                let got = &block[at + w * 4..at + w * 4 + 4];
                if w < 3 {
                    assert_eq!(got, [0x80, 0x80, 0x80, 0x30], "record {record} word {w}");
                } else {
                    assert_eq!(got, [0x80, 0x40, 0x20, 0x30], "record {record} word {w}");
                }
            }
        }
        // The footer slot is not a record.
        assert_eq!(&block[8 + 40..8 + 44], [0x80, 0x40, 0x20, 0x30]);
        let flat = 8 + 60;
        assert_eq!(&block[flat + 8..flat + 12], [0x90, 0x90, 0x90, 0x20]);
        assert_eq!(&block[flat + 12..flat + 16], [0x10, 0x90, 0x10, 0x20]);
    }

    #[test]
    fn a_light_source_group_is_walked_and_left_alone() {
        let mut block = group(3, 0x14, 4, [0xAA, 0xBB, 0xCC, 0xDD]);
        block.extend(group(1, 0x1A, 4, [0xFF, 0x00, 0x00, 0x24]));
        block.extend_from_slice(&[0; 4]);
        let before = block.clone();
        assert_eq!(shift_primitive_colours(&mut block, &GREY), 1);
        let second = 8 + 4 * 16;
        assert_eq!(block[..second + 8], before[..second + 8]);
        // A grey of the word's max, capped at `0xF8`.
        assert_eq!(&block[second + 8..second + 12], [0xF8, 0xF8, 0xF8, 0x24]);
    }

    #[test]
    fn a_tmd_s_objects_are_each_walked() {
        let mut tmd = vec![0u8; TMD_HEADER + 2 * TMD_OBJECT];
        tmd[0..4].copy_from_slice(&0x8000_0002u32.to_le_bytes());
        tmd[8..12].copy_from_slice(&2u32.to_le_bytes());
        for obj in 0..2 {
            let prim_top = (tmd.len() - TMD_HEADER) as u32;
            let at = TMD_HEADER + obj * TMD_OBJECT + 0x10;
            tmd[at..at + 4].copy_from_slice(&prim_top.to_le_bytes());
            tmd.extend(group(2, 0x20, 4, [0x20, 0x60, 0x40, 0x20]));
            tmd.extend_from_slice(&[0; 4]);
        }
        let mut second = tmd.clone();
        let n = shift_resident_tmds([tmd.as_mut_slice(), second.as_mut_slice()], &GREY);
        assert_eq!(n, 8);
        assert_eq!(tmd, second);
    }
}
