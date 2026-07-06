//! Motion-VM (`FUN_80038158`) script table - MAN tail-section **1**.
//!
//! PORT: FUN_8003A9D4
//! REF: FUN_80038158, FUN_8003AEB0, FUN_8003BC08
//!
//! The per-actor motion / bytecode VM at `FUN_80038158` (dispatched by the
//! `_DAT_8007C354`-list tick `FUN_8003BC08` when actor `+0x10 & 0x80`) reads
//! its bytecode from `*(u32*)(actor + 0x80) + *(u16*)(actor + 0x84)`. That
//! pointer is installed at scene entry by `FUN_8003A9D4`, which walks the
//! MAN's **tail section 1** (the pointer `FUN_8003AEB0` leaves in the field
//! control block `*_DAT_801C6EA4`, i.e. `ctrl[+0x00]` - previously the
//! "(Open)" slot in [`man_section`](crate::man_section)'s section table).
//! This is the *second* bytecode VM that writes the system story-flag bank
//! `DAT_80085758` (ops `7`/`8`), invisible to the MAN field-VM
//! (`0x50/0x60/0x70`) flag census.
//!
//! ## Section-1 body layout (`FUN_8003A9D4`)
//!
//! A chain of **motion records**, terminated by a zero count byte:
//!
//! ```text
//! +0        u8   count           ; 0 = end of table
//! +1        s16  next_delta      ; byte delta from THIS record to the next
//! +3        count x [u8 actor_id, u8 enable]
//! +3+2n     motion stream        ; shared by every bound actor
//! ```
//!
//! `actor_id` selects who the stream is bound to: `0xF8` = the player
//! (`_DAT_8007C364`), `0xFB` = the first `_DAT_8007C34C`-list node whose tick
//! is the world-map entity SM (`FUN_801DA51C`), anything else = the
//! `_DAT_8007C354` field actor whose `+0x50` (placement index) matches. The
//! `enable` byte lands in actor `+0x8A` (bit 0 gates the whole VM tick).
//!
//! ## Motion stream layout (`FUN_80038158` preamble)
//!
//! The stream opens with a **variant header table**: records of
//! `[u16 selector][s16 delta]`, walked by adding `delta` to the current
//! header position. The VM picks the *first* variant whose selector flag
//! (`selector & 0xFFF`, in the `DAT_80085758` system bank) is **set**;
//! `0xFFFF` terminates the table and doubles as the always-match default
//! variant. The chosen variant's bytecode begins at its header `+4`; the VM
//! re-evaluates the table every tick, so flag changes swap variants live.
//!
//! ## Opcode width table (`FUN_80038158` switch)
//!
//! | op | width | effect |
//! |----|-------|--------|
//! | 0x01 | 1 | end / loop back to the variant's first opcode |
//! | 0x02 | 3 | set anim/timer pair `+0x88/+0x5C` from u16 operand |
//! | 0x03 | 3 | directional step (facing table `DAT_80073F04`) |
//! | 0x04 | 3 | facing ramp toward table direction |
//! | 0x05 | 2 | wait `operand` ticks |
//! | 0x06 | 5 | pad-echo / bounded chase step |
//! | 0x07 | 3 | **SET system flag** `u16 LE operand` in `DAT_80085758` |
//! | 0x08 | 3 | **CLEAR system flag** `u16 LE operand` |
//! | 0x09 | 3 | post u16 to the 4-slot ring `DAT_8007B6D8` (`FUN_80035B50`) |
//! | 0x0A | 3 | set actor flag `0x1000000` + op-2 body |
//! | 0x0B | 3 | clear actor flag `0x1000000` + op-2 body |
//! | 0x0C | 8 | glide channel install (u24 target + u16 + u16 duration) |
//! | 0x0D | 4 | facing ramp with tween-channel install |
//! | 0x0E | 3 | swap model (kingdom pool `DAT_8007B6F8` / `_DAT_8007B824`) |
//! | 0x0F | 3 | teleport to tile (same `(b&0x7F)*0x80+0x40` grid decode) |
//! | 0x10 | 2 | set bit in `+0x10`/`+0x12`/`+0x62`/scratch flag words |
//! | 0x11 | 2 | clear bit (same targets) |
//! | 0x12 | 2 | wait for bit state (same targets) |
//! | 0x13 | 13 | `FUN_80058490` call (4 x u16 + 2 x u16 params) |
//! | 0x14 | 5 | set / tween `+0x72` (speed) |
//! | 0x15 | 5 | set / tween `+0x24` |
//! | 0x16 | 5 | set / tween `+0x28` |
//! | 0x17 | 3 | write per-actor pause-table pair (`0x801C6470`) |
//! | 0x18 | 5 | bounded wander inside an AABB (4 tile bytes) |
//! | 0x19 | 3 | directional step variant (shares the op-3 body) |
//! | 0x20 | 3 | directional step variant, half step (shares op-3 body) |
//!
//! Every other opcode value hangs the retail interpreter (no switch case
//! advances the cursor), so the walker treats it as a decode stop. Across the
//! whole retail scene corpus the linear decode hits **zero** unknown opcodes
//! and every record chain lands exactly on the section terminator.

