//! **Fortress encounter fix** - restores random battles in Jette's Absolute
//! Fortress on a save that carries the Floating Castle's stale "no battles"
//! flag.
//!
//! ## The retail defect
//!
//! Zora's Floating Castle (`taiku`) arms a six-minute escape clock with field-VM
//! op `4C D3`. Its clock actor (`0x801D2EBC` in the field overlay) raises
//! system flag [`STALE_FLAG`] (`0x15`) through `FUN_8003CE08` whenever less
//! than a minute remains, and the castle's encounter table opens with a
//! flag-`0x15` group that is one whole-map rate-0 row: no random battles in the
//! last minute. The record that ends a successful escape stops the clock and
//! never clears the flag, so a file that ever dipped under a minute keeps it in
//! its saved flag bank for the rest of the game.
//!
//! Both Fortress scenes - `deroa` (the entrance, PROT entry
//! [`DEROA_ENTRY`]) and `chitei2` (the interior, PROT entry
//! [`CHITEI2_ENTRY`]) - open their encounter condition list with a byte copy of
//! that group. On an affected file the whole Fortress is silent and the Door of
//! Light is greyed out inside it (the row's `+8` byte has bit `0x80` clear).
//! Full trace: `docs/reference/glitches/field-oddities.md`.
//!
//! ## The edit
//!
//! A scene's region array is a run of story-state groups, and the condition
//! list (`[u16 flag][s16 region_count]` per record) says which flag selects
//! each; the roll `FUN_801D9E1C` takes the first record whose flag is set
//! (`docs/formats/encounter.md`). Retail's Fortress lists read:
//!
//! ```text
//! cond 0  flag 0x0015, 1 row   whole map, rate 0, both Doors blocked   <- the castle's group
//! cond 1  flag 0x0521, N rows  whole map, rate 0, Door of Light allowed (after Jette)
//! ...
//! cond L  0xFFFF               the default rooms
//! ```
//!
//! The fix turns group 0 into a **duplicate of group 1**: condition 0's flag id
//! becomes group 1's flag ([`POST_JETTE_FLAG`]) and its single region row
//! becomes a copy of group 1's whole-map row. The list then never tests flag
//! `0x15`:
//!
//! - flag `0x521` clear - condition 0 fails exactly as it does on a healthy
//!   retail file, and the walk carries on to the same later groups;
//! - flag `0x521` set - condition 0 wins with a row that reads the same as the
//!   group retail would have reached one record later.
//!
//! So a healthy file plays byte-for-byte the same Fortress, and an affected
//! file plays the healthy one. The castle's own tables are not touched: its
//! last-minute quiet window stays.
//!
//! ### Why a duplicate, not an unused flag
//!
//! Retargeting condition 0 at a flag "nothing ever sets" would also be
//! same-size, but it rests on a disc-wide negative - and the defect itself is
//! the counter-example: flag `0x15`'s writer is a native routine that receives
//! its flag id from script operands, which a census of script set ops and of
//! constant-id `FUN_8003CE08` callers both miss. The duplicate needs no such
//! proof. Its equivalence is local to the one table and is checked at plan
//! time: group 1 must resolve every tile to the same row payload as the row
//! written into group 0 (see [`plan`]).
//!
//! ### Same-size, in place
//!
//! Fourteen bytes change (two flag-id bytes, one 12-byte row) and nothing
//! moves, so the MAN is LZS re-packed into its own footprint like the
//! [encounter](crate::encounter) and [chest](crate::chest) edits. Neither
//! Fortress scene has a streaming variant MAN.
//!
//! ## What is deliberately not done
//!
//! The root cause - the castle's escape-success record `taiku` P2\[16\] never
//! issuing `60 15` - is left alone. The clear is two more bytes in a record
//! with no slack, and `taiku` has no LZS-packed bundle MAN: its only MAN is a
//! **raw** type-3 chunk of a `DATA_FIELD` stream (PROT entry 373) with three
//! more chunks behind it, which the patcher rewrites same-size only
//! ([`crate::encounter::SceneEncounters::locate_streaming_mans`]). Growing it
//! means re-laying the stream, machinery no other feature has. Outside the
//! castle the two Fortress scenes are the only ones whose condition list names
//! flag `0x15` (asserted disc-wide by the `fortress_encounter_fix_real`
//! oracle), and the field-VM flag census finds no script that tests it, so the
//! edit above removes the symptom entirely: the flag stays set in an affected
//! save and nothing reads it.
//!
//! The only disc values named here are the two flag ids and the 12-byte row the
//! fix verifies before writing - the same footing as the stock words a hook mod
//! checks. The row it writes is read from the user's own disc.

