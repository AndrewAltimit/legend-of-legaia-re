//! `switch` jump tables in any overlay image, found from their dispatch.
//!
//! The compiler that built every Legaia overlay lowers a dense C `switch` to
//! one idiom, and the idiom states the table's address **and** its extent:
//!
//! ```text
//! sltiu $c, $i, ARMS        ; bound the index
//! beqz  $c, default
//! sll   $s, $i, 2           ; scale it
//! lui   $b, hi              ; form the table base ...
//! addiu $b, $b, lo          ; ... (or fold `lo` into the lw below)
//! addu  $b, $b, $s
//! lw    $t, off($b)
//! jr    $t
//! ```
//!
//! So a table is read off its **consumer**: the base is the `lui`
//! (`+ addiu` / `+ lw` displacement) the dispatch forms, and the length is the
//! `sltiu` bound times four. Nothing is scanned out of the table's own bytes.
//! [`crate::battle_jump_tables`] pins PROT 0898's twenty-two head tables row by
//! row; this finds every one of them without a row list, plus the two just
//! above that head, and every other overlay's tables by the same rule.
//!
//! A candidate is kept only when every one of its `ARMS` words is a
//! word-aligned VA inside the image's own content: a table whose arms land
//! elsewhere (in a co-resident image) is not this image's to claim, and a
//! dispatch that sits in an [inherited tail](crate::inherited_tail) belongs to
//! the donor.

/// One `switch` table and the dispatch that reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwitchTable {
    /// VA of arm 0.
    pub va: u32,
    /// Arm count: the dispatch's `sltiu` bound.
    pub arms: u32,
    /// VA of the dispatch `jr`.
    pub jr: u32,
    /// VA of the `lui` that forms the base.
    pub base_site: u32,
}

impl SwitchTable {
    /// Byte length (`4 * arms`).
    pub const fn byte_len(&self) -> usize {
        self.arms as usize * 4
    }
}

/// How far back from the `jr` the dispatch idiom is searched for.
const WINDOW: u32 = 20;

