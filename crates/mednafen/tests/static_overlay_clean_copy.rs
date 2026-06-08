//! Disc + save-state-gated clean-copy proof for the static overlay pipeline.
//!
//! This is the decisive experiment behind the static overlay-extraction
//! pipeline (`legaia_asset::static_overlay`, `docs/tooling/static-overlay-pipeline.md`):
//! a runtime overlay extracted statically from `PROT.DAT` is byte-identical to
//! the image the runtime DMAs into the `0x801C0000+` overlay window. PSX
//! overlays are clean copies of a fixed-VA-linked blob (FlushCache + jump, no
//! per-load relocation), so the on-disc entry IS the loaded code.
//!
//! For each overlay with a save state that captures it resident, we:
//!  1. Locate the overlay's head bytes inside the live RAM overlay window and
//!     derive the load base from where they land -- and assert it matches the
//!     committed `base_va` (the base is RAM-confirmed, not guessed).
//!  2. Assert the on-disc bytes are byte-identical to RAM over the clean-copy
//!     region (for a code-first overlay, the whole `.text`+`.rodata` prefix;
//!     only the runtime-written `.bss` tail diverges).
//!
//! This COMPLEMENTS the dynamic capture workflow -- it does not unblock runtime
//! VALUE captures (those still need live probes); it proves overlay IDENTITY +
//! that static disassembly is faithful.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` / `extracted/PROT.DAT` / the save-state
//! library backups are absent (the disc-gated convention).

use std::path::PathBuf;

use legaia_asset::static_overlay::{self, OverlayRecord};
use legaia_mednafen::container::SaveState;
use legaia_mednafen::extract::ram_slice;
use legaia_prot::archive::Archive;

const OVERLAY_LO: u32 = 0x801C_0000;
const OVERLAY_HI: u32 = 0x8020_0000;

fn first_existing(rels: &[&str]) -> Option<PathBuf> {
    rels.iter().map(PathBuf::from).find(|p| p.is_file())
}

fn prot_dat() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    first_existing(&["extracted/PROT.DAT", "../../extracted/PROT.DAT"])
}

/// Resolve a mednafen save-state library backup by its sha256 fingerprint.
fn library_save(fp: &str) -> Option<PathBuf> {
    first_existing(&[
        &format!("saves/library/mednafen/{fp}.mcr"),
        &format!("../../saves/library/mednafen/{fp}.mcr"),
    ])
}

/// Read one overlay's as-loaded bytes from the open archive.
fn as_loaded(archive: &mut Archive, rec: &OverlayRecord) -> Vec<u8> {
    let entry = archive
        .entries
        .iter()
        .find(|e| e.index == rec.prot_index)
        .cloned()
        .unwrap_or_else(|| panic!("PROT entry {} missing", rec.prot_index));
    let mut raw = Vec::new();
    archive.read_entry(&entry, &mut raw).expect("read entry");
    static_overlay::as_loaded(&raw, rec).expect("as-loaded")
}

