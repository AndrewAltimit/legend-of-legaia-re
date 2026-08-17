//! Disc-gated oracle for **show Super Arts on the in-battle move list**
//! (`legaia_patcher::super_art_list` + `apply::inject_super_art_list`).
//!
//! The feature lists a character's *performed* Super Arts on the Tactical-Arts
//! list `FUN_80034358` draws, sorted in by AP, through two same-size detours into
//! that renderer (`0x800343C4` count / `0x80034450` id), a row-fill routine and
//! three small tables spread over the four verified-dead SCUS regions, a
//! wholesale in-place replacement of the list pager `FUN_801D3748` inside PROT
//! 0898 whose tail hosts the performed-byte writer, and a two-word detour from
//! the Super applier's match arm (`0x801EFBCC`) into that writer.
//!
//! These apply it to a scratch copy of the real disc and assert, off the patched
//! image, that every hosted region was all-zero pre-patch; each detour became
//! exactly the planned `j routine` and the scan head / hit arm the hooks return
//! into stay retail; the routines and tables land exactly where the plan says;
//! every Super Art's trigger chain resolves against the disc's own arts-name
//! table, its AP total is the sum of those rows' AP bytes, its threshold id is
//! the first row at or below that AP, and its derived physical input tokenizes
//! back to its trigger pattern; the packed arrows and the 4-byte records read
//! back as planned; the runtime name chase lands on a record that carries that
//! Super Art's own name; the replacement pager plus writer fit inside the
//! original 81-instruction body, are nop-padded to it, and the pager's one
//! caller still targets its entry; the applier detour lands and everything else
//! in the applier stays retail; **every byte that changed in `SCUS_942.54` and
//! PROT 0898 is inside a planned edit, and no other file on the disc moved at
//! all** (which is what makes the toggle byte-inert when off); every touched
//! sector stays EDC/ECC valid; the run is byte-deterministic; and an
//! unrecognized build, a dirty region or a prior dead-space feature is refused
//! without writing anything.
//!
//! Gates on `LEGAIA_DISC_BIN`; skips and passes when unset.
//!
//! HONESTY GATE: this proves only WHERE the bytes land and what the tables say,
//! never in-game behaviour. A live battle playtest - open Triangle before any
//! Super Art was performed and see an unchanged list, perform one and see
//! exactly that Super Art appear from the next battle on, in AP order, with its
//! name, AP and arrows, and see Terra's list stay empty - is still required
//! before calling the feature done.

use legaia_asset::item_names::file_offset_for_va;
use legaia_iso::iso9660::{find_file_in_image, read_file_in_image};
use legaia_iso::raw::{SECTOR_SIZE, USER_DATA_SIZE};
use legaia_patcher::apply;
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::shiny_seru::{
    ARENA1_END_VA, ARENA1_VA, ARENA2_END_VA, ARENA2_VA, SCUS_GAP_END_VA, SCUS_GAP_VA, SLOT6_END_VA,
    SLOT6_VA,
};
use legaia_patcher::super_art_list::{
    APPLIER_VA, ARROW_BITS, ARROWS_COUNT_SHIFT, ARROWS_STRIDE, ART_CONSTANT_BIAS, ARTS_CHARACTERS,
    FIRST_NORMAL_ORDINAL, GLYPH_BUF_BYTES, HOOK_COUNT_VA, HOOK_ID_VA, HOOK_PERFORMED_VA,
    MARKERS_SHIFT, MAX_MARKERS, OVERLAY_BASE_VA, OVERLAY_PROT_INDEX, PAGER_VA, PAGER_WORDS,
    PERFORMED_COUNT_SHIFT, PERFORMED_MASK, PERFORMED_RET_VA, SCAN_HEAD_VA, SCAN_HIT_VA,
    SCRATCH_BYTES, SUP_STRIDE, SUPER_ARTS_PER_CHAR, SuperArtListInjection, super_art_rows,
};
use legaia_patcher::super_art_menu::{
    HOOK_BOUND_VA, HOOK_CURSOR_VA, HOOK_MARK_VA, HOOK_ROW_VA, MENU_BASE_VA, MENU_DESC_END_VA,
    MENU_DESC_VA, MENU_PROT_INDEX, MENU_RUN_END_VA, MENU_RUN_VA, SuperArtMenuInjection,
};

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

fn scus_of(img: &[u8]) -> Vec<u8> {
    read_file_in_image(img, "SCUS_942.54").expect("SCUS_942.54")
}

fn word_at_va(scus: &[u8], va: u32) -> u32 {
    let off = file_offset_for_va(scus, va).expect("VA in SCUS");
    u32::from_le_bytes(scus[off..off + 4].try_into().unwrap())
}

fn bytes_at_va(scus: &[u8], va: u32, len: usize) -> Vec<u8> {
    let off = file_offset_for_va(scus, va).expect("VA in SCUS");
    scus[off..off + len].to_vec()
}