use anyhow::{Context, Result, bail};

use legaia_scene_formats::man_section::{self, CONDITION_DEFAULT_FLAG, ConditionRecord};

/// System flag the Floating Castle's escape clock raises under 1:00 and never
/// clears (RAM byte `0x8008575A` bit `0x04`; save block `+0x161A` bit `0x04`).
pub const STALE_FLAG: u16 = 0x0015;

/// System flag set after Jette and the Cort scene (`chitei2` P2\[15\] /
/// P2\[19\]); selects each Fortress scene's intended whole-map rate-0 group.
pub const POST_JETTE_FLAG: u16 = 0x0521;

/// PROT entry of the `deroa` scene bundle (the Fortress entrance).
pub const DEROA_ENTRY: usize = 606;
/// PROT entry of the `chitei2` scene bundle (the Fortress interior).
pub const CHITEI2_ENTRY: usize = 706;

/// The two scene bundles the fix edits: `(PROT entry, scene name)`.
pub const FORTRESS_SCENES: [(usize, &str); 2] =
    [(DEROA_ENTRY, "deroa"), (CHITEI2_ENTRY, "chitei2")];

/// Retail's group-0 region row in both Fortress scenes - the Floating Castle's
/// row with one stage bit changed: whole map `(0,0)..(128,128)`, rate 0,
/// formation slice `[0, +1)`, `+8 = 0x20` (stage 0, neither Door allowed), no
/// world-map return point.
pub const RETAIL_STALE_ROW: [u8; 12] = [
    0x00, 0x00, 0x80, 0x80, 0x00, 0x80, 0x00, 0x01, 0x20, 0x00, 0x00, 0x00,
];

/// Retail stride of a condition record.
const CONDITION_STRIDE: usize = 4;
/// Retail stride of a region record.
const REGION_STRIDE: usize = 12;

/// Which state a Fortress encounter table is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableState {
    /// Retail: group 0 is the flag-`0x15` row.
    Retail,
    /// Already fixed: group 0 duplicates group 1.
    Fixed,
}

/// One planned table edit, as absolute offsets into the decoded MAN.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableEdit {
    /// Offset of condition record 0 (its `u16 flag_id`).
    pub condition_off: usize,
    /// Offset of region row 0.
    pub region_off: usize,
    /// The row written over region row 0 - group 1's whole-map row.
    pub new_row: [u8; 12],
    /// Region index (in the flat array) the row was copied from.
    pub source_region: usize,
}

/// A located Fortress encounter table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located {
    /// Retail or already fixed.
    pub state: TableState,
    /// The edit that takes a retail table to the fixed one (also describes a
    /// fixed table: `new_row` is what region row 0 already holds).
    pub edit: TableEdit,
}

/// `true` when AABB `inner` lies inside `outer` (`[x_min, z_min, x_max, z_max]`).
fn aabb_within(inner: &[u8], outer: &[u8]) -> bool {
    inner[0] >= outer[0] && inner[1] >= outer[1] && inner[2] <= outer[2] && inner[3] <= outer[3]
}