/// Find the byte offset of `needle` inside `haystack` (naive; needle is short).
fn find_sub(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Confirm the overlay's base by locating its head bytes in the live overlay
/// window. Returns the longest byte-identical prefix length at that base.
fn confirm_base_and_prefix(disc: &[u8], ram: &[u8], rec: &OverlayRecord) -> usize {
    let window = ram_slice(ram, OVERLAY_LO, OVERLAY_HI).expect("overlay window");
    // The first 64 bytes are const data/code at the very start of the blob;
    // they load verbatim, so where they appear in the window IS the base.
    let head = &disc[..64.min(disc.len())];
    let pos = find_sub(window, head)
        .unwrap_or_else(|| panic!("{}: head bytes not found in RAM window", rec.label));
    let derived_base = OVERLAY_LO + pos as u32;
    assert_eq!(
        derived_base, rec.base_va,
        "{} (PROT {}): RAM-derived base 0x{:08x} != committed 0x{:08x}",
        rec.label, rec.prot_index, derived_base, rec.base_va
    );

    // Longest common prefix at the base.
    let base_off = pos;
    let n = disc.len().min(window.len() - base_off);
    let mut lcp = 0usize;
    while lcp < n && disc[lcp] == window[base_off + lcp] {
        lcp += 1;
    }
    lcp
}

#[test]
fn battle_overlay_is_a_clean_copy() {
    let Some(prot) = prot_dat() else {
        eprintln!("[skip] LEGAIA_DISC_BIN / extracted/PROT.DAT missing");
        return;
    };
    // party_battle_gobu_gobu (battle phase) backup fingerprint.
    let Some(save) =
        library_save("a0c56d29eeeffd96aa809040bb807606545da792209b421274b9bea27c35e7f2")
    else {
        eprintln!("[skip] battle save-state backup not in library");
        return;
    };

    let mut archive = Archive::open(&prot).expect("open PROT.DAT");
    let map = static_overlay::overlay_map();
    let rec = map.by_label("battle_action").expect("battle_action in map");
    let disc = as_loaded(&mut archive, rec);

    let state = SaveState::from_path(&save).expect("load save state");
    let ram = state.main_ram().expect("main RAM");

    let lcp = confirm_base_and_prefix(&disc, ram, rec);

    // The committed clean-copy region must be byte-identical in RAM.
    let clean = rec.clean_copy_bytes.expect("battle has clean_copy_bytes") as usize;
    assert!(
        lcp >= clean,
        "battle clean prefix shrank: RAM-matched 0x{:x} < committed 0x{:x}",
        lcp,
        clean
    );
    eprintln!(
        "[ok] battle_action: clean copy verified, RAM-matched 0x{:x} bytes (>= committed 0x{:x})",
        lcp, clean
    );
}

#[test]
fn menu_overlay_is_a_clean_copy() {
    let Some(prot) = prot_dat() else {
        eprintln!("[skip] LEGAIA_DISC_BIN / extracted/PROT.DAT missing");
        return;
    };
    // menu_equipment_field (equipment menu open from the field, map01) backup.
    let Some(save) =
        library_save("9b0e7a6a4498c06e618d05fb1a1f9fa1d9d7d9101c5ed1e65d6175a6bcf24d98")
    else {
        eprintln!("[skip] menu_equipment_field save-state backup not in library");
        return;
    };

    let mut archive = Archive::open(&prot).expect("open PROT.DAT");
    let map = static_overlay::overlay_map();
    let rec = map.by_label("menu").expect("menu in map");
    let disc = as_loaded(&mut archive, rec);

    let state = SaveState::from_path(&save).expect("load save state");
    let ram = state.main_ram().expect("main RAM");

    // The menu overlay (PROT 0899) REPLACES the field overlay in slot A, so the
    // field VM at 0x801DE840 must NOT be a prologue here while the equip
    // aggregator FUN_801CF650 IS -- the proof these are swapping VA-alias
    // siblings, not the same overlay.
    let field_vm = legaia_mednafen::extract::read_u32_le(ram, 0x801D_E840).expect("ram word");
    assert_ne!(
        field_vm & 0xFFFF_0000,
        0x27BD_0000,
        "field VM is a prologue -- field overlay resident, not the menu overlay"
    );
    let equip_agg = legaia_mednafen::extract::read_u32_le(ram, 0x801C_F650).expect("ram word");
    assert_eq!(
        equip_agg & 0xFFFF_0000,
        0x27BD_0000,
        "equip aggregator FUN_801CF650 not a prologue -- menu overlay not resident"
    );

    let lcp = confirm_base_and_prefix(&disc, ram, rec);
    let clean = rec.clean_copy_bytes.expect("menu has clean_copy_bytes") as usize;
    assert!(
        lcp >= clean,
        "menu clean prefix shrank: RAM-matched 0x{lcp:x} < committed 0x{clean:x}"
    );
    eprintln!(
        "[ok] menu (PROT 0899): clean copy verified, RAM-matched 0x{lcp:x} bytes (>= committed 0x{clean:x})"
    );
}

#[test]
fn field_overlay_head_and_base_match_ram() {
    let Some(prot) = prot_dat() else {
        eprintln!("[skip] LEGAIA_DISC_BIN / extracted/PROT.DAT missing");
        return;
    };
    // v0_1_pre_battle_tetsu (field phase, town01) backup fingerprint.
    let Some(save) =
        library_save("4c650a3bb872b22029851a2a2e919fd8e3bb05eee19abaec9f5c82976424a779")
    else {
        eprintln!("[skip] field save-state backup not in library");
        return;
    };

    let mut archive = Archive::open(&prot).expect("open PROT.DAT");
    let map = static_overlay::overlay_map();
    let rec = map.by_label("field").expect("field in map");
    let disc = as_loaded(&mut archive, rec);

    let state = SaveState::from_path(&save).expect("load save state");
    let ram = state.main_ram().expect("main RAM");

    // The field overlay is data-section-first and a live town capture has
    // runtime-mutated globals, so it has no single clean prefix -- but the head
    // loads verbatim (confirming the base) and the MAIN_INIT prologue aligns.
    let lcp = confirm_base_and_prefix(&disc, ram, rec);
    assert!(lcp >= 1024, "field head clean prefix too short: 0x{lcp:x}");

    // MAIN_INIT FUN_801D6704 is at base+0x07eec and is a clean prologue; assert
    // the live RAM word there equals the disc word (code byte-matches).
    let off = (0x801D_6704u32 - rec.base_va) as usize;
    let disc_word = u32::from_le_bytes(disc[off..off + 4].try_into().unwrap());
    let ram_word = legaia_mednafen::extract::read_u32_le(ram, 0x801D_6704).expect("ram word");
    assert_eq!(
        disc_word, ram_word,
        "field MAIN_INIT word diverges: disc 0x{disc_word:08x} != RAM 0x{ram_word:08x}"
    );
    assert_eq!(
        disc_word & 0xFFFF_0000,
        0x27BD_0000,
        "MAIN_INIT not a prologue"
    );
    eprintln!(
        "[ok] field: base RAM-confirmed (head 0x{lcp:x} bytes identical), MAIN_INIT code byte-matches"
    );
}

#[test]
fn summon_render_overlay_region_matches_ram_at_slot_b_base() {
    let Some(prot) = prot_dat() else {
        eprintln!("[skip] LEGAIA_DISC_BIN / extracted/PROT.DAT missing");
        return;
    };
    // battle_gimard_tail_fire_a (mid-cast, PROT 0900 render overlay resident).
    let Some(save) =
        library_save("c8038c2d86f84e42c0c2148fbf499eb795b645689e7260e88f7065b5c6a7c935")
    else {
        eprintln!("[skip] Gimard mid-cast save-state backup not in library");
        return;
    };

    let mut archive = Archive::open(&prot).expect("open PROT.DAT");
    let map = static_overlay::overlay_map();
    let rec = map.by_label("summon_render").expect("summon_render in map");
    assert_eq!(rec.base_va, 0x801F_69D8, "slot-B link base");
    let disc = as_loaded(&mut archive, rec);

    let state = SaveState::from_path(&save).expect("load save state");
    let ram = state.main_ram().expect("main RAM");

    // The slot-B buffer timeshares two overlays, so there is no clean
    // whole-overlay prefix -- but the resident render region (disc file
    // 0x1000..0x2400 -> RAM 0x801F79D8..0x801F8DD8) is byte-identical, which
    // pins the slot-B base 0x801F69D8 (the on-disc entry maps file 0x1628 to the
    // 0x801F8000 anchor). This is exactly why these overlays want STATIC
    // extraction: the dynamic capture is an inseparable mix of two overlays.
    let region_start = 0x1000usize;
    let region_end = 0x2400usize;
    let ram_region = ram_slice(
        ram,
        rec.base_va + region_start as u32,
        rec.base_va + region_end as u32,
    )
    .expect("ram region");
    assert_eq!(
        &disc[region_start..region_end],
        ram_region,
        "summon_render resident region diverges from RAM at base 0x{:08x}",
        rec.base_va
    );
    eprintln!(
        "[ok] summon_render: slot-B base 0x{:08x} confirmed, render region 0x{:x}..0x{:x} byte-identical in RAM",
        rec.base_va, region_start, region_end
    );
}
