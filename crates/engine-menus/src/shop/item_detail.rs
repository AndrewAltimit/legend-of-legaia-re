//! The item-detail line under an item's name: `FUN_800337B0`, the printer
//! the shop's info window 34 (`FUN_801D4A80`) hands an item's description.
//!
//! For anything but equipment (item record `+0 != 1`) it prints the
//! description string and stops. For equipment it prints the description or,
//! when the record has none, a **slot template** (`0x80073E2C`, eight
//! pointers: per slot class a `[icon] <Slot>. For [0xC1 n].` form and a
//! shorter one for a piece every character wears), and then a **stat line**
//! one row down (`y + 0xD`): one or two label / value pairs picked by the
//! slot class, the labels from `0x8007B4B0..0x8007B4BC`, the values the
//! equipment record's own bonus bytes as 8x12 numerals.
//!
//! Every string is read from the user's `SCUS_942.54` at runtime; this
//! module holds the addresses and the selection logic only.
//!
//! PORT: FUN_800337B0

/// The template pointer table (`lui`-pair `-0x7FF8C1D4` = `0x80073E2C`):
/// eight words, slot class `c` at `c`, its all-characters form at `c + 4`.
pub const DETAIL_TEMPLATE_TABLE_VA: u32 = 0x8007_3E2C;
/// The four stat labels, 4 bytes apart: ATK, UDF, INT, LDF order
/// (`&DAT_8007B4B0` .. `&DAT_8007B4BC`).
pub const DETAIL_STAT_LABEL_VA: u32 = 0x8007_B4B0;
/// Item record table (`0x80074368`, 12-byte stride: `+0` kind, `+1` bonus
/// index, `+8` description pointer).
pub const ITEM_TABLE_VA: u32 = 0x8007_4368;
/// Equipment bonus table (`0x80074F68`, 8-byte stride).
pub const EQUIP_TABLE_VA: u32 = 0x8007_4F68;

/// Index into [`ItemDetailTables::stat_labels`].
pub const LABEL_ATK: usize = 0;
/// See [`LABEL_ATK`].
pub const LABEL_UDF: usize = 1;
/// See [`LABEL_ATK`].
pub const LABEL_INT: usize = 2;
/// See [`LABEL_ATK`].
pub const LABEL_LDF: usize = 3;

/// What [`item_detail_line`] reads off the executable.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemDetailTables {
    /// Per item id: record `+0` kind.
    pub kind: Vec<u8>,
    /// Per item id: the raw description bytes (escapes kept), empty for none.
    pub desc: Vec<Vec<u8>>,
    /// Per item id: the equipment bonus record its `+1` index names, for
    /// kind-`1` ids.
    pub equip: Vec<Option<[u8; 8]>>,
    /// The eight slot templates, raw.
    pub templates: [Vec<u8>; 8],
    /// ATK, UDF, INT, LDF.
    pub stat_labels: [String; 4],
}

/// One label / value pair of the stat line, offsets from the printer's pen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DetailStat {
    /// Index into [`ItemDetailTables::stat_labels`].
    pub label: usize,
    /// Label pen, `x` offset.
    pub label_dx: i32,
    /// Value field (3 cells, right-aligned) pen, `x` offset.
    pub value_dx: i32,
    /// The bonus byte.
    pub value: u8,
}

/// The detail line for one item.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemDetailLine {
    /// The text the first row prints, raw (sprite and name escapes kept).
    pub text: Vec<u8>,
    /// The stat line at [`DETAIL_STAT_DY`] below the text, empty for
    /// non-equipment.
    pub stats: Vec<DetailStat>,
}

/// The stat line's row offset below the printer's pen (`param_4 + 0xd`).
pub const DETAIL_STAT_DY: i32 = 0x0D;

/// The slot class `FUN_800337B0` derives from equipment `+7`
/// (`(b & 0x60) >> 5` remapped: `2` weapon -> `0`, `1` head -> `1`, `0` body
/// -> `2`, `3` legs -> `3`) - the template index and the stat-line arm.
pub fn detail_slot_class(b7: u8) -> usize {
    match (b7 & 0x60) >> 5 {
        2 => 0,
        1 => 1,
        0 => 2,
        _ => 3,
    }
}