use crate::man_section::ManFile;

/// `actor_id` binding for the player actor (`_DAT_8007C364`).
pub const ACTOR_PLAYER: u8 = 0xF8;

/// `actor_id` binding for the world-map entity-SM node (the first
/// `_DAT_8007C34C` node ticked by `FUN_801DA51C`).
pub const ACTOR_WORLD_ENTITY: u8 = 0xFB;

/// Variant-selector value that terminates the header table and acts as the
/// always-match default variant.
pub const SELECTOR_DEFAULT: u16 = 0xFFFF;

/// Total byte width of opcode `op` (opcode byte included), or `None` for a
/// value the retail interpreter has no case for (a decode stop).
pub fn op_width(op: u8) -> Option<usize> {
    Some(match op {
        0x01 => 1,
        0x05 | 0x10 | 0x11 | 0x12 => 2,
        0x02 | 0x03 | 0x04 | 0x07 | 0x08 | 0x09 | 0x0A | 0x0B | 0x0E | 0x0F | 0x17 | 0x19
        | 0x20 => 3,
        0x0D => 4,
        0x06 | 0x14 | 0x15 | 0x16 | 0x18 => 5,
        0x0C => 8,
        0x13 => 13,
        _ => return None,
    })
}

/// One `[u8 actor_id][u8 enable]` binding from a motion record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MotionBinding {
    /// `0xF8` player / `0xFB` world-map entity / else placement index
    /// matched against field-actor `+0x50`.
    pub actor_id: u8,
    /// Written to actor `+0x8A`; bit 0 gates the motion-VM tick.
    pub enable: u8,
}

/// One motion record from the section-1 chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MotionRecord {
    /// Chain index (0-based).
    pub index: usize,
    /// Absolute MAN byte offset of the record's count byte.
    pub offset: usize,
    /// The actors this stream is installed on.
    pub bindings: Vec<MotionBinding>,
    /// Absolute MAN byte offset of the motion stream (`offset + 3 + 2n`) -
    /// the value retail stores at actor `+0x80`.
    pub stream_offset: usize,
    /// One past the record's last byte (`offset + next_delta`, clamped to
    /// the section end). The default variant's bytecode is bounded by this.
    pub end_offset: usize,
}

/// One `[u16 selector][s16 delta]` variant from a stream's header table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MotionVariant {
    /// 0-based position in the header table.
    pub index: usize,
    /// Absolute MAN byte offset of the variant header.
    pub header_offset: usize,
    /// Raw selector. `0xFFFF` = default; else `selector & 0xFFF` is the
    /// gating system-flag id (variant runs while the flag is SET).
    pub selector: u16,
    /// Absolute MAN byte offset of the variant's first opcode
    /// (`header_offset + 4`).
    pub code_offset: usize,
    /// One past the variant's last bytecode byte.
    pub code_end: usize,
}