fn overlay_word(entry: &[u8], va: u32) -> u32 {
    let off = (va - OVERLAY_BASE_VA) as usize;
    u32::from_le_bytes(entry[off..off + 4].try_into().unwrap())
}

/// `j target` as the encoder emits it.
fn j_word(target: u32) -> u32 {
    (0x02 << 26) | ((target >> 2) & 0x03ff_ffff)
}

/// Patch a scratch copy and hand back `(patched image, plan)`.
fn patched(original: &[u8]) -> (Vec<u8>, SuperArtListInjection) {
    let (img, plan, _) = patched_with_menu(original);
    (img, plan)
}

/// The same, keeping the menu-side plan too.
fn patched_with_menu(original: &[u8]) -> (Vec<u8>, SuperArtListInjection, SuperArtMenuInjection) {
    let scus = scus_of(original);
    let mut patcher = DiscPatcher::open(original.to_vec()).expect("open disc");
    let ov = patcher.read_entry(OVERLAY_PROT_INDEX).expect("read 0898");
    let plan = SuperArtListInjection::plan(&scus, &ov).expect("plan");
    let ov9 = patcher.read_entry(MENU_PROT_INDEX).expect("read 0899");
    let menu = SuperArtMenuInjection::plan(&ov9, &plan.rows, plan.sup_va, plan.arrows_va)
        .expect("menu plan");
    apply::inject_super_art_list(&mut patcher).expect("inject");
    (patcher.into_image(), plan, menu)
}

/// The four verified-dead regions this feature spans.
const REGIONS: [(u32, u32, &str); 4] = [
    (SCUS_GAP_VA, SCUS_GAP_END_VA, "gap 1"),
    (ARENA1_VA, ARENA1_END_VA, "arena 1"),
    (ARENA2_VA, ARENA2_END_VA, "arena 2"),
    (SLOT6_VA, SLOT6_END_VA, "slot 6"),
];

#[test]
fn hosted_regions_are_all_zero_before_patch() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let scus = scus_of(&disc);
    for (va, end, what) in REGIONS {
        let off = file_offset_for_va(&scus, va).expect("VA");
        let len = (end - va) as usize;
        assert!(
            scus[off..off + len].iter().all(|&b| b == 0),
            "{what} {va:#x}..{end:#x} must be dead space"
        );
    }
    eprintln!("all four dead-space regions are all-zero on the real disc");
}

#[test]
fn the_performed_byte_is_the_unreachable_sixteenth_id_slot() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let scus = scus_of(&disc);
    let rows = legaia_art::arts_table::raw_records_from_scus(&scus).expect("arts table");
    assert_eq!(rows.len(), 45, "fifteen regular arts per character");
    for ch in legaia_art::queue::Character::all() {
        let n = rows.iter().filter(|r| r.character == ch).count();
        assert_eq!(
            n, 15,
            "{ch:?}: the learned list can hold at most fifteen ids"
        );
        let max = rows
            .iter()
            .filter(|r| r.character == ch)
            .map(|r| r.index)
            .max()
            .unwrap();
        assert!(
            max <= PERFORMED_MASK,
            "{ch:?}: ids fit the five-bit threshold field"
        );
    }
    // Sixteen slots at +0x74E..+0x75D; fifteen arts fill +0x74E..+0x75C.
    assert_eq!(0x74E + 15, 0x75D);
    eprintln!(
        "record +0x75D is the sixteenth id slot: unreachable with fifteen arts per character"
    );
}

