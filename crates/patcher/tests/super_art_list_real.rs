//! Disc-gated oracle for **show Super Arts on the in-battle move list**
//! (`legaia_patcher::super_art_list` + `apply::inject_super_art_list`).
//!
//! The feature adds each character's five Super Arts to the Tactical-Arts list
//! `FUN_80034358` draws, through three same-size detours into that renderer
//! (`0x800343C4` count / `0x80034450` id / `0x8003474C` miss-draw), routines +
//! a name blob in the verified-dead SCUS regions `shiny_seru::ARENA1_VA` and
//! `SCUS_GAP_VA`, and a wholesale in-place replacement of the list pager
//! `FUN_801D3748` inside PROT 0898.
//!
//! These apply it to a scratch copy of the real disc and assert, off the patched
//! image, that every hosted region was all-zero pre-patch; each detour became
//! exactly the planned `j routine` (and that the **one-word** draw detour left
//! `0x80034750` - a live jump target - untouched); the routines, offset table
//! and name blob land exactly where the plan says and every name reads back
//! through its own offset; the replacement pager fits inside the original
//! 81-instruction body, is nop-padded to it, and its one caller still targets
//! its entry; the synthetic id space is free in the disc's own arts-name table;
//! **every byte that changed in `SCUS_942.54` and PROT 0898 is inside a planned
//! edit, and no other file on the disc moved at all** (which is what makes the
//! toggle byte-inert when off); every touched sector stays EDC/ECC-valid; the
//! run is byte-deterministic; and an unrecognized build, a dirty arena or a
//! prior arena feature is refused without writing anything.
//!
//! Gates on `LEGAIA_DISC_BIN`; skips and passes when unset.
//!
//! HONESTY GATE: this proves only WHERE the bytes land, never in-game
//! behaviour. A live battle playtest - open Triangle, page to the added rows,
//! see five correctly-named Super Arts and no blank rows for Terra - is still
//! required before calling the feature done.

use legaia_asset::item_names::file_offset_for_va;
use legaia_iso::iso9660::{find_file_in_image, read_file_in_image};
use legaia_iso::raw::{SECTOR_SIZE, USER_DATA_SIZE};
use legaia_patcher::apply;
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::shiny_seru::{ARENA1_END_VA, ARENA1_VA, SCUS_GAP_END_VA, SCUS_GAP_VA};
use legaia_patcher::super_art_list::{
    HOOK_COUNT_VA, HOOK_DRAW_VA, HOOK_ID_VA, OVERLAY_BASE_VA, OVERLAY_PROT_INDEX, PAGER_VA,
    PAGER_WORDS, SUPER_ARTS_PER_CHAR, SYN_ID_BASE, SuperArtListInjection, super_art_names,
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
    let scus = scus_of(original);
    let mut patcher = DiscPatcher::open(original.to_vec()).expect("open disc");
    let ov = patcher.read_entry(OVERLAY_PROT_INDEX).expect("read 0898");
    let plan = SuperArtListInjection::plan(&scus, &ov).expect("plan");
    apply::inject_super_art_list(&mut patcher).expect("inject");
    (patcher.into_image(), plan)
}

#[test]
fn hosted_regions_are_all_zero_before_patch() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let scus = scus_of(&disc);
    for (va, end) in [(ARENA1_VA, ARENA1_END_VA), (SCUS_GAP_VA, SCUS_GAP_END_VA)] {
        let off = file_offset_for_va(&scus, va).expect("VA");
        let len = (end - va) as usize;
        assert!(
            scus[off..off + len].iter().all(|&b| b == 0),
            "{va:#x}..{end:#x} must be dead space"
        );
    }
    eprintln!("arena 1 + rodata gap are all-zero on the real disc");
}

#[test]
fn synthetic_id_space_is_free_in_the_discs_own_arts_table() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let scus = scus_of(&disc);
    let rows = legaia_art::arts_table::raw_records_from_scus(&scus).expect("arts table");
    assert_eq!(rows.len(), 45, "fifteen regular arts per character");
    let max = rows.iter().map(|r| r.index).max().unwrap();
    assert!(
        max < SYN_ID_BASE,
        "real art ids stop at {max:#x}, below the synthetic base {SYN_ID_BASE:#x}"
    );
    eprintln!("real art ids run 0x00..={max:#x}; {SYN_ID_BASE:#x}.. is free");
}

#[test]
fn detours_land_and_the_shared_jump_target_survives() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let before = scus_of(&disc);
    let stock_next = word_at_va(&before, HOOK_DRAW_VA + 4);
    let (img, plan) = patched(&disc);
    let after = scus_of(&img);

    assert_eq!(word_at_va(&after, HOOK_COUNT_VA), j_word(plan.count_va));
    assert_eq!(word_at_va(&after, HOOK_COUNT_VA + 4), 0, "nop");
    assert_eq!(word_at_va(&after, HOOK_ID_VA), j_word(plan.id_va));
    assert_eq!(word_at_va(&after, HOOK_ID_VA + 4), 0, "nop");
    assert_eq!(word_at_va(&after, HOOK_DRAW_VA), j_word(plan.draw_va));
    // The word after the draw detour is `lw t0,0x2c(sp)` and is ALSO reached by
    // `j 0x80034750` from the hit path, so overwriting it would break the loop
    // bound for every drawn row. It must be byte-identical to retail.
    assert_eq!(
        word_at_va(&after, HOOK_DRAW_VA + 4),
        stock_next,
        "the draw detour is one word; 0x80034750 stays retail"
    );
    eprintln!(
        "detours: count -> {:#x}, id -> {:#x}, draw -> {:#x}",
        plan.count_va, plan.id_va, plan.draw_va
    );
}