impl MotionVariant {
    /// `true` for the `0xFFFF` always-match default variant.
    pub fn is_default(&self) -> bool {
        self.selector == SELECTOR_DEFAULT
    }

    /// The system-flag id gating this variant (`None` for the default).
    pub fn gate_flag(&self) -> Option<u16> {
        (!self.is_default()).then_some(self.selector & 0xFFF)
    }
}

/// SET (`op 0x07`) vs CLEAR (`op 0x08`) discriminator for a flag site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionFlagKind {
    Set,
    Clear,
}

/// One system-flag write found in a motion stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MotionFlagSite {
    /// Motion-record chain index.
    pub record: usize,
    /// Variant index within the record's header table.
    pub variant: usize,
    /// The variant's gating flag (`None` = default variant).
    pub gate: Option<u16>,
    /// Absolute MAN byte offset of the opcode byte.
    pub offset: usize,
    /// The `DAT_80085758` system-flag id (u16 LE operand).
    pub flag: u16,
    pub kind: MotionFlagKind,
}

fn u16_le(man: &[u8], pos: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*man.get(pos)?, *man.get(pos + 1)?]))
}

fn s16_le(man: &[u8], pos: usize) -> Option<i16> {
    Some(i16::from_le_bytes([*man.get(pos)?, *man.get(pos + 1)?]))
}

/// Walk the section-1 motion-record chain (`FUN_8003A9D4`).
///
/// Returns an empty vec when section 1 is the chain terminator or the body
/// bytes are malformed (the retail installer would equally read a zero count
/// and stop). Records with a non-positive `next_delta` end the walk - the
/// retail chain only ever advances forward.
pub fn motion_records(man: &[u8], man_file: &ManFile) -> Vec<MotionRecord> {
    let sec = &man_file.sections[1];
    if sec.is_terminator() {
        return Vec::new();
    }
    let body_start = sec.body_offset();
    let end = sec.end_offset().min(man.len());
    let mut out = Vec::new();
    let mut p = body_start;
    let mut index = 0usize;
    while p < end {
        let count = man[p] as usize;
        if count == 0 {
            break;
        }
        let Some(delta) = s16_le(man, p + 1) else {
            break;
        };
        if delta <= 0 {
            break;
        }
        let stream_offset = p + 3 + 2 * count;
        if stream_offset > end {
            break;
        }
        let mut bindings = Vec::with_capacity(count);
        for i in 0..count {
            let b = p + 3 + 2 * i;
            bindings.push(MotionBinding {
                actor_id: man[b],
                enable: man[b + 1],
            });
        }
        let next = p + delta as usize;
        out.push(MotionRecord {
            index,
            offset: p,
            bindings,
            stream_offset,
            end_offset: next.min(end),
        });
        p = next;
        index += 1;
    }
    out
}

/// Walk a motion stream's variant header table (`FUN_80038158` preamble).
///
/// Stops at the `0xFFFF` default (included in the result), a non-positive
/// delta, or the record end. The default variant's bytecode is bounded by
/// the record end; a gated variant's by its own delta (the next header).
pub fn stream_variants(man: &[u8], rec: &MotionRecord) -> Vec<MotionVariant> {
    let mut out = Vec::new();
    let mut v = rec.stream_offset;
    for index in 0..64 {
        let Some(selector) = u16_le(man, v) else {
            break;
        };
        let Some(delta) = s16_le(man, v + 2) else {
            break;
        };
        let code_end = if selector == SELECTOR_DEFAULT {
            rec.end_offset
        } else {
            (v + delta.max(0) as usize).min(rec.end_offset)
        };
        out.push(MotionVariant {
            index,
            header_offset: v,
            selector,
            code_offset: v + 4,
            code_end,
        });
        if selector == SELECTOR_DEFAULT || delta <= 0 {
            break;
        }
        v += delta as usize;
    }
    out
}