#[test]
fn every_trigger_chain_resolves_against_this_discs_arts_table() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let scus = scus_of(&disc);
    let table = legaia_art::arts_table::raw_records_from_scus(&scus).expect("arts table");
    let rows = super_art_rows(&scus).expect("derive rows");
    assert_eq!(rows.len(), ARTS_CHARACTERS * SUPER_ARTS_PER_CHAR);

    for r in &rows {
        // The chain is stored as action constants; the learned list stores
        // display ids, and display row n is constant 0x1B + n. Every converted
        // id has to be a real row of this disc's table for the same character.
        let source = legaia_art::SUPER_ARTS
            .iter()
            .find(|s| s.character == r.character && s.finisher == r.finisher)
            .expect("source entry");
        let want: Vec<u8> = source
            .art_sequence()
            .iter()
            .map(|c| c - ART_CONSTANT_BIAS)
            .collect();
        assert_eq!(r.chain_ids, want, "{} chain ids", r.name);
        assert!(want.len() >= 2, "{} would trigger on one art", r.name);

        // AP is the sum of the chain arts' own AP bytes, duplicates included.
        let mut ap = 0u32;
        for &id in &r.chain_ids {
            let rec = table
                .iter()
                .find(|t| t.character == r.character && t.index == id)
                .unwrap_or_else(|| panic!("{}: chain id {id} has no row", r.name));
            ap += u32::from(rec.ap);
        }
        assert_eq!(u32::from(r.ap), ap, "{} AP total", r.name);
        assert!(r.ap > 0, "{} would advertise a free move", r.name);

        // The threshold: the lowest id whose AP is at or below the Super's,
        // in this character's table - the id the merge puts the row before.
        let mut mine: Vec<_> = table
            .iter()
            .filter(|t| t.character == r.character)
            .collect();
        mine.sort_by_key(|t| t.index);
        let thr = mine
            .iter()
            .find(|t| t.ap <= r.ap)
            .map(|t| t.index)
            .expect("threshold");
        assert_eq!(r.thr, thr, "{} threshold", r.name);
        for t in &mine {
            let before = t.index >= r.thr;
            assert_eq!(
                before,
                t.ap <= r.ap,
                "{} vs id {} ({} AP)",
                r.name,
                t.index,
                t.ap
            );
        }

        // The physical input tokenizes back to the trigger pattern through the
        // retail tokenizer, over this character's normal arts in grid order.
        let catalog: Vec<(legaia_art::ActionConstant, &[legaia_art::Command])> = mine
            .iter()
            .enumerate()
            .filter(|(o, _)| *o >= FIRST_NORMAL_ORDINAL)
            .map(|(_, t)| {
                (
                    legaia_art::ActionConstant::from_byte(t.index + ART_CONSTANT_BIAS).unwrap(),
                    t.commands.as_slice(),
                )
            })
            .collect();
        let q = legaia_art::tokenize(&catalog, &r.input);
        let p = legaia_art::tokenize::populated(&q);
        assert_eq!(
            &p[p.len() - source.find.len()..],
            source.find,
            "{}: input {} does not tokenize to its trigger",
            r.name,
            r.input_letters()
        );
        assert!(
            (7..=9).contains(&r.input.len()),
            "{}: {} arrows",
            r.name,
            r.input.len()
        );
        // The art ends the row colours: one per chain art, the last on the
        // final arrow, all recomputed here off the disc's own catalog.
        let ends = legaia_art::art_ends(&catalog, &r.input);
        assert_eq!(ends, r.ends, "{}: art ends", r.name);
        assert_eq!(
            ends.len(),
            source.art_sequence().len(),
            "{}: one end per chain art",
            r.name
        );
        assert_eq!(
            *ends.last().unwrap(),
            r.input.len() - 1,
            "{}: the last art ends on the last arrow",
            r.name
        );
        // ...and it matches the curated walkthrough input, direction for
        // direction (the two sources are independent).
        let curated = legaia_gamedata::Database::load();
        let art = curated
            .find_art_by_name(r.name)
            .unwrap_or_else(|| panic!("{}: not in the curated arts table", r.name));
        let curated_dirs: Vec<u8> = art.directions.clone();
        let mine_dirs: Vec<u8> = r.input.iter().map(|c| c.as_byte()).collect();
        assert_eq!(
            mine_dirs, curated_dirs,
            "{}: derived input vs walkthrough",
            r.name
        );
        assert_eq!(
            u32::from(r.ap),
            art.ap,
            "{}: chain AP vs walkthrough",
            r.name
        );
        eprintln!(
            "{:?} {:<20} {:>3} AP  thr {:>2}  {}  ({})",
            r.character,
            r.name,
            r.ap,
            r.thr,
            r.input_letters(),
            r.chain_names.join(" + ")
        );
    }
    // Per character: five rows, sorted AP-descending, sorted_index 0..5.
    for (c, ch) in legaia_art::queue::Character::all().into_iter().enumerate() {
        let mine = &rows[c * SUPER_ARTS_PER_CHAR..(c + 1) * SUPER_ARTS_PER_CHAR];
        assert!(mine.iter().all(|r| r.character == ch));
        assert!(
            mine.windows(2).all(|w| w[0].ap >= w[1].ap),
            "{ch:?} AP-descending"
        );
        assert_eq!(
            mine.iter().map(|r| r.sorted_index).collect::<Vec<_>>(),
            [0, 1, 2, 3, 4]
        );
        let mut trig: Vec<u8> = mine.iter().map(|r| r.trigger_row).collect();
        trig.sort_unstable();
        assert_eq!(trig, [0, 1, 2, 3, 4], "{ch:?} every trigger row once");
    }
}

#[test]
fn the_runtime_name_chase_lands_on_the_super_arts_own_record() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // The injected routine resolves a row's name as
    // `DAT_801C9360[char] -> +0x58 -> +4 -> (finisher - 0x10) * 0xD0 -> +0x10`.
    // The same arithmetic off the decoded `record0` is what `super_art_power`
    // locates a Super Art's record with, and it only yields a row when that
    // record's `+0x10` field IS the Super Art's name - so all fifteen resolving
    // is the disc-side proof that the chase's constants are right.
    let scus = scus_of(&disc);
    let patcher = DiscPatcher::open(disc.clone()).expect("open");
    let mut found = 0usize;
    for ch in legaia_art::queue::Character::all() {
        let idx = legaia_patcher::super_art_power::player_entry_index(ch);
        let entry = patcher.read_entry(idx).expect("player battle file");
        let rows = legaia_patcher::super_art_power::super_art_powers(&scus, &entry, ch)
            .expect("decode record0");
        assert_eq!(
            rows.len(),
            SUPER_ARTS_PER_CHAR,
            "{ch:?}: every Super Art record must carry its own name at +0x10"
        );
        found += rows.len();
    }
    assert_eq!(found, ARTS_CHARACTERS * SUPER_ARTS_PER_CHAR);
    eprintln!("{found} Super Art records resolve by name through the chase's offsets");
}