/// Build item `id`'s detail line.
///
/// The `0xC1` operand a class template carries is patched the way the
/// printer patches its live copy (`0x80033900..0x80033988`): `1` when the
/// `+6` mask has bit `2`, else `2` for bit `4`, else the template's own `0`.
pub fn item_detail_line(t: &ItemDetailTables, id: u8) -> ItemDetailLine {
    let i = usize::from(id);
    let desc = t.desc.get(i).cloned().unwrap_or_default();
    let equip = match (t.kind.get(i), t.equip.get(i)) {
        (Some(1), Some(Some(e))) => *e,
        _ => {
            return ItemDetailLine {
                text: desc,
                stats: Vec::new(),
            };
        }
    };
    let class = detail_slot_class(equip[7]);
    let text = if !desc.is_empty() {
        desc
    } else if equip[6] & 7 == 7 {
        t.templates[class + 4].clone()
    } else {
        let mut s = t.templates[class].clone();
        if let Some(p) = s.iter().position(|&b| b == 0xC1)
            && let Some(op) = s.get_mut(p + 1)
        {
            *op = if equip[6] & 2 != 0 {
                1
            } else if equip[6] & 4 != 0 {
                2
            } else {
                *op
            };
        }
        s
    };
    let stat = |label, label_dx, value_dx, value| DetailStat {
        label,
        label_dx,
        value_dx,
        value,
    };
    // `0x800339B0..`: weapon prints one pair to the right, the rest two.
    let stats = match class {
        0 => vec![stat(LABEL_ATK, 0x50, 0x78, equip[1])],
        1 => vec![
            stat(LABEL_UDF, 0x0C, 0x34, equip[2]),
            stat(LABEL_INT, 0x56, 0x78, equip[0]),
        ],
        3 => vec![
            stat(LABEL_ATK, 0x0C, 0x34, equip[1]),
            stat(LABEL_LDF, 0x50, 0x78, equip[3]),
        ],
        _ => vec![
            stat(LABEL_UDF, 0x0C, 0x34, equip[2]),
            stat(LABEL_LDF, 0x50, 0x78, equip[3]),
        ],
    };
    ItemDetailLine { text, stats }
}

/// PS-X EXE VA -> file offset.
fn exe_off(scus: &[u8], va: u32) -> Option<usize> {
    if scus.get(0..8)? != b"PS-X EXE" {
        return None;
    }
    let t_addr = u32::from_le_bytes(scus.get(0x18..0x1C)?.try_into().ok()?);
    let off = va.checked_sub(t_addr)? as usize + 0x800;
    (off < scus.len()).then_some(off)
}

/// A NUL-terminated menu string at `va`, raw, keeping each two-byte escape
/// (`0xC1..=0xC7`, `0xCE`, `0xCF`) whole even when its operand is `0`.
fn read_raw(scus: &[u8], va: u32) -> Vec<u8> {
    let Some(start) = exe_off(scus, va) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut i = start;
    while let Some(&b) = scus.get(i) {
        match b {
            0 => break,
            0xC1..=0xC7 | 0xCE | 0xCF => {
                out.push(b);
                if let Some(&op) = scus.get(i + 1) {
                    out.push(op);
                }
                i += 2;
            }
            _ => {
                out.push(b);
                i += 1;
            }
        }
        if out.len() > 256 {
            break;
        }
    }
    out
}

fn read_u32(scus: &[u8], va: u32) -> Option<u32> {
    let o = exe_off(scus, va)?;
    Some(u32::from_le_bytes(scus.get(o..o + 4)?.try_into().ok()?))
}