fn word(image: &[u8], off: usize) -> Option<u32> {
    image
        .get(off..off + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// The general-purpose register an instruction writes, for the instruction
/// classes the idiom is made of plus the ones that could clobber it on the way.
fn dest(w: u32) -> Option<u32> {
    let op = w >> 26;
    let rt = (w >> 16) & 31;
    let rd = (w >> 11) & 31;
    match op {
        0 => match w & 0x3F {
            // jr / jalr / syscall / break / mthi / mtlo / mult* / div*
            0x08 | 0x09 | 0x0C | 0x0D | 0x11 | 0x13 | 0x18..=0x1B => None,
            _ => Some(rd),
        },
        0x08..=0x0F | 0x20..=0x26 => Some(rt),
        _ => None,
    }
}

/// Find every `switch` table the code in `image[..own_end]` dispatches
/// through, for an image linked at `base`.
pub fn find(image: &[u8], base: u32, own_end: usize) -> Vec<SwitchTable> {
    let own_end = own_end.min(image.len());
    let end_va = base + own_end as u32;
    let mut out = Vec::new();
    for jr_off in (0..own_end.saturating_sub(3)).step_by(4) {
        let Some(w) = word(image, jr_off) else { break };
        // jr rs (not $ra)
        if w & 0xFC1F_FFFF != 0x0000_0008 {
            continue;
        }
        let target = (w >> 21) & 31;
        if target == 31 {
            continue;
        }
        let at = |k: u32| -> Option<u32> {
            let off = jr_off.checked_sub(4 * k as usize)?;
            word(image, off)
        };
        // lw target, disp(rb)
        let Some((k_lw, rb, disp)) = (1..WINDOW).find_map(|k| {
            let w = at(k)?;
            (w >> 26 == 0x23 && (w >> 16) & 31 == target)
                .then(|| (k, (w >> 21) & 31, i32::from(w as u16 as i16)))
        }) else {
            continue;
        };
        // addu rb, p, q
        let Some((k_addu, p, q)) = (k_lw + 1..WINDOW).find_map(|k| {
            let w = at(k)?;
            (w >> 26 == 0 && w & 0x3F == 0x21 && (w >> 11) & 31 == rb).then_some((
                k,
                (w >> 21) & 31,
                (w >> 16) & 31,
            ))
        }) else {
            continue;
        };
        let mut hi: Option<(u32, u32)> = None;
        let mut lo: i32 = 0;
        let mut idx: Option<u32> = None;
        for src in [p, q] {
            let mut k = k_addu + 1;
            while k < WINDOW {
                let Some(w) = at(k) else { break };
                if dest(w) != Some(src) {
                    k += 1;
                    continue;
                }
                let op = w >> 26;
                if op == 0x0F {
                    hi = Some((w & 0xFFFF, jr_off as u32 - 4 * k));
                } else if op == 0x09 && (w >> 21) & 31 == src && hi.is_none() {
                    lo = i32::from(w as u16 as i16);
                    k += 1;
                    continue;
                } else if op == 0 && w & 0x3F == 0 && (w >> 6) & 31 == 2 {
                    idx = Some((w >> 16) & 31);
                } else if op == 0 && w & 0x3F == 0x04 {
                    // sllv idx, idx, $s where $s was set to 2 in the window
                    // (PROT 0898's 27-arm dispatch at 0x801E7168 scales so).
                    let sh = (w >> 21) & 31;
                    let two = (k + 1..WINDOW).find_map(|j| {
                        let v = at(j)?;
                        (dest(v) == Some(sh)).then_some(v)
                    });
                    if two.is_some_and(|v| {
                        matches!(v >> 26, 0x09 | 0x0D) && (v >> 21) & 31 == 0 && v & 0xFFFF == 2
                    }) {
                        idx = Some((w >> 16) & 31);
                    }
                }
                break;
            }
        }
        let (Some((hi, lui_off)), Some(idx)) = (hi, idx) else {
            continue;
        };
        let va = (hi << 16).wrapping_add(lo as u32).wrapping_add(disp as u32);
        // sltiu c, idx, ARMS
        let Some(arms) = (k_addu + 1..WINDOW).find_map(|k| {
            let w = at(k)?;
            (w >> 26 == 0x0B && (w >> 21) & 31 == idx).then_some(w & 0xFFFF)
        }) else {
            continue;
        };
        if arms == 0 || va < base || va % 4 != 0 {
            continue;
        }
        let off = (va - base) as usize;
        let Some(len) = (arms as usize).checked_mul(4) else {
            continue;
        };
        if off + len > own_end {
            continue;
        }
        let in_image = (0..arms as usize).all(|i| {
            word(image, off + 4 * i).is_some_and(|a| a % 4 == 0 && (base..end_va).contains(&a))
        });
        if !in_image {
            continue;
        }
        let t = SwitchTable {
            va,
            arms,
            jr: base + jr_off as u32,
            base_site: base + lui_off,
        };
        if !out.contains(&t) {
            out.push(t);
        }
    }
    out.sort_by_key(|t| (t.va, t.jr));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enc_i(op: u32, rs: u32, rt: u32, imm: u16) -> u32 {
        (op << 26) | (rs << 21) | (rt << 16) | u32::from(imm)
    }

    #[test]
    fn finds_a_table_from_its_dispatch() {
        let base = 0x8010_0000u32;
        // v0 = 2, v1 = 3, a0 = 4
        let code = [
            enc_i(0x0B, 4, 2, 3),                     // sltiu v0, a0, 3
            enc_i(0x04, 2, 0, 8),                     // beqz v0, ...
            (4 << 16) | (3 << 11) | (2 << 6),         // sll v1, a0, 2
            enc_i(0x0F, 0, 2, 0x8010),                // lui v0, 0x8010
            enc_i(0x09, 2, 2, 0x0040),                // addiu v0, v0, 0x40
            (2 << 21) | (3 << 16) | (2 << 11) | 0x21, // addu v0, v0, v1
            enc_i(0x23, 2, 2, 0),                     // lw v0, 0(v0)
            (2 << 21) | 0x08,                         // jr v0
            0,
        ];
        let mut img = vec![0u8; 0x60];
        for (i, w) in code.iter().enumerate() {
            img[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
        }
        for i in 0..3u32 {
            let a = base + 4 * i;
            img[0x40 + 4 * i as usize..0x44 + 4 * i as usize].copy_from_slice(&a.to_le_bytes());
        }
        let t = find(&img, base, img.len());
        assert_eq!(
            t,
            vec![SwitchTable {
                va: base + 0x40,
                arms: 3,
                jr: base + 0x1C,
                base_site: base + 0x0C,
            }]
        );
        // An arm outside the image rejects the table.
        img[0x44..0x48].copy_from_slice(&0x8000_0000u32.to_le_bytes());
        assert!(find(&img, base, img.len()).is_empty());
    }
}