/// Inspect a decoded Fortress MAN and work out the edit.
///
/// Accepts exactly two shapes and refuses everything else, so a disc whose
/// table is not retail's (or this fix's own output) is never written:
///
/// - **retail** - condition 0 is `{0x15, 1 row}` and that row is
///   [`RETAIL_STALE_ROW`];
/// - **fixed** - condition 0 is `{0x521, 1 row}` and that row equals group 1's
///   whole-map row.
///
/// In both, condition 1 must be `{0x521, N >= 1}`, and group 1 must *be* a
/// single whole-map row in effect: one of its rows has group 0's AABB, every
/// other row lies inside that box, and all of them carry the same payload
/// (`+4..+12`: rate, stage, formation slice, Door bits, return point). That is
/// the property that makes the duplicate exact - whichever of group 1's rows a
/// tile matches first, the roll reads the same bytes as it does from the copy.
/// The list must also still end in the unconditional `0xFFFF` record.
pub fn locate(man: &[u8]) -> Result<Located> {
    let file = man_section::parse(man).map_err(|e| anyhow::anyhow!("MAN does not parse: {e}"))?;
    let body_off = file.encounter_section().body_offset();
    let body = file
        .encounter_section_body(man)
        .context("MAN has no encounter section")?;
    let sec = man_section::parse_encounter_section(body)
        .map_err(|e| anyhow::anyhow!("encounter section does not parse: {e}"))?;
    if sec.condition_stride as usize != CONDITION_STRIDE
        || sec.region_stride as usize != REGION_STRIDE
    {
        bail!(
            "unexpected encounter strides (condition {}, region {})",
            sec.condition_stride,
            sec.region_stride
        );
    }
    let conds: Vec<ConditionRecord> = man_section::condition_records(body, &sec)
        .collect::<Option<_>>()
        .context("malformed condition record")?;
    if conds.len() < 3 {
        bail!(
            "expected at least 3 condition records, found {}",
            conds.len()
        );
    }
    if conds.last().map(|c| c.flag_id) != Some(CONDITION_DEFAULT_FLAG) {
        bail!("condition list does not end in the unconditional record");
    }
    let total: usize = conds.iter().map(|c| c.region_count as usize).sum();
    if total != sec.region_count as usize {
        bail!(
            "condition groups cover {total} regions, the table has {}",
            sec.region_count
        );
    }
    let (c0, c1) = (conds[0], conds[1]);
    if c0.region_count != 1 {
        bail!("group 0 has {} rows, expected 1", c0.region_count);
    }
    if c1.flag_id != POST_JETTE_FLAG || c1.region_count == 0 {
        bail!(
            "group 1 is flag {:#06x} with {} rows, expected flag {POST_JETTE_FLAG:#06x}",
            c1.flag_id,
            c1.region_count
        );
    }

    let regions = &body[sec.region_range.0..sec.region_range.1];
    let row = |i: usize| &regions[i * REGION_STRIDE..(i + 1) * REGION_STRIDE];
    let row0 = row(0);

    // Group 1 = rows 1..=N. Find its whole-map row and prove the group reads
    // as that single row.
    let group1 = 1..1 + c1.region_count as usize;
    let source_region = group1
        .clone()
        .find(|&i| row(i)[..4] == row0[..4])
        .context("group 1 has no row with group 0's whole-map box")?;
    let source = row(source_region);
    for i in group1 {
        if !aabb_within(&row(i)[..4], &source[..4]) || row(i)[4..] != source[4..] {
            bail!("group 1 is not a single whole-map row in effect (row {i} differs)");
        }
    }
    let mut new_row = [0u8; REGION_STRIDE];
    new_row.copy_from_slice(source);

    let state = if c0.flag_id == STALE_FLAG && row0 == RETAIL_STALE_ROW {
        TableState::Retail
    } else if c0.flag_id == POST_JETTE_FLAG && row0 == new_row {
        TableState::Fixed
    } else {
        bail!(
            "group 0 is neither retail's flag-{STALE_FLAG:#06x} row nor the fixed duplicate \
             (flag {:#06x}, row {row0:02X?})",
            c0.flag_id
        );
    };

    Ok(Located {
        state,
        edit: TableEdit {
            condition_off: body_off + sec.condition_range.0,
            region_off: body_off + sec.region_range.0,
            new_row,
            source_region,
        },
    })
}