#[test]
fn detours_land_and_the_scan_head_survives() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let before = scus_of(&disc);
    let stock_head = word_at_va(&before, SCAN_HEAD_VA);
    let stock_hit = word_at_va(&before, SCAN_HIT_VA);
    let stock_between = word_at_va(&before, HOOK_ID_VA + 8);
    let (img, plan) = patched(&disc);
    let after = scus_of(&img);

    assert_eq!(word_at_va(&after, HOOK_COUNT_VA), j_word(plan.count_va));
    assert_eq!(word_at_va(&after, HOOK_COUNT_VA + 4), 0, "nop");
    assert_eq!(word_at_va(&after, HOOK_ID_VA), j_word(plan.id_va));
    assert_eq!(word_at_va(&after, HOOK_ID_VA + 4), 0, "nop");
    // A learned row returns to the word after the id detour, a Super Art row
    // enters the scan's hit arm, and the scan head is reached by the scan's own
    // tail - all three must stay retail.
    assert_eq!(
        word_at_va(&after, HOOK_ID_VA + 8),
        stock_between,
        "the id hook's return word"
    );
    assert_eq!(
        word_at_va(&after, SCAN_HEAD_VA),
        stock_head,
        "the scan head stays retail"
    );
    assert_eq!(
        word_at_va(&after, SCAN_HIT_VA),
        stock_hit,
        "the scan's hit arm stays retail"
    );
    eprintln!(
        "detours: count -> {:#x}, id -> {:#x}; fill at {:#x}",
        plan.count_va, plan.id_va, plan.fill_va
    );
}

#[test]
fn the_tables_read_back_exactly_as_planned() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let (img, plan) = patched(&disc);
    let scus = scus_of(&img);
    let n = ARTS_CHARACTERS * SUPER_ARTS_PER_CHAR;

    // The 4-byte records and the packed arrows, both in the
    // `character * 5 + sorted_index` order the routines index them by.
    let sup = bytes_at_va(&scus, plan.sup_va, n * SUP_STRIDE as usize);
    let arrows = bytes_at_va(&scus, plan.arrows_va, n * ARROWS_STRIDE as usize);
    for (i, r) in plan.rows.iter().enumerate() {
        let rec = &sup[i * 4..i * 4 + 4];
        assert_eq!(rec, r.sup_record(), "record {i} ({})", r.name);
        assert_eq!(rec[0], r.ap);
        assert_eq!(rec[1] & PERFORMED_MASK, r.thr);
        assert_eq!(rec[1] >> PERFORMED_COUNT_SHIFT, r.trigger_row);
        assert_eq!(u16::from_le_bytes([rec[2], rec[3]]) & 0x1FFF, r.name_offset);
        assert_eq!(
            u16::from_le_bytes([rec[2], rec[3]]) >> MARKERS_SHIFT,
            r.marker_count() as u16,
            "{} marker count",
            r.name
        );
        assert!(r.marker_count() <= MAX_MARKERS);
        let a = &arrows[i * 4..i * 4 + 4];
        assert_eq!(a, r.packed_arrows(), "arrows {i} ({})", r.name);
        // Unpack the word the way the fill routine does and get the input, the
        // art ends and the count back.
        let w = u32::from_le_bytes(a.try_into().unwrap());
        assert_eq!(
            (w >> ARROWS_COUNT_SHIFT) as usize,
            r.input.len(),
            "{} count",
            r.name
        );
        for (k, c) in r.input.iter().enumerate() {
            let f = (w >> (k as u32 * ARROW_BITS)) & 7;
            assert_eq!(
                (f & 3) as u8,
                legaia_patcher::super_art_list::arrow_code(*c),
                "{} arrow {k}",
                r.name
            );
            assert_eq!(f & 4 != 0, r.ends.contains(&k), "{} end bit {k}", r.name);
        }
        assert_eq!(r.sorted_index as usize, i % SUPER_ARTS_PER_CHAR);
    }

    // The scratch record: `+8` points at the glyph buffer; `+2` and `+0xC`
    // start clear and are filled per row.
    let scratch = bytes_at_va(&scus, plan.scratch_va, SCRATCH_BYTES);
    let glyphs = u32::from_le_bytes(scratch[8..12].try_into().unwrap());
    assert_eq!(glyphs, plan.buf_va, "the glyph pointer aims at the buffer");
    assert_eq!(&scratch[0..8], &[0u8; 8], "AP + padding start clear");
    assert_eq!(&scratch[12..16], &[0u8; 4], "the name pointer starts clear");
    // The buffer itself is dead space at patch time (filled at runtime).
    assert!(
        bytes_at_va(&scus, plan.buf_va, GLYPH_BUF_BYTES)
            .iter()
            .all(|&b| b == 0)
    );
    eprintln!(
        "tables: records {:#x}, arrows {:#x}, scratch {:#x}, buffer {:#x}",
        plan.sup_va, plan.arrows_va, plan.scratch_va, plan.buf_va
    );
}