impl ItemDetailTables {
    /// Read the tables out of a `SCUS_942.54` image. `None` when it is not a
    /// PS-X EXE.
    pub fn from_scus(scus: &[u8]) -> Option<Self> {
        exe_off(scus, ITEM_TABLE_VA)?;
        let mut t = Self::default();
        for id in 0..256u32 {
            let rec = ITEM_TABLE_VA + id * 12;
            let o = exe_off(scus, rec)?;
            let kind = *scus.get(o)?;
            let sub = *scus.get(o + 1)?;
            let dp = read_u32(scus, rec + 8).unwrap_or(0);
            t.kind.push(kind);
            t.desc.push(if dp == 0 {
                Vec::new()
            } else {
                read_raw(scus, dp)
            });
            t.equip.push(if kind == 1 {
                exe_off(scus, EQUIP_TABLE_VA + u32::from(sub) * 8)
                    .and_then(|e| scus.get(e..e + 8))
                    .and_then(|b| b.try_into().ok())
            } else {
                None
            });
        }
        for (k, slot) in t.templates.iter_mut().enumerate() {
            if let Some(p) = read_u32(scus, DETAIL_TEMPLATE_TABLE_VA + k as u32 * 4) {
                *slot = read_raw(scus, p);
            }
        }
        for (k, slot) in t.stat_labels.iter_mut().enumerate() {
            *slot = read_raw(scus, DETAIL_STAT_LABEL_VA + k as u32 * 4)
                .into_iter()
                .map(char::from)
                .collect();
        }
        Some(t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tables(kind: u8, desc: &[u8], equip: [u8; 8]) -> ItemDetailTables {
        let mut t = ItemDetailTables {
            kind: vec![kind],
            desc: vec![desc.to_vec()],
            equip: vec![(kind == 1).then_some(equip)],
            ..Default::default()
        };
        for (k, s) in t.templates.iter_mut().enumerate() {
            *s = vec![b'T', b'0' + k as u8, 0xC1, 0, b'.'];
        }
        t
    }

    #[test]
    fn a_non_equipment_item_prints_its_description_alone() {
        let l = item_detail_line(&tables(0, b"abc", [0; 8]), 0);
        assert_eq!(l.text, b"abc");
        assert!(l.stats.is_empty());
    }

    #[test]
    fn an_undescribed_weapon_takes_its_class_template_and_one_atk_pair() {
        // Weapon (`+7 = 0x40`) for Noa (`+6 = 2`), ATK 40.
        let l = item_detail_line(&tables(1, b"", [0, 40, 0, 0, 0, 0x40, 2, 0x40]), 0);
        assert_eq!(l.text, [b'T', b'0', 0xC1, 1, b'.']);
        assert_eq!(
            l.stats,
            vec![DetailStat {
                label: LABEL_ATK,
                label_dx: 0x50,
                value_dx: 0x78,
                value: 40
            }]
        );
    }

    #[test]
    fn an_all_character_piece_takes_the_short_template() {
        // Body (`+7 = 0`), every character.
        let l = item_detail_line(&tables(1, b"", [0, 0, 33, 34, 0, 0x40, 7, 0]), 0);
        assert_eq!(l.text, [b'T', b'6', 0xC1, 0, b'.']);
        let pairs: Vec<_> = l.stats.iter().map(|s| (s.label, s.value)).collect();
        assert_eq!(pairs, vec![(LABEL_UDF, 33), (LABEL_LDF, 34)]);
    }

    #[test]
    fn a_described_piece_keeps_its_text_and_still_prints_stats() {
        let l = item_detail_line(&tables(1, b"x", [16, 0, 23, 0, 0, 0x40, 1, 0x20]), 0);
        assert_eq!(l.text, b"x");
        let pairs: Vec<_> = l.stats.iter().map(|s| (s.label, s.value)).collect();
        assert_eq!(pairs, vec![(LABEL_UDF, 23), (LABEL_INT, 16)]);
    }

    #[test]
    fn legs_print_atk_then_ldf() {
        let l = item_detail_line(&tables(1, b"", [0, 40, 0, 37, 0x10, 0x40, 1, 0x60]), 0);
        let pairs: Vec<_> = l.stats.iter().map(|s| (s.label, s.value)).collect();
        assert_eq!(pairs, vec![(LABEL_ATK, 40), (LABEL_LDF, 37)]);
    }
}
