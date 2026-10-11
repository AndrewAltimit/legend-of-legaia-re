//! Minigame overlay tables: the Muscle Dome arena's course ladder and leg
//! tables, the fishing species names and the slot machine's dead developer
//! labels.

use super::*;

/// PROT entry of the Muscle Dome arena's door / init overlay.
pub(super) const ARENA_PROT_INDEX: u32 = 977;
/// The arena's course descriptor table: three `{ i32 round_count; u32
/// first_round }` records (`legaia_engine_minigames::muscle_dome::COURSE_TABLE_VA`).
const ARENA_COURSE_TABLE_VA: u32 = 0x801D_1A08;
/// The per-`(course, round)` score table, sixteen `i32` cells per course
/// (`legaia_engine_minigames::muscle_dome::SCORE_TABLE_VA`).
const ARENA_SCORE_TABLE_VA: u32 = 0x801D_1860;
/// The two `lui` sites that form [`ARENA_SCORE_TABLE_VA`]: the settlement's
/// `DAT_801D1860 + course * 0x40 + (round - 1) * 4` read (`0x801D10E8`,
/// `sll v1,v1,0x6` on the course) and its sibling at `0x801D1234`.
const ARENA_SCORE_TABLE_SITES: [u32; 2] = [0x801D_10E8, 0x801D_1234];
const ARENA_COURSES: usize = 3;
const ARENA_MAX_ROUNDS: usize = 16;

/// The arena's course ladder, read the way `FUN_801D1510` and the settlement
/// read it: the score table (three rows of sixteen cells, the row stride the
/// consumer's `sll 6` states), every course's run of `{ u32 label_va; u32
/// monster_id }` round records reached through the descriptor table's
/// `first_round` pointers, and the label each round names - the opponent-name
/// pool at the head of the image, which is reached only through those
/// records. The parser of record is
/// `legaia_engine_minigames::muscle_dome::parse_course_ladder`; this mirrors its
/// validation (three courses of `1..=16` rounds, non-zero byte monster ids)
/// and claims nothing when any of it fails.
pub(in crate::byte_account) fn claim_arena_course_ladder(buf: &[u8], sink: &mut Sink) {
    const SLOT_A: u32 = 0x801C_E818;
    let at = |va: u32| va.checked_sub(SLOT_A).map(|o| o as usize);
    let rd = |o: usize| legaia_bytes::u32_le(buf, o);
    let score_ok = ARENA_SCORE_TABLE_SITES
        .iter()
        .all(|&site| at(site).and_then(|o| lui_pair_address(buf, o)) == Some(ARENA_SCORE_TABLE_VA));
    let Some(table) = at(ARENA_COURSE_TABLE_VA) else {
        return;
    };
    let mut runs = Vec::new();
    for c in 0..ARENA_COURSES {
        let (Some(count), Some(first)) = (rd(table + c * 8), rd(table + c * 8 + 4)) else {
            return;
        };
        let count = count as usize;
        let Some(base) = at(first) else {
            sink.note("arena course ladder not claimed: a first_round pointer is below the image");
            return;
        };
        if count == 0 || count > ARENA_MAX_ROUNDS || base + count * 8 > buf.len() {
            sink.note("arena course ladder not claimed: a descriptor is out of range");
            return;
        }
        for r in 0..count {
            let id = rd(base + r * 8 + 4).unwrap_or(0);
            if id == 0 || id > 0xFF {
                sink.note("arena course ladder not claimed: a round's monster id is not a byte");
                return;
            }
        }
        runs.push((base, count));
    }
    if score_ok {
        let start = at(ARENA_SCORE_TABLE_VA).unwrap_or(0);
        sink.claim(
            start,
            start + ARENA_COURSES * ARENA_MAX_ROUNDS * 4,
            OWNER_RECORD,
            "arena score table, 3 courses x 16 i32 cells (FUN_801D1510 settlement)",
        );
    }
    let mut labels = 0usize;
    for (c, &(base, count)) in runs.iter().enumerate() {
        sink.claim(
            base,
            base + count * 8,
            OWNER_RECORD,
            format!("arena course {c}: {count} round record(s) (label_va, monster_id)"),
        );
        for r in 0..count {
            let Some(label) = rd(base + r * 8).and_then(at) else {
                continue;
            };
            let Some(len) = buf
                .get(label..)
                .and_then(|t| t.iter().position(|&b| b == 0))
            else {
                continue;
            };
            sink.claim(label, label + len + 1, OWNER_STRING, "arena round label");
            labels += 1;
        }
    }
    // The descriptor table itself: the three `{ count, first_round }` records
    // the loop above read, which nothing had claimed beyond the pointees.
    sink.claim(
        table,
        table + ARENA_COURSES * 8,
        OWNER_RECORD,
        "arena course descriptors, 3 x { round_count, first_round }",
    );
    sink.note(format!(
        "arena course ladder: {} round record(s), {labels} label(s)",
        runs.iter().map(|&(_, n)| n).sum::<usize>()
    ));
    claim_arena_leg_tables(buf, sink);
}