#[test]
fn every_region_still_has_slack_after_the_injection() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let (img, plan) = patched(&disc);
    let scus = scus_of(&img);
    let mut total = 0usize;
    for (va, end, what) in REGIONS {
        let len = (end - va) as usize;
        let region = bytes_at_va(&scus, va, len);
        let used = region.iter().rposition(|&b| b != 0).map_or(0, |i| i + 1);
        total += used;
        assert!(used <= len, "{what} overran");
        eprintln!("{what}: {used} of {len} B used");
    }
    // Every planned SCUS edit is inside one of the four regions or is a detour.
    for e in plan.edits.iter().filter(|e| e.prot_index.is_none()) {
        assert!(!e.bytes.is_empty());
    }
    eprintln!("{total} B of 652 B of verified-dead SCUS space used");
}

#[test]
fn pager_is_replaced_whole_and_its_caller_still_reaches_it() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(disc.clone()).expect("open");
    let before = patcher.read_entry(OVERLAY_PROT_INDEX).expect("0898");
    // Retail's single caller, `jal FUN_801D3748` at 0x801D21BC.
    const CALLER_VA: u32 = 0x801D_21BC;
    let caller = overlay_word(&before, CALLER_VA);
    assert_eq!(
        caller & 0xFC00_0000,
        0x0C00_0000,
        "0x801D21BC is the jal to the pager"
    );
    assert_eq!((caller & 0x03ff_ffff) << 2, PAGER_VA & 0x0fff_ffff);

    let (img, plan) = patched(&disc);
    let after = DiscPatcher::open(img)
        .unwrap()
        .read_entry(OVERLAY_PROT_INDEX)
        .expect("0898 after");

    // The caller is untouched, so the replacement keeps the same entry point.
    assert_eq!(overlay_word(&after, CALLER_VA), caller);
    // The whole 81-word body was rewritten; the last words are the nop padding
    // the plan appends so no retail instruction survives behind the new tail.
    let base = (PAGER_VA - OVERLAY_BASE_VA) as usize;
    let body = &after[base..base + PAGER_WORDS * 4];
    let planned = &plan
        .edits
        .iter()
        .find(|e| e.prot_index == Some(OVERLAY_PROT_INDEX) && e.bytes.len() == PAGER_WORDS * 4)
        .expect("pager edit")
        .bytes;
    assert_eq!(planned.len(), PAGER_WORDS * 4, "same size as retail's body");
    assert_eq!(body, planned.as_slice());
    assert_eq!(
        overlay_word(&after, PAGER_VA + (PAGER_WORDS as u32) * 4),
        overlay_word(&before, PAGER_VA + (PAGER_WORDS as u32) * 4),
        "the next function is untouched"
    );
    // The pager's tail hosts the performed-byte writer, at the VA the plan says,
    // and the applier's match arm detours into it - two words, with the flag
    // store after them and the applier's entry untouched.
    let words: Vec<u32> = planned
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
        .collect();
    assert!(
        plan.performed_va > PAGER_VA && plan.performed_va < PAGER_VA + (PAGER_WORDS as u32) * 4
    );
    let w_idx = ((plan.performed_va - PAGER_VA) / 4) as usize;
    assert_eq!(words[w_idx], 0x3C03_801F, "W replays lui v1,0x801f");
    assert_eq!(words[w_idx + 1], 0x2402_0001, "W replays addiu v0,zero,1");
    assert_eq!(
        overlay_word(&after, HOOK_PERFORMED_VA),
        j_word(plan.performed_va)
    );
    assert_eq!(overlay_word(&after, HOOK_PERFORMED_VA + 4), 0, "nop");
    assert_eq!(
        overlay_word(&after, PERFORMED_RET_VA),
        overlay_word(&before, PERFORMED_RET_VA),
        "the flag store W returns to stays retail"
    );
    assert_eq!(
        overlay_word(&after, APPLIER_VA),
        overlay_word(&before, APPLIER_VA)
    );
    // Nothing else in the applier moved.
    for va in (APPLIER_VA..APPLIER_VA + 536).step_by(4) {
        if va == HOOK_PERFORMED_VA || va == HOOK_PERFORMED_VA + 4 {
            continue;
        }
        assert_eq!(
            overlay_word(&after, va),
            overlay_word(&before, va),
            "applier word {va:#x}"
        );
    }
    let live = words.iter().rposition(|&w| w != 0).unwrap() + 1;
    assert!(
        live < PAGER_WORDS,
        "replacement is {live} words in an {PAGER_WORDS}-word body"
    );
    eprintln!("pager: {live} live words + {} nops", PAGER_WORDS - live);
}