/// Plan the fix for one decoded Fortress MAN.
///
/// Returns the rewritten MAN (same length), or `None` when the table is already
/// fixed. Errors - leaving the caller nothing to write - when the table is in
/// neither recognized shape (see [`locate`]).
pub fn plan(man: &[u8]) -> Result<Option<Vec<u8>>> {
    let located = locate(man)?;
    if located.state == TableState::Fixed {
        return Ok(None);
    }
    let edit = &located.edit;
    let mut out = man.to_vec();
    out[edit.condition_off..edit.condition_off + 2].copy_from_slice(&POST_JETTE_FLAG.to_le_bytes());
    out[edit.region_off..edit.region_off + REGION_STRIDE].copy_from_slice(&edit.new_row);
    // The rewrite must land on the shape `locate` calls fixed, or a re-apply
    // would refuse its own output.
    debug_assert_eq!(locate(&out).map(|l| l.state).ok(), Some(TableState::Fixed));
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The post-Jette whole-map row both Fortress scenes carry (synthetic
    /// payload; only its relation to the other rows matters here).
    const QUIET: [u8; 12] = [0, 0, 128, 128, 0, 0x81, 1, 1, 0xA0, 0x87, 66, 113];
    const ROOM: [u8; 12] = [0, 0, 43, 52, 18, 0x81, 0, 4, 0xA0, 0x87, 66, 113];
    const CATCH_ALL: [u8; 12] = [0, 0, 128, 128, 0, 0x81, 4, 4, 0xA2, 0x87, 66, 113];

    /// Minimal MAN: empty record partitions, section 0 = 2 formations + the
    /// given condition list and region rows, five empty tail sections.
    fn build_man(conditions: &[(u16, u16)], regions: &[[u8; 12]]) -> Vec<u8> {
        let mut buf = vec![0u8; man_section::RECORDS_BEGIN_OFFSET];
        let mut s0 = vec![8u8, 4, 12, 2];
        s0.extend_from_slice(&[0, 0, 0, 1, 4, 0, 0, 0]);
        s0.extend_from_slice(&[0, 0, 0, 2, 4, 4, 0, 0]);
        s0.push(conditions.len() as u8);
        for (flag, n) in conditions {
            s0.extend_from_slice(&flag.to_le_bytes());
            s0.extend_from_slice(&n.to_le_bytes());
        }
        s0.push(regions.len() as u8);
        for r in regions {
            s0.extend_from_slice(r);
        }
        let len = s0.len() as u32;
        buf.extend_from_slice(&len.to_le_bytes()[..3]);
        buf.extend_from_slice(&s0);
        buf.extend_from_slice(&[0u8; 15]);
        buf
    }

    /// `deroa`-shaped: group 1 is one row.
    fn entrance() -> Vec<u8> {
        build_man(
            &[(STALE_FLAG, 1), (POST_JETTE_FLAG, 1), (0xFFFF, 2)],
            &[RETAIL_STALE_ROW, QUIET, ROOM, CATCH_ALL],
        )
    }

    /// `chitei2`-shaped: group 1 is a pocket row inside its whole-map row.
    fn interior() -> Vec<u8> {
        let mut pocket = QUIET;
        pocket[..4].copy_from_slice(&[0, 89, 30, 126]);
        build_man(
            &[(STALE_FLAG, 1), (POST_JETTE_FLAG, 2), (0xFFFF, 2)],
            &[RETAIL_STALE_ROW, pocket, QUIET, ROOM, CATCH_ALL],
        )
    }

    /// The row the roll reads for `tile` under a flag bank (`None` = no row).
    fn winning_row(man: &[u8], flags: &[u16], tile: (u8, u8)) -> Option<[u8; 8]> {
        let file = man_section::parse(man).unwrap();
        let body = file.encounter_section_body(man).unwrap();
        let sec = man_section::parse_encounter_section(body).unwrap();
        let (first, count) = man_section::active_region_group(body, &sec, |f| flags.contains(&f))?;
        let regions = &body[sec.region_range.0..sec.region_range.1];
        (first..first + count)
            .map(|i| &regions[i * 12..(i + 1) * 12])
            .find(|r| tile.0 >= r[0] && tile.0 <= r[2] && tile.1 >= r[1] && tile.1 <= r[3])
            .map(|r| r[4..].try_into().unwrap())
    }

    #[test]
    fn retail_table_is_located_and_rewritten_same_size() {
        for man in [entrance(), interior()] {
            let located = locate(&man).unwrap();
            assert_eq!(located.state, TableState::Retail);
            assert_eq!(located.edit.new_row, QUIET);
            let fixed = plan(&man).unwrap().expect("retail table is edited");
            assert_eq!(fixed.len(), man.len());
            let diff = man.iter().zip(&fixed).filter(|(a, b)| a != b).count();
            assert!(diff > 0 && diff <= 14, "{diff} bytes differ");
            // Only condition 0's flag id and region row 0 moved.
            let e = &located.edit;
            let mut masked = fixed.clone();
            masked[e.condition_off..e.condition_off + 2].copy_from_slice(&STALE_FLAG.to_le_bytes());
            masked[e.region_off..e.region_off + 12].copy_from_slice(&RETAIL_STALE_ROW);
            assert_eq!(masked, man);
        }
    }

    #[test]
    fn fix_is_idempotent() {
        for man in [entrance(), interior()] {
            let fixed = plan(&man).unwrap().unwrap();
            assert_eq!(locate(&fixed).unwrap().state, TableState::Fixed);
            assert_eq!(plan(&fixed).unwrap(), None);
        }
    }

    #[test]
    fn stale_flag_no_longer_selects_the_silent_group() {
        for man in [entrance(), interior()] {
            let fixed = plan(&man).unwrap().unwrap();
            let tile = (10, 10); // inside ROOM
            // Retail: the stale flag wins with the castle's row.
            let retail_row: [u8; 8] = RETAIL_STALE_ROW[4..].try_into().unwrap();
            assert_eq!(winning_row(&man, &[STALE_FLAG], tile), Some(retail_row));
            // Fixed: the stale flag is not read; the default room rolls.
            let room: [u8; 8] = ROOM[4..].try_into().unwrap();
            assert_eq!(winning_row(&fixed, &[STALE_FLAG], tile), Some(room));
        }
    }

    #[test]
    fn every_other_flag_state_reads_the_same_rows() {
        for man in [entrance(), interior()] {
            let fixed = plan(&man).unwrap().unwrap();
            for x in (0..=128u8).step_by(4) {
                for z in (0..=128u8).step_by(4) {
                    // Healthy file, before and after Jette: unchanged.
                    for flags in [&[][..], &[POST_JETTE_FLAG][..]] {
                        assert_eq!(
                            winning_row(&man, flags, (x, z)),
                            winning_row(&fixed, flags, (x, z)),
                            "flags {flags:?} tile ({x}, {z})"
                        );
                    }
                    // Affected file reads what a healthy one does.
                    assert_eq!(
                        winning_row(&fixed, &[STALE_FLAG], (x, z)),
                        winning_row(&man, &[], (x, z))
                    );
                    assert_eq!(
                        winning_row(&fixed, &[STALE_FLAG, POST_JETTE_FLAG], (x, z)),
                        winning_row(&man, &[POST_JETTE_FLAG], (x, z))
                    );
                }
            }
        }
    }

    #[test]
    fn refuses_tables_that_are_not_retail() {
        // Group 0 gated on some other flag.
        let other = build_man(
            &[(0x0142, 1), (POST_JETTE_FLAG, 1), (0xFFFF, 1)],
            &[RETAIL_STALE_ROW, QUIET, ROOM],
        );
        assert!(plan(&other).is_err());
        // Group 0's row edited (a rate where retail has none).
        let mut row = RETAIL_STALE_ROW;
        row[4] = 18;
        let edited = build_man(
            &[(STALE_FLAG, 1), (POST_JETTE_FLAG, 1), (0xFFFF, 1)],
            &[row, QUIET, ROOM],
        );
        assert!(plan(&edited).is_err());
        // Group 1 is not the post-Jette group.
        let no_jette = build_man(
            &[(STALE_FLAG, 1), (0x046D, 1), (0xFFFF, 1)],
            &[RETAIL_STALE_ROW, QUIET, ROOM],
        );
        assert!(plan(&no_jette).is_err());
        // Group 1 is not one whole-map row in effect: its pocket row carries a
        // different payload, so a single copied row could not stand in for it.
        let mut pocket = ROOM;
        pocket[..4].copy_from_slice(&[0, 89, 30, 126]);
        let split = build_man(
            &[(STALE_FLAG, 1), (POST_JETTE_FLAG, 2), (0xFFFF, 1)],
            &[RETAIL_STALE_ROW, pocket, QUIET, ROOM],
        );
        assert!(plan(&split).is_err());
        // Group 1 has no whole-map row at all.
        let mut small = QUIET;
        small[..4].copy_from_slice(&[0, 0, 64, 64]);
        let partial = build_man(
            &[(STALE_FLAG, 1), (POST_JETTE_FLAG, 1), (0xFFFF, 1)],
            &[RETAIL_STALE_ROW, small, ROOM],
        );
        assert!(plan(&partial).is_err());
        // A scene with only a default group (every ordinary dungeon).
        let plain = build_man(&[(0xFFFF, 1)], &[ROOM]);
        assert!(plan(&plain).is_err());
        // Not a MAN.
        assert!(plan(&[7, 0, 0, 0]).is_err());
    }
}