/// Collect every op-`0x07`/`0x08` system-flag write in every motion record
/// of `man`'s tail section 1.
///
/// The per-variant decode is linear from `code_offset` using [`op_width`]
/// and does **not** stop at op-`0x01` (the loop-back), so bytes the
/// interpreter parks behind an end marker are still surveyed; on the retail
/// corpus the greedy and reachable-only walks agree exactly, and no stream
/// hits an unknown opcode.
pub fn motion_flag_sites(man: &[u8], man_file: &ManFile) -> Vec<MotionFlagSite> {
    let mut out = Vec::new();
    for rec in motion_records(man, man_file) {
        for var in stream_variants(man, &rec) {
            let mut pc = var.code_offset;
            while pc < var.code_end {
                let op = man[pc];
                let Some(w) = op_width(op) else {
                    break;
                };
                if (op == 0x07 || op == 0x08)
                    && let Some(flag) = u16_le(man, pc + 1)
                {
                    out.push(MotionFlagSite {
                        record: rec.index,
                        variant: var.index,
                        gate: var.gate_flag(),
                        offset: pc,
                        flag,
                        kind: if op == 0x07 {
                            MotionFlagKind::Set
                        } else {
                            MotionFlagKind::Clear
                        },
                    });
                }
                pc += w;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::man_section;

    /// Minimal MAN whose tail section 1 carries `body` as its motion table.
    fn build_man_with_motion_section(body: &[u8]) -> Vec<u8> {
        let mut buf = vec![0u8; 0x2B];
        // No partition records; u24_at_28 = 0 puts section 0 right at the
        // data region.
        // Section 0: empty encounter section (len 0 would terminate the
        // chain per SectionRef::is_terminator? No - the chain keeps walking
        // regardless; only parse() cares about bounds). Use a 6-byte body.
        let s0_body = [0u8, 0, 0, 0, 0, 0];
        buf.extend_from_slice(&[s0_body.len() as u8, 0, 0]);
        buf.extend_from_slice(&s0_body);
        // Section 1: the motion table under test.
        let len = body.len() as u32;
        buf.extend_from_slice(&[
            (len & 0xFF) as u8,
            ((len >> 8) & 0xFF) as u8,
            ((len >> 16) & 0xFF) as u8,
        ]);
        buf.extend_from_slice(body);
        // Sections 2..=4 empty + terminator.
        for _ in 2..6 {
            buf.extend_from_slice(&[0, 0, 0]);
        }
        buf
    }

    /// Two records:
    ///  - record 0 binds actor 0x26; gated variant (flag 0x23E) whose code
    ///    SETs flag 549 then ENDs; default variant that CLEARs flag 7.
    ///  - record 1 binds the player (0xF8) + actor 3; default variant only,
    ///    movement ops but no flag write.
    fn motion_body() -> Vec<u8> {
        let mut b = Vec::new();
        // record 0: count=1, delta = filled below, bind (0x26, 0x00)
        let rec0 = b.len();
        b.extend_from_slice(&[1, 0, 0, 0x26, 0x00]);
        // variant 0: selector 0x023E, delta to next header
        let var0 = b.len();
        b.extend_from_slice(&[0x3E, 0x02, 0, 0]);
        b.extend_from_slice(&[0x07, 0x25, 0x02]); // SET 549
        b.push(0x01); // END
        let var1 = b.len();
        let var0_delta = (var1 - var0) as u16;
        b[var0 + 2..var0 + 4].copy_from_slice(&var0_delta.to_le_bytes());
        // variant 1: default
        b.extend_from_slice(&[0xFF, 0xFF, 0x07, 0x00]);
        b.extend_from_slice(&[0x08, 0x07, 0x00]); // CLEAR 7
        b.push(0x01);
        let rec1 = b.len();
        let rec0_delta = (rec1 - rec0) as u16;
        b[rec0 + 1..rec0 + 3].copy_from_slice(&rec0_delta.to_le_bytes());
        // record 1: count=2, binds (0xF8, 0x01), (0x03, 0x01)
        b.extend_from_slice(&[2, 0, 0, ACTOR_PLAYER, 0x01, 0x03, 0x01]);
        b.extend_from_slice(&[0xFF, 0xFF, 0x0B, 0x00]);
        b.extend_from_slice(&[0x17, 0x0B, 0x0C]); // pause pair
        b.extend_from_slice(&[0x05, 0x10]); // wait
        b.extend_from_slice(&[0x18, 0x01, 0x02, 0x03, 0x04]); // wander box
        b.push(0x01);
        let end = b.len();
        let rec1_delta = (end - rec1) as u16;
        b[rec1 + 1..rec1 + 3].copy_from_slice(&rec1_delta.to_le_bytes());
        // chain terminator
        b.push(0);
        b
    }

    #[test]
    fn decodes_records_bindings_and_variants() {
        let man = build_man_with_motion_section(&motion_body());
        let mf = man_section::parse(&man).expect("parse");
        let recs = motion_records(&man, &mf);
        assert_eq!(recs.len(), 2);
        assert_eq!(
            recs[0].bindings,
            vec![MotionBinding {
                actor_id: 0x26,
                enable: 0
            }]
        );
        assert_eq!(recs[1].bindings.len(), 2);
        assert_eq!(recs[1].bindings[0].actor_id, ACTOR_PLAYER);
        assert_eq!(recs[0].stream_offset, recs[0].offset + 5);
        assert_eq!(recs[1].stream_offset, recs[1].offset + 7);

        let v0 = stream_variants(&man, &recs[0]);
        assert_eq!(v0.len(), 2);
        assert_eq!(v0[0].gate_flag(), Some(0x23E));
        assert!(v0[1].is_default());
        let v1 = stream_variants(&man, &recs[1]);
        assert_eq!(v1.len(), 1);
        assert!(v1[0].is_default());
    }

    #[test]
    fn finds_set_and_clear_flag_sites() {
        let man = build_man_with_motion_section(&motion_body());
        let mf = man_section::parse(&man).expect("parse");
        let sites = motion_flag_sites(&man, &mf);
        assert_eq!(sites.len(), 2);
        assert_eq!(sites[0].flag, 549);
        assert_eq!(sites[0].kind, MotionFlagKind::Set);
        assert_eq!(sites[0].record, 0);
        assert_eq!(sites[0].variant, 0);
        assert_eq!(sites[0].gate, Some(0x23E));
        assert_eq!(sites[1].flag, 7);
        assert_eq!(sites[1].kind, MotionFlagKind::Clear);
        assert_eq!(sites[1].gate, None);
    }

    #[test]
    fn empty_or_terminator_section_yields_nothing() {
        let man = build_man_with_motion_section(&[0]);
        let mf = man_section::parse(&man).expect("parse");
        assert!(motion_records(&man, &mf).is_empty());
        assert!(motion_flag_sites(&man, &mf).is_empty());
    }

    #[test]
    fn width_table_matches_retail_switch() {
        // Spot-check the documented widths.
        assert_eq!(op_width(0x01), Some(1));
        assert_eq!(op_width(0x05), Some(2));
        assert_eq!(op_width(0x07), Some(3));
        assert_eq!(op_width(0x0C), Some(8));
        assert_eq!(op_width(0x0D), Some(4));
        assert_eq!(op_width(0x13), Some(13));
        assert_eq!(op_width(0x18), Some(5));
        assert_eq!(op_width(0x20), Some(3));
        // Values without a retail switch case are decode stops.
        assert_eq!(op_width(0x00), None);
        assert_eq!(op_width(0x1A), None);
        assert_eq!(op_width(0x21), None);
        assert_eq!(op_width(0xFF), None);
    }
}