/// The between-leg HP lane's outcome table (`0x801D1A5C`): `FUN_801D1184`
/// clamps the leg outcome to `3` (`slti v0,a2,0x4` / `li a2,0x3` at
/// `0x801D120C..0x801D1218`) and reads `table[outcome * 4]`, so four words.
/// Around it sit nine words from `0x801D1A38` and one at `0x801D1A6C` that no
/// instruction reaches in any form either address scan covers - a descending
/// percentage run (`100` down to `5`) the shipped turns lane does not use
/// (it scales `min(turns, 8)` directly). Those are claimed shape-checked as
/// dead data.
const ARENA_LEG_OUTCOME_VA: u32 = 0x801D_1A5C;
const ARENA_LEG_OUTCOME_ROWS: usize = 4;
const ARENA_DEAD_PCT_VA: u32 = 0x801D_1A38;
const ARENA_DEAD_PCT_WORDS: usize = 9;
const ARENA_DEAD_TAIL_VA: u32 = 0x801D_1A6C;

fn claim_arena_leg_tables(buf: &[u8], sink: &mut Sink) {
    const SLOT_A: u32 = 0x801C_E818;
    let at = |va: u32| (va - SLOT_A) as usize;
    let small = |o: usize, n: usize| {
        (0..n).all(|i| legaia_bytes::u32_le(buf, o + i * 4).is_some_and(|w| (1..=100).contains(&w)))
    };
    if lui_pair_address(buf, at(0x801D_1214)) != Some(ARENA_LEG_OUTCOME_VA)
        || !small(at(ARENA_LEG_OUTCOME_VA), ARENA_LEG_OUTCOME_ROWS)
    {
        sink.note("arena leg-outcome table not at its pinned consumer");
        return;
    }
    let o = at(ARENA_LEG_OUTCOME_VA);
    sink.claim(
        o,
        o + ARENA_LEG_OUTCOME_ROWS * 4,
        OWNER_RECORD,
        "arena leg-outcome HP table, 4 i32 (FUN_801D1184, outcome clamped to 3)",
    );
    for (va, words) in [
        (ARENA_DEAD_PCT_VA, ARENA_DEAD_PCT_WORDS),
        (ARENA_DEAD_TAIL_VA, 1),
    ] {
        let o = at(va);
        if small(o, words) {
            sink.claim(
                o,
                o + words * 4,
                OWNER_RECORD,
                "arena percentage words no instruction reaches, dead data",
            );
        }
    }
}

/// The species names the fishing species table's `+0x00` pointers name - the
/// banner string `FUN_801D4004` prints for a hooked fish. The table itself is
/// a [`pinned_overlay_tables`] row; its pool is the head of the image, which
/// nothing else reaches.
pub(in crate::byte_account) fn claim_fishing_species_names(buf: &[u8], sink: &mut Sink) {
    use legaia_minigame_formats::fishing_species as fish;
    let Some(species) = fish::parse(buf) else {
        return;
    };
    for sp in &species {
        let Some(name) = sp.name(buf) else {
            continue;
        };
        let off = (sp.name_ptr_va - fish::FISHING_OVERLAY_BASE_VA) as usize;
        sink.claim(
            off,
            off + name.len() + 1,
            OWNER_STRING,
            "fishing species name (fishing_species)",
        );
    }
}

/// The slot overlay's seven dead developer labels
/// ([`legaia_minigame_formats::minigame_slot_scene::DEV_LABELS_OFFSET`]): no instruction in any
/// image forms an address in them, so the claim is shape-checked - each cell a
/// non-empty run of printable ASCII closed by NUL fill - rather than sized off
/// a consumer.
pub(super) fn claim_slot_dev_labels(buf: &[u8], sink: &mut Sink) {
    use legaia_minigame_formats::minigame_slot_scene as slot;
    let start = slot::DEV_LABELS_OFFSET;
    let len = slot::DEV_LABEL_COUNT * slot::DEV_LABEL_STRIDE;
    let Some(run) = buf.get(start..start + len) else {
        return;
    };
    let shaped = run.chunks(slot::DEV_LABEL_STRIDE).all(|cell| {
        let n = cell.iter().position(|&b| b == 0).unwrap_or(cell.len());
        n > 0
            && n < cell.len()
            && cell[..n].iter().all(u8::is_ascii_graphic)
            && cell[n..].iter().all(|&b| b == 0)
    });
    if !shaped {
        sink.note("no developer-label run at the pinned slot-overlay offset");
        return;
    }
    sink.claim(
        start,
        start + len,
        OWNER_STRING,
        "slot developer labels, unreferenced dead data (minigame_slot_scene)",
    );
}