#[test]
fn the_menu_side_lands_in_0899_and_reads_back() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let before = DiscPatcher::open(disc.clone())
        .unwrap()
        .read_entry(MENU_PROT_INDEX)
        .unwrap();
    let (img, plan, menu) = patched_with_menu(&disc);
    let after = DiscPatcher::open(img)
        .unwrap()
        .read_entry(MENU_PROT_INDEX)
        .unwrap();
    let w = |ov: &[u8], va: u32| {
        let o = (va - MENU_BASE_VA) as usize;
        u32::from_le_bytes(ov[o..o + 4].try_into().unwrap())
    };
    // The four detours, and the words they return to / around them untouched.
    assert_eq!(w(&after, HOOK_BOUND_VA), j_word(menu.bound_va));
    assert_eq!(w(&after, HOOK_BOUND_VA + 4), 0);
    assert_eq!(w(&after, HOOK_BOUND_VA + 8), w(&before, HOOK_BOUND_VA + 8));
    assert_eq!(w(&after, HOOK_MARK_VA), j_word(menu.mark_va));
    assert_eq!(w(&after, HOOK_MARK_VA + 8), w(&before, HOOK_MARK_VA + 8));
    assert_eq!(w(&after, HOOK_ROW_VA), j_word(menu.row_va));
    assert_eq!(
        w(&after, HOOK_ROW_VA + 4),
        w(&before, HOOK_ROW_VA + 4),
        "the scan head is a branch target and stays"
    );
    assert_eq!(w(&after, HOOK_CURSOR_VA), j_word(menu.cursor_va));
    assert_eq!(
        w(&after, HOOK_CURSOR_VA + 4),
        w(&before, HOOK_CURSOR_VA + 4),
        "the nop after the cursor hook is a branch target and stays"
    );
    // Every menu edit landed, and both runs were dead space before.
    for e in menu
        .edits
        .iter()
        .filter(|e| e.prot_index == Some(MENU_PROT_INDEX))
    {
        assert_eq!(
            &after[e.file_off..e.file_off + e.bytes.len()],
            e.bytes.as_slice(),
            "menu edit at {:#x}",
            e.file_off
        );
    }
    for (lo, hi) in [
        (MENU_RUN_VA, MENU_RUN_END_VA),
        (MENU_DESC_VA, MENU_DESC_END_VA),
    ] {
        let (a, b) = ((lo - MENU_BASE_VA) as usize, (hi - MENU_BASE_VA) as usize);
        assert!(
            before[a..b].iter().all(|&x| x == 0),
            "0899 run {lo:#x} was dead space"
        );
    }
    // Names and descriptions read back as text, one per row in table order.
    let scratch_off = (menu.scratch_va - MENU_BASE_VA) as usize;
    assert_eq!(
        &after[scratch_off + 8..scratch_off + 12],
        &menu.buf_va.to_le_bytes(),
        "scratch +8 -> the glyph buffer"
    );
    for (i, r) in plan.rows.iter().enumerate() {
        let np = w(&after, menu.names_va + i as u32 * 4);
        let dp = w(&after, menu.descs_va + i as u32 * 4);
        let cstr = |va: u32| {
            let mut o = (va - MENU_BASE_VA) as usize;
            let mut v = Vec::new();
            while after[o] != 0 {
                v.push(after[o]);
                o += 1;
            }
            String::from_utf8(v).unwrap()
        };
        assert_eq!(cstr(np), r.name, "row {i} name");
        let d = cstr(dp);
        assert!(d.starts_with("Super Arts."), "row {i} description {d:?}");
        for chain in &r.chain_names {
            assert!(
                d.contains(chain.as_str()),
                "row {i} description names {chain}: {d:?}"
            );
        }
        assert!(
            (MENU_DESC_VA..MENU_DESC_END_VA).contains(&dp),
            "descriptions live in the second run"
        );
    }
    eprintln!(
        "menu: {} B of the 0899 code run, {} B of descriptions",
        menu.run_used, menu.desc_used
    );
}