#[test]
fn names_read_back_through_their_own_offsets() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let (img, plan) = patched(&disc);
    let scus = scus_of(&img);
    let offtab = file_offset_for_va(&scus, plan.offtab_va).expect("offtab VA");
    let blob = file_offset_for_va(&scus, plan.blob_va).expect("blob VA");
    let want = super_art_names();
    assert_eq!(want.len(), 15);
    for (i, name) in want.iter().enumerate() {
        let off = scus[offtab + i] as usize;
        let start = blob + off;
        let end = start + scus[start..].iter().position(|&b| b == 0).expect("NUL");
        assert_eq!(
            std::str::from_utf8(&scus[start..end]).unwrap(),
            name,
            "name {i} reads back through its offset"
        );
    }
    // The routine indexes the table as `character * 5 + k`, so the fifteen
    // entries have to be grouped by character in `Character::all()` order.
    for (c, ch) in legaia_art::queue::Character::all().into_iter().enumerate() {
        for (k, s) in legaia_art::SUPER_ARTS
            .iter()
            .filter(|s| s.character == ch)
            .enumerate()
        {
            assert_eq!(
                want[c * SUPER_ARTS_PER_CHAR + k],
                s.name,
                "slot {c}*5+{k} is {ch:?}'s Super Art {k}"
            );
        }
    }
    eprintln!(
        "15 names at {:#x}, table at {:#x}",
        plan.blob_va, plan.offtab_va
    );
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
        .find(|e| e.prot_index == Some(OVERLAY_PROT_INDEX))
        .expect("pager edit")
        .bytes;
    assert_eq!(planned.len(), PAGER_WORDS * 4, "same size as retail's body");
    assert_eq!(body, planned.as_slice());
    assert_eq!(
        overlay_word(&after, PAGER_VA + (PAGER_WORDS as u32) * 4),
        overlay_word(&before, PAGER_VA + (PAGER_WORDS as u32) * 4),
        "the next function is untouched"
    );
    // The replacement must be shorter than the body it replaces: the padding is
    // real, not an artifact of a same-length rewrite.
    let live = planned
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
        .rposition(|w| w != 0)
        .unwrap()
        + 1;
    assert!(
        live < PAGER_WORDS,
        "replacement is {live} words in an {PAGER_WORDS}-word body"
    );
    eprintln!("pager: {live} live words + {} nops", PAGER_WORDS - live);
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
            (scus_first..scus_last).contains(&s) || (ov_first..ov_last).contains(&s),
            "sector {s} changed but belongs to neither SCUS nor PROT {OVERLAY_PROT_INDEX}"
        );
        // 3. Every touched sector is still EDC/ECC-valid.
        assert!(
            legaia_iso::write::mode2_form1_sector_is_valid(&img[a..a + SECTOR_SIZE]),
            "sector {s} must stay EDC/ECC-valid"
        );
    }
    assert!(touched > 0);
    eprintln!(
        "{touched} sectors changed, all inside SCUS / PROT {OVERLAY_PROT_INDEX}, all EDC/ECC-valid"
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
fn re_applying_or_stacking_an_arena_feature_is_refused() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // Re-apply: the arena is no longer dead space.
    let (once, _) = patched(&disc);
    let mut patcher = DiscPatcher::open(once).expect("open patched");
    let err = apply::inject_super_art_list(&mut patcher).expect_err("second apply must fail");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("dead space") || msg.contains("unrecognized build"),
        "unexpected error: {msg}"
    );

    // Stacking on shiny-Seru: the arena bytes are already taken, so the plan
    // refuses rather than silently writing over another feature's routine.
    let mut patcher = DiscPatcher::open(disc).expect("open");
    apply::inject_shiny_seru(&mut patcher, legaia_patcher::shiny_seru::DEFAULT_PCT)
        .expect("shiny-seru");
    let err = apply::inject_super_art_list(&mut patcher).expect_err("must refuse on shiny-seru");
    assert!(
        format!("{err:#}").contains("dead space"),
        "unexpected error: {err:#}"
    );
    eprintln!("re-apply and arena stacking both refused");
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
    for va in [HOOK_COUNT_VA, HOOK_ID_VA, HOOK_DRAW_VA, HOOK_DRAW_VA + 4] {
        let mut bad = scus.clone();
        let off = file_offset_for_va(&scus, va).unwrap();
        bad[off] ^= 0xFF;
        assert!(
            SuperArtListInjection::plan(&bad, &ov).is_err(),
            "a corrupt word at {va:#x} must be refused"
        );
    }
    // Same for the pager's entry word in the overlay.
    let mut bad_ov = ov.clone();
    bad_ov[(PAGER_VA - OVERLAY_BASE_VA) as usize] ^= 0xFF;
    assert!(SuperArtListInjection::plan(&scus, &bad_ov).is_err());

    // A dirty arena is refused even though every fingerprint matches.
    let mut dirty = scus.clone();
    dirty[file_offset_for_va(&scus, ARENA1_VA).unwrap()] = 1;
    assert!(SuperArtListInjection::plan(&dirty, &ov).is_err());

    // And none of that touched the disc.
    assert_eq!(patcher.image(), &disc[..], "a refused plan writes nothing");
    eprintln!("every fingerprint + the arena guard are load-bearing");
}
