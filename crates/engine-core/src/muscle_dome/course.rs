//! The course ladder: who you fight, the score table, and the Master-course prize.
//! Split out of `muscle_dome.rs`.

/// Fixed item id of the one-shot Master-course first-clear prize (the
/// War God Icon; `FUN_800421D4(0xCD, 1)`).
pub const CONTEST_PRIZE_ITEM_ID: u8 = 0xCD;

/// Story-flag id of the one-shot prize latch (`FUN_8003CE64(0x6CB)` - once
/// set, the prize never re-awards).
pub const CONTEST_PRIZE_FLAG: u16 = 0x6CB;

/// The Master-course fight index the prize gates on (`round >= 0xD`, i.e.
/// the 13th and final fight of the Master course row).
pub const CONTEST_PRIZE_ROUND: u32 = 0xD;

// --- The course ladder: who you actually fight ------------------------------

/// PROT entry holding the arena door/init overlay the ladder lives in.
pub const ARENA_OVERLAY_PROT_INDEX: usize = 977;

/// Load base of that entry as a slot-A overlay.
pub const ARENA_OVERLAY_BASE_VA: u32 = 0x801C_E818;

/// Overlay VA of the 3-entry course descriptor table.
pub const COURSE_TABLE_VA: u32 = 0x801D_1A08;

/// File offset of the course descriptor table in the raw entry.
pub const COURSE_TABLE_FILE_OFFSET: usize = (COURSE_TABLE_VA - ARENA_OVERLAY_BASE_VA) as usize;

/// Overlay VA of the per-`(course, round)` score table.
pub const SCORE_TABLE_VA: u32 = 0x801D_1860;

/// File offset of the score table in the raw entry.
pub const SCORE_TABLE_FILE_OFFSET: usize = (SCORE_TABLE_VA - ARENA_OVERLAY_BASE_VA) as usize;

/// Row stride of the score table: 16 `i32` cells per course.
pub const SCORE_TABLE_COURSE_STRIDE: usize = 0x40;

/// Courses the arena offers.
pub const COURSE_COUNT: usize = 3;

/// Byte stride of one course descriptor (`{ i32 count; u32 first_round }`).
pub const COURSE_DESCRIPTOR_STRIDE: usize = 8;

/// Byte stride of one round record (`{ u32 name_ptr; u32 monster_id }`).
pub const ROUND_RECORD_STRIDE: usize = 8;

/// Rounds any one course may declare, as a sanity bound on the descriptor.
pub const MAX_ROUNDS_PER_COURSE: usize = 16;

/// One round of a course: the opponent, and where its label lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DomeRound {
    /// `+0x00` - overlay VA of the round's label string (the course menu
    /// draws it; the port does not need the text to fight the round).
    pub label_va: u32,
    /// `+0x04` - the opponent's **monster id**, the byte `FUN_801D1510`
    /// stores into formation slot 0 at `0x8007BD0C`. Index it into the
    /// monster archive as `(id - 1) * 0x14000`.
    pub monster_id: u8,
}

/// One course of the ladder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomeCourse {
    /// Its rounds, in fight order.
    pub rounds: Vec<DomeRound>,
}

/// Decode the arena's course ladder out of a raw PROT 0977 entry.
///
/// The descriptor table at [`COURSE_TABLE_FILE_OFFSET`] holds three
/// `{ i32 round_count; u32 first_round }` records; each `first_round`
/// points at a run of `round_count` `{ u32 label_va; u32 monster_id }`
/// records in the same entry. Retail's `FUN_801D1510` indexes exactly this
/// pair with `(DAT_801D1A90, DAT_801D1A94)` - the same `(course, round)` the
/// score table takes - and writes the round's `monster_id` byte into
/// formation slot 0.
///
/// Returns `None` when the descriptor does not decode as three in-range
/// courses of `1..=`[`MAX_ROUNDS_PER_COURSE`] rounds each, which is what
/// keeps the fixed offsets honest on an entry that is not this one.
///
/// PORT: FUN_801d1510 (the table walk; the formation store is the host's)
pub fn parse_course_ladder(overlay_0977: &[u8]) -> Option<Vec<DomeCourse>> {
    let read_u32 = |at: usize| -> Option<u32> {
        overlay_0977
            .get(at..at + 4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
    };
    let mut out = Vec::with_capacity(COURSE_COUNT);
    for course in 0..COURSE_COUNT {
        let at = COURSE_TABLE_FILE_OFFSET + course * COURSE_DESCRIPTOR_STRIDE;
        let count = read_u32(at)? as usize;
        let first = read_u32(at + 4)?;
        if count == 0 || count > MAX_ROUNDS_PER_COURSE {
            return None;
        }
        let base = first.checked_sub(ARENA_OVERLAY_BASE_VA)? as usize;
        let mut rounds = Vec::with_capacity(count);
        for r in 0..count {
            let rec = base + r * ROUND_RECORD_STRIDE;
            let label_va = read_u32(rec)?;
            let monster_id = read_u32(rec + 4)?;
            // Retail takes the byte, not the word (`lbu ... 4(v0)`).
            if monster_id > 0xFF || monster_id == 0 {
                return None;
            }
            rounds.push(DomeRound {
                label_va,
                monster_id: monster_id as u8,
            });
        }
        out.push(DomeCourse { rounds });
    }
    Some(out)
}

/// The score cell a cleared `(course, round)` adds to the running tally.
///
/// `round` is 1-based, matching retail's `DAT_801D1860 + course * 0x40 +
/// (round - 1) * 4`. Returns `None` outside the table.
pub fn course_score_cell(overlay_0977: &[u8], course: usize, round: u32) -> Option<i32> {
    if course >= COURSE_COUNT || round == 0 || round as usize > MAX_ROUNDS_PER_COURSE {
        return None;
    }
    let at =
        SCORE_TABLE_FILE_OFFSET + course * SCORE_TABLE_COURSE_STRIDE + (round as usize - 1) * 4;
    overlay_0977
        .get(at..at + 4)
        .map(|b| i32::from_le_bytes(b.try_into().unwrap()))
}

/// The score cell table's rounds-per-course capacity, as a decoded row.
pub type ScoreRow = [i32; MAX_ROUNDS_PER_COURSE];

/// Decode the arena's per-`(course, round)` score table out of a raw PROT
/// 0977 entry - the same `DAT_801D1860` rows [`course_score_cell`] indexes,
/// lifted whole so a running contest carries its own copy.
///
/// Returns `None` when the table does not lie inside the entry.
pub fn parse_score_table(overlay_0977: &[u8]) -> Option<[ScoreRow; COURSE_COUNT]> {
    let mut out = [[0i32; MAX_ROUNDS_PER_COURSE]; COURSE_COUNT];
    for (course, row) in out.iter_mut().enumerate() {
        for (r, cell) in row.iter_mut().enumerate() {
            let at = SCORE_TABLE_FILE_OFFSET + course * SCORE_TABLE_COURSE_STRIDE + r * 4;
            let b = overlay_0977.get(at..at + 4)?;
            *cell = i32::from_le_bytes(b.try_into().unwrap());
        }
    }
    Some(out)
}