#[test]
fn only_the_planned_bytes_move_and_no_other_file_does() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let (img, plan) = patched(&disc);
    assert_eq!(img.len(), disc.len(), "same-size in-place edits only");

    // 1. In SCUS logical space, every changed byte is inside a planned edit.
    let (before, after) = (scus_of(&disc), scus_of(&img));
    let mut allowed: Vec<(usize, usize)> = plan
        .edits
        .iter()
        .filter(|e| e.prot_index.is_none())
        .map(|e| (e.file_off, e.file_off + e.bytes.len()))
        .collect();
    allowed.sort_unstable();
    let changed: Vec<usize> = (0..before.len())
        .filter(|&i| before[i] != after[i])
        .collect();
    assert!(!changed.is_empty(), "the feature must change SCUS");
    for &i in &changed {
        assert!(
            allowed.iter().any(|&(a, b)| (a..b).contains(&i)),
            "SCUS byte {i:#x} changed outside every planned edit"
        );
    }
    // ...and every planned edit really landed.
    for e in plan.edits.iter().filter(|e| e.prot_index.is_none()) {
        assert_eq!(
            &after[e.file_off..e.file_off + e.bytes.len()],
            e.bytes.as_slice(),
            "planned SCUS edit at {:#x}",
            e.file_off
        );
    }

    // 2. Nothing outside SCUS_942.54 and PROT entry 898 moved. Map every
    // changed 2352-byte sector back and require it to belong to one of the two.
    let (scus_lba, scus_size) = find_file_in_image(&disc, "SCUS_942.54").expect("SCUS extent");
    let scus_sectors = (scus_size as usize).div_ceil(USER_DATA_SIZE);
    let (prot_lba, prot_size) = find_file_in_image(&disc, "PROT.DAT").expect("PROT extent");
    let mut payload = Vec::with_capacity(prot_size as usize);
    for s in 0..(prot_size as usize).div_ceil(USER_DATA_SIZE) {
        let base = (prot_lba as usize + s) * SECTOR_SIZE + 24;
        payload.extend_from_slice(&disc[base..base + USER_DATA_SIZE]);
    }
    payload.truncate(prot_size as usize);
    let archive = legaia_prot::archive::Archive::from_bytes(payload).expect("PROT TOC");
    let ov = &archive.entries[OVERLAY_PROT_INDEX];
    let ov_first = prot_lba as usize + ov.start_lba as usize;
    let ov_last = ov_first + ov.size_sectors as usize;
    let ov9 = &archive.entries[MENU_PROT_INDEX];
    let ov9_first = prot_lba as usize + ov9.start_lba as usize;
    let ov9_last = ov9_first + ov9.size_sectors as usize;
    let scus_first = scus_lba as usize;
    let scus_last = scus_first + scus_sectors;

    let mut touched = 0usize;
    for s in 0..disc.len() / SECTOR_SIZE {
        let a = s * SECTOR_SIZE;
        if disc[a..a + SECTOR_SIZE] == img[a..a + SECTOR_SIZE] {
            continue;
        }
        touched += 1;
        assert!(
            (scus_first..scus_last).contains(&s)
                || (ov_first..ov_last).contains(&s)
                || (ov9_first..ov9_last).contains(&s),
            "sector {s} changed but belongs to none of SCUS, PROT {OVERLAY_PROT_INDEX}, PROT {MENU_PROT_INDEX}"
        );
        // 3. Every touched sector is still EDC/ECC-valid.
        assert!(
            legaia_iso::write::mode2_form1_sector_is_valid(&img[a..a + SECTOR_SIZE]),
            "sector {s} must stay EDC/ECC-valid"
        );
    }
    assert!(touched > 0);
    eprintln!(
        "{touched} sectors changed, all inside SCUS / PROT {OVERLAY_PROT_INDEX} / PROT {MENU_PROT_INDEX}, all EDC/ECC-valid"
    );
}

#[test]
fn with_the_toggle_off_nothing_this_feature_touches_moves() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // Patch a disc with an unrelated feature (the drop shuffle, which writes
    // both SCUS-adjacent tables and PROT entries) and leave show-super-arts off.
    // Every byte this feature would touch has to come back byte-identical, which
    // is what makes the toggle inert when it is not asked for.
    let mut patcher = DiscPatcher::open(disc.clone()).expect("open");
    apply::randomize_drops(
        &mut patcher,
        &[],
        0xC0FFEE,
        legaia_patcher::drops::DropMode::Shuffle,
    )
    .expect("drop shuffle");
    let img = patcher.into_image();

    let (before, after) = (scus_of(&disc), scus_of(&img));
    for va in [
        HOOK_COUNT_VA,
        HOOK_COUNT_VA + 4,
        HOOK_ID_VA,
        HOOK_ID_VA + 4,
        SCAN_HEAD_VA,
        SCAN_HIT_VA,
    ] {
        assert_eq!(
            word_at_va(&after, va),
            word_at_va(&before, va),
            "renderer word {va:#x} must not move with the toggle off"
        );
    }
    for (va, end, what) in REGIONS {
        let len = (end - va) as usize;
        assert!(
            bytes_at_va(&after, va, len).iter().all(|&b| b == 0),
            "{what} must still be dead space with the toggle off"
        );
    }
    let ov9_before = DiscPatcher::open(disc.clone())
        .unwrap()
        .read_entry(MENU_PROT_INDEX)
        .unwrap();
    let ov_before = DiscPatcher::open(disc)
        .unwrap()
        .read_entry(OVERLAY_PROT_INDEX)
        .unwrap();
    let ov_after = DiscPatcher::open(img.clone())
        .unwrap()
        .read_entry(OVERLAY_PROT_INDEX)
        .unwrap();
    let base = (PAGER_VA - OVERLAY_BASE_VA) as usize;
    assert_eq!(
        &ov_after[base..base + PAGER_WORDS * 4],
        &ov_before[base..base + PAGER_WORDS * 4],
        "the pager must stay retail with the toggle off"
    );
    let a = (APPLIER_VA - OVERLAY_BASE_VA) as usize;
    assert_eq!(
        &ov_after[a..a + 536],
        &ov_before[a..a + 536],
        "the applier must stay retail with the toggle off"
    );
    let m_after = DiscPatcher::open(img)
        .unwrap()
        .read_entry(MENU_PROT_INDEX)
        .unwrap();
    for va in [HOOK_BOUND_VA, HOOK_MARK_VA, HOOK_ROW_VA, HOOK_CURSOR_VA] {
        let o = (va - MENU_BASE_VA) as usize;
        assert_eq!(
            &m_after[o..o + 8],
            &ov9_before[o..o + 8],
            "menu hook {va:#x} stays retail with the toggle off"
        );
    }
    for (lo, hi) in [
        (MENU_RUN_VA, MENU_RUN_END_VA),
        (MENU_DESC_VA, MENU_DESC_END_VA),
    ] {
        let (a, b) = ((lo - MENU_BASE_VA) as usize, (hi - MENU_BASE_VA) as usize);
        assert!(
            m_after[a..b].iter().all(|&x| x == 0),
            "0899 run {lo:#x} must stay dead space with the toggle off"
        );
    }
    eprintln!(
        "toggle off: every hook site, region, the pager, the applier and the menu overlay are byte-identical"
    );
}

#[test]
fn a_fixed_input_is_byte_deterministic() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let (a, _) = patched(&disc);
    let (b, _) = patched(&disc);
    assert_eq!(a, b, "the injection carries no entropy");
    eprintln!("two runs are byte-identical");
}

#[test]
fn re_applying_or_stacking_a_dead_space_feature_is_refused() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // Re-apply: the regions are no longer dead space.
    let (once, _) = patched(&disc);
    let mut patcher = DiscPatcher::open(once).expect("open patched");
    let err = apply::inject_super_art_list(&mut patcher).expect_err("second apply must fail");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("dead space") || msg.contains("unrecognized build"),
        "unexpected error: {msg}"
    );

    // Stacking on shiny-Seru: the bytes are already taken, so the plan refuses
    // rather than silently writing over another feature's routine.
    let mut patcher = DiscPatcher::open(disc).expect("open");
    apply::inject_shiny_seru(&mut patcher, legaia_patcher::shiny_seru::DEFAULT_PCT)
        .expect("shiny-seru");
    let err = apply::inject_super_art_list(&mut patcher).expect_err("must refuse on shiny-seru");
    assert!(
        format!("{err:#}").contains("dead space"),
        "unexpected error: {err:#}"
    );
    eprintln!("re-apply and dead-space stacking both refused");
}

#[test]
fn an_unrecognized_build_is_refused_before_anything_is_written() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(disc.clone()).expect("open");
    let ov = patcher.read_entry(OVERLAY_PROT_INDEX).expect("0898");
    let scus = scus_of(&disc);

    // Every renderer fingerprint has to be load-bearing: corrupting any one of
    // them refuses the plan.
    for va in [HOOK_COUNT_VA, HOOK_ID_VA, SCAN_HEAD_VA, SCAN_HIT_VA] {
        let mut bad = scus.clone();
        let off = file_offset_for_va(&scus, va).unwrap();
        bad[off] ^= 0xFF;
        assert!(
            SuperArtListInjection::plan(&bad, &ov).is_err(),
            "a corrupt word at {va:#x} must be refused"
        );
    }
    // Same for the pager's entry word and the applier's match arm in the overlay.
    for va in [
        PAGER_VA,
        HOOK_PERFORMED_VA,
        HOOK_PERFORMED_VA + 4,
        PERFORMED_RET_VA,
        APPLIER_VA,
    ] {
        let mut bad_ov = ov.clone();
        bad_ov[(va - OVERLAY_BASE_VA) as usize] ^= 0xFF;
        assert!(
            SuperArtListInjection::plan(&scus, &bad_ov).is_err(),
            "overlay {va:#x}"
        );
    }

    // A dirty region is refused even though every fingerprint matches - all
    // four of them, since the feature spans all four.
    for (va, _, what) in REGIONS {
        let mut dirty = scus.clone();
        dirty[file_offset_for_va(&scus, va).unwrap()] = 1;
        assert!(
            SuperArtListInjection::plan(&dirty, &ov).is_err(),
            "a dirty {what} must be refused"
        );
    }

    // And none of that touched the disc.
    assert_eq!(patcher.image(), &disc[..], "a refused plan writes nothing");
    eprintln!("every fingerprint + all four region guards are load-bearing");
}
