//! Disc-gated round-trip oracle for the seru-trade config write.
//!
//! Gates on `LEGAIA_DISC_BIN`; skips+passes when unset. The patched image lives
//! only in memory. Asserts the embedded config blob decodes back to what was
//! written, the write is same-size + sector-valid, a fixed seed is
//! byte-deterministic, and the rest of the disc is untouched.

use legaia_asset::seru_trade::{DEFAULT_MAX_OFFERS, SeruTradeConfig};
use legaia_rando::apply;
use legaia_rando::disc::DiscPatcher;
use legaia_rando::seru_overlay;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

#[test]
fn seru_trade_config_round_trips_and_is_deterministic() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };

    // Vanilla disc carries no seru-trade blob.
    let base = DiscPatcher::open(disc.clone()).expect("open disc");
    assert_eq!(
        apply::current_seru_trade(&base),
        None,
        "an unpatched disc must not report a seru-trade config"
    );

    let seed = 0x5E11_7EADu64;
    let mut patcher = DiscPatcher::open(disc.clone()).expect("open disc");
    let report = apply::enable_seru_trades(&mut patcher, seed, DEFAULT_MAX_OFFERS).expect("enable");
    assert!(report.config.enabled);
    assert_eq!(report.config.seed, seed);

    // Same-size, in-place write.
    assert_eq!(patcher.image().len(), disc.len(), "image size unchanged");

    // Re-decode the embedded blob off the patched image.
    let decoded = apply::current_seru_trade(&patcher).expect("config present after enable");
    assert_eq!(
        decoded,
        SeruTradeConfig {
            enabled: true,
            seed,
            max_offers: DEFAULT_MAX_OFFERS,
        }
    );

    // Exactly one file changed (SCUS); the patch is a tiny localized edit, so the
    // vast majority of disc bytes are identical.
    let diff = disc
        .iter()
        .zip(patcher.image())
        .filter(|(a, b)| a != b)
        .count();
    assert!(diff > 0, "the config write must change some bytes");
    assert!(
        diff < 4096,
        "config write touched {diff} bytes; expected a tiny localized edit"
    );

    // Re-running with a different seed overwrites the prior blob (idempotent slot).
    let new_seed = 0x1234_5678u64;
    let report2 = apply::enable_seru_trades(&mut patcher, new_seed, 6).expect("re-enable");
    assert_eq!(report2.config.seed, new_seed);
    assert_eq!(report2.config.max_offers, 6);
    assert_eq!(patcher.image().len(), disc.len());
    assert_eq!(
        apply::current_seru_trade(&patcher).map(|c| (c.seed, c.max_offers)),
        Some((new_seed, 6))
    );

    // Fixed seed is byte-deterministic.
    let mut p2 = DiscPatcher::open(disc).expect("reopen");
    apply::enable_seru_trades(&mut p2, seed, DEFAULT_MAX_OFFERS).expect("enable again");
    let mut p1 =
        DiscPatcher::open(load_disc().expect("disc still readable")).expect("reopen baseline");
    apply::enable_seru_trades(&mut p1, seed, DEFAULT_MAX_OFFERS).expect("enable baseline");
    assert_eq!(p1.image(), p2.image(), "fixed seed is byte-deterministic");
}

#[test]
fn overlay_slice_patches_pochi_slot_stub_and_detour() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };

    let mut patcher = DiscPatcher::open(disc.clone()).expect("open disc");
    let report = apply::inject_overlay_slice(&mut patcher).expect("inject overlay slice");

    // Same-size, in-place.
    assert_eq!(patcher.image().len(), disc.len(), "image size unchanged");

    // The chosen host is a real pochi-filler slot and the baked LBA is its
    // actual on-disc LBA.
    let host = patcher
        .read_entry(report.pochi_index)
        .expect("read host slot");
    assert!(report.sectors >= 1);
    assert_eq!(
        patcher.entry_disc_lba(report.pochi_index),
        Some(report.lba),
        "stub LBA matches the host slot's disc LBA"
    );

    // The overlay bytes landed at the head of the host slot.
    let expected = seru_overlay::words_to_bytes(&seru_overlay::assemble_sentinel_overlay());
    assert_eq!(
        &host[..expected.len()],
        &expected[..],
        "overlay written to slot head"
    );

    // The detour lives in the field overlay (PROT 0897) at the op-0x49 arm edge,
    // and jumps to the loader stub.
    let overlay = patcher
        .read_entry(seru_overlay::SHOP_OVERLAY_PROT_INDEX)
        .expect("read field overlay");
    let hook_off = (seru_overlay::SHOP_HOOK_VA - seru_overlay::SHOP_OVERLAY_BASE) as usize;
    let detour = u32::from_le_bytes(overlay[hook_off..hook_off + 4].try_into().unwrap());
    assert_eq!(
        (detour & 0x03ff_ffff) << 2,
        seru_overlay::STUB_VA & 0x0fff_ffff,
        "op-0x49 arm edge detours to the loader stub"
    );

    // The stub lands in the SCUS rodata gap and gates on the sub-op
    // (first word = lbu t3,0(s6); opcode 0x24, rt=t3=11, rs=s6=22).
    let scus = patcher
        .read_named_file("SCUS_942.54")
        .expect("SCUS present");
    let stub_off =
        legaia_asset::item_names::file_offset_for_va(&scus, seru_overlay::STUB_VA).unwrap();
    let stub0 = u32::from_le_bytes(scus[stub_off..stub_off + 4].try_into().unwrap());
    assert_eq!(
        stub0,
        0x9000_0000 | (22 << 21) | (11 << 16),
        "stub gates on the op-0x49 sub-op (lbu t3,0(s6))"
    );

    // Determinism: a fresh apply yields a byte-identical image.
    let mut p2 = DiscPatcher::open(disc).expect("reopen");
    apply::inject_overlay_slice(&mut p2).expect("re-inject");
    assert_eq!(
        p2.image(),
        patcher.image(),
        "overlay-slice patch is deterministic"
    );
}

#[test]
fn dead_mode_slice_patches_gap_routines_detour_and_mode_table() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };

    let mut patcher = DiscPatcher::open(disc.clone()).expect("open disc");
    let report = apply::inject_overlay_slice_dead_mode(&mut patcher).expect("inject dead-mode");

    assert_eq!(patcher.image().len(), disc.len(), "image size unchanged");

    // Overlay at the pochi head; baked LBA matches the slot.
    let host = patcher
        .read_entry(report.pochi_index)
        .expect("read host slot");
    let overlay = seru_overlay::words_to_bytes(&seru_overlay::assemble_sentinel_overlay());
    assert_eq!(&host[..overlay.len()], &overlay[..], "overlay at slot head");
    assert_eq!(patcher.entry_disc_lba(report.pochi_index), Some(report.lba));

    // op-0x49 arm edge detours to the trigger (== STUB_VA).
    let fov = patcher
        .read_entry(seru_overlay::SHOP_OVERLAY_PROT_INDEX)
        .expect("field overlay");
    let hook_off = (seru_overlay::SHOP_HOOK_VA - seru_overlay::SHOP_OVERLAY_BASE) as usize;
    let detour = u32::from_le_bytes(fov[hook_off..hook_off + 4].try_into().unwrap());
    assert_eq!(
        (detour & 0x03ff_ffff) << 2,
        seru_overlay::TRIGGER_VA & 0x0fff_ffff,
        "detour -> dead-mode trigger"
    );

    let scus = patcher
        .read_named_file("SCUS_942.54")
        .expect("SCUS present");
    let word_at = |va: u32| -> u32 {
        let off = legaia_asset::item_names::file_offset_for_va(&scus, va).unwrap();
        u32::from_le_bytes(scus[off..off + 4].try_into().unwrap())
    };

    // Trigger: stashes the interrupted mode then requests the dead mode. Head is
    // `lui at,hi(MODE_INDEX_VA)`; the `addiu v0,zero,DEAD_MODE_INDEX` is word 4.
    assert_eq!(
        word_at(seru_overlay::TRIGGER_VA) >> 26,
        0x0F,
        "trigger head is a lui (origin-mode read)"
    );
    assert_eq!(
        word_at(seru_overlay::TRIGGER_VA + 16),
        0x2402_0000 | seru_overlay::DEAD_MODE_INDEX as u32,
        "trigger requests the dead mode (addiu v0,zero,dead_mode)"
    );
    // Loader: first word is the stack frame (addiu sp,sp,-8).
    assert_eq!(
        word_at(seru_overlay::MODE_INIT_VA),
        0x27BD_FFF8,
        "loader head = addiu sp,sp,-8"
    );
    // The dead mode's mode-table handler now points at our loader.
    assert_eq!(
        word_at(seru_overlay::dead_mode_handler_va()),
        seru_overlay::MODE_INIT_VA,
        "dead-mode handler repointed to the loader"
    );

    // Determinism.
    let mut p2 = DiscPatcher::open(disc).expect("reopen");
    apply::inject_overlay_slice_dead_mode(&mut p2).expect("re-inject");
    assert_eq!(p2.image(), patcher.image(), "dead-mode patch deterministic");
}

#[test]
fn warp_slice_patches_gap_detour_and_fun80025980() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };

    let mut patcher = DiscPatcher::open(disc.clone()).expect("open disc");
    let report = apply::inject_overlay_slice_warp(&mut patcher).expect("inject warp");

    assert_eq!(patcher.image().len(), disc.len(), "image size unchanged");

    // Overlay at the pochi head; baked LBA matches.
    let host = patcher
        .read_entry(report.pochi_index)
        .expect("read host slot");
    let overlay = seru_overlay::words_to_bytes(&seru_overlay::assemble_sentinel_overlay());
    assert_eq!(&host[..overlay.len()], &overlay[..], "overlay at slot head");
    assert_eq!(patcher.entry_disc_lba(report.pochi_index), Some(report.lba));

    // op-0x49 arm edge detours to the warp trigger (== STUB_VA).
    let fov = patcher
        .read_entry(seru_overlay::SHOP_OVERLAY_PROT_INDEX)
        .expect("field overlay");
    let hook_off = (seru_overlay::SHOP_HOOK_VA - seru_overlay::SHOP_OVERLAY_BASE) as usize;
    let detour = u32::from_le_bytes(fov[hook_off..hook_off + 4].try_into().unwrap());
    assert_eq!(
        (detour & 0x03ff_ffff) << 2,
        seru_overlay::WARP_TRIGGER_VA & 0x0fff_ffff,
        "detour -> warp trigger"
    );

    let scus = patcher
        .read_named_file("SCUS_942.54")
        .expect("SCUS present");
    let word_at = |va: u32| -> u32 {
        let off = legaia_asset::item_names::file_offset_for_va(&scus, va).unwrap();
        u32::from_le_bytes(scus[off..off + 4].try_into().unwrap())
    };

    // FUN_80025980's overlay-load site is detoured to the redirect.
    assert_eq!(
        (word_at(seru_overlay::WARP_INIT_DETOUR_VA) & 0x03ff_ffff) << 2,
        seru_overlay::WARP_REDIRECT_VA & 0x0fff_ffff,
        "FUN_80025980 load site -> redirect"
    );
    // The redirect's .ours load targets slot A (a2 = lui 0x801C).
    assert_eq!(
        word_at(seru_overlay::WARP_REDIRECT_VA + 12 * 4) >> 16,
        0x3C06,
        "redirect word 12 is lui a2 (slot-A base load)"
    );
    // The warp trigger requests mode 24 (0x18) somewhere in its body.
    let trig: Vec<u32> = (0..17)
        .map(|i| word_at(seru_overlay::WARP_TRIGGER_VA + i * 4))
        .collect();
    assert!(
        trig.contains(&(0x2402_0000 | 0x18)),
        "trigger sets mode 0x18 (addiu v0,zero,0x18)"
    );

    // Determinism.
    let mut p2 = DiscPatcher::open(disc).expect("reopen");
    apply::inject_overlay_slice_warp(&mut p2).expect("re-inject");
    assert_eq!(p2.image(), patcher.image(), "warp patch deterministic");
}

/// Picker-trigger build: the trade overlay is armed from the shop's Buy/Sell/Quit
/// picker renderer (`FUN_801d4868`, overlay 0899) instead of the op-0x49 arm,
/// dodging the same-frame mode-0x17 override at a merchant. Asserts the picker
/// prologue is detoured, the picker stub + redirect land in the SCUS gap, the
/// menu-overlay byte layout matches the recognized build, and the patch is
/// deterministic + size-preserving.
#[test]
fn picker_slice_patches_menu_overlay_gap_and_fun80025980() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };

    let mut patcher = DiscPatcher::open(disc.clone()).expect("open disc");
    let report = apply::inject_overlay_slice_picker(&mut patcher).expect("inject picker");

    assert_eq!(patcher.image().len(), disc.len(), "image size unchanged");

    // Draw overlay sits at the pochi head with the baked LBA.
    let host = patcher
        .read_entry(report.pochi_index)
        .expect("read host slot");
    let overlay = seru_overlay::words_to_bytes(&seru_overlay::assemble_draw_overlay());
    assert_eq!(
        &host[..overlay.len()],
        &overlay[..],
        "draw overlay at slot head"
    );
    assert_eq!(patcher.entry_disc_lba(report.pochi_index), Some(report.lba));

    // Picker prologue (overlay 0899) detours to the picker trigger (== STUB_VA).
    let mov = patcher
        .read_entry(seru_overlay::PICKER_MENU_PROT_INDEX)
        .expect("menu overlay");
    let hook_off = (seru_overlay::PICKER_RENDER_VA - seru_overlay::SLOT_A_BASE) as usize;
    // The two words below the detour are the recognized prologue head.
    assert_eq!(
        (u32::from_le_bytes(mov[hook_off..hook_off + 4].try_into().unwrap()) & 0xfc00_0000),
        0x0800_0000,
        "picker hook word 0 is a j instruction"
    );
    let detour = u32::from_le_bytes(mov[hook_off..hook_off + 4].try_into().unwrap());
    assert_eq!(
        (detour & 0x03ff_ffff) << 2,
        seru_overlay::PICKER_TRIGGER_VA & 0x0fff_ffff,
        "picker detour -> picker trigger"
    );

    let scus = patcher
        .read_named_file("SCUS_942.54")
        .expect("SCUS present");
    let word_at = |va: u32| -> u32 {
        let off = legaia_asset::item_names::file_offset_for_va(&scus, va).unwrap();
        u32::from_le_bytes(scus[off..off + 4].try_into().unwrap())
    };

    // The picker stub requests mode 24 (0x18) and sub-id 7 in its body.
    let stub: Vec<u32> = (0..24)
        .map(|i| word_at(seru_overlay::PICKER_TRIGGER_VA + i * 4))
        .collect();
    assert!(
        stub.contains(&(0x2402_0000 | 0x18)),
        "stub sets mode 0x18 (addiu v0,zero,0x18)"
    );
    assert!(
        stub.contains(&(0x2402_0000 | u32::from(seru_overlay::WARP_SUBID))),
        "stub sets sub-id (addiu v0,zero,WARP_SUBID)"
    );
    // The stub fits its 0x60-byte slot: word 24 onward is the (untouched) redirect.
    assert_eq!(
        seru_overlay::PICKER_TRIGGER_VA + 24 * 4,
        seru_overlay::WARP_REDIRECT_VA,
        "stub abuts the redirect exactly"
    );

    // FUN_80025980's overlay-load site is detoured to the redirect.
    assert_eq!(
        (word_at(seru_overlay::WARP_INIT_DETOUR_VA) & 0x03ff_ffff) << 2,
        seru_overlay::WARP_REDIRECT_VA & 0x0fff_ffff,
        "FUN_80025980 load site -> redirect"
    );

    // Determinism.
    let mut p2 = DiscPatcher::open(disc).expect("reopen");
    apply::inject_overlay_slice_picker(&mut p2).expect("re-inject");
    assert_eq!(p2.image(), patcher.image(), "picker patch deterministic");
}

/// Native fourth "Trade" row: clamp 3->4 in the dispatcher, renderer epilogue
/// detoured to the draw stub, and stub + "@Trade" label in the SCUS gap. Asserts
/// the recognized-build guards hold, the edits land, the stub fits its window, and
/// the patch is deterministic + size-preserving. Selecting the row is a no-op
/// here (the trade action is wired separately), so this just validates styling.
#[test]
fn native_trade_row_patches_clamp_renderer_and_gap() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };

    let mut patcher = DiscPatcher::open(disc.clone()).expect("open disc");
    apply::inject_native_trade_row(&mut patcher).expect("inject native trade row");

    assert_eq!(patcher.image().len(), disc.len(), "image size unchanged");

    let mov = patcher
        .read_entry(seru_overlay::PICKER_MENU_PROT_INDEX)
        .expect("menu overlay");
    let word = |off: usize| u32::from_le_bytes(mov[off..off + 4].try_into().unwrap());

    // Clamp bumped 3 -> 4.
    let clamp_off = (seru_overlay::CLAMP_VA - seru_overlay::SLOT_A_BASE) as usize;
    assert_eq!(
        word(clamp_off),
        seru_overlay::CLAMP_NEW,
        "cursor clamp -> 4"
    );

    // Picker box grown one row taller (sprite-def height for id 0x2a).
    let box_h_off = (seru_overlay::BOX_H_VA - seru_overlay::SLOT_A_BASE) as usize;
    assert_eq!(
        u16::from_le_bytes(mov[box_h_off..box_h_off + 2].try_into().unwrap()),
        seru_overlay::BOX_H_NEW,
        "picker box height -> 4 rows"
    );

    // Renderer in-body detour to the row-4 stub.
    let row4_off = (seru_overlay::ROW4_DETOUR_VA - seru_overlay::SLOT_A_BASE) as usize;
    assert_eq!(
        (word(row4_off) & 0x03ff_ffff) << 2,
        seru_overlay::ROW4_STUB_VA & 0x0fff_ffff,
        "renderer detour -> row-4 stub"
    );

    let scus = patcher
        .read_named_file("SCUS_942.54")
        .expect("SCUS present");
    let sb =
        legaia_asset::item_names::file_offset_for_va(&scus, seru_overlay::ROW4_STUB_VA).unwrap();
    let stub = seru_overlay::words_to_bytes(&seru_overlay::assemble_row4_draw_stub());
    assert_eq!(&scus[sb..sb + stub.len()], &stub[..], "stub bytes in gap");
    // The "@Trade" label sits past the stub, within the reserved window.
    assert!(
        seru_overlay::TRADE_STR_VA >= seru_overlay::ROW4_STUB_VA + stub.len() as u32,
        "label does not overlap the stub"
    );
    let lb =
        legaia_asset::item_names::file_offset_for_va(&scus, seru_overlay::TRADE_STR_VA).unwrap();
    assert_eq!(
        &scus[lb..lb + seru_overlay::TRADE_STR.len()],
        seru_overlay::TRADE_STR,
        "@Trade label in gap"
    );

    // Determinism.
    let mut p2 = DiscPatcher::open(disc).expect("reopen");
    apply::inject_native_trade_row(&mut p2).expect("re-inject");
    assert_eq!(
        p2.image(),
        patcher.image(),
        "native trade row deterministic"
    );
}

/// Full in-shop Trade vendor: native row + reorder (Buy/Sell/Trade/Quit) + the
/// Trade sub-mode wiring (dispatch + entry detours + SCUS handler), no warp.
/// Asserts each overlay-0899 edit + each SCUS-gap routine lands, the gap routines
/// don't overlap, and the patch is deterministic.
#[test]
fn trade_flow_patches_in_shop_submode_and_reorder() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };

    let mut patcher = DiscPatcher::open(disc.clone()).expect("open disc");
    apply::inject_trade_full(&mut patcher).expect("inject trade flow");
    assert_eq!(patcher.image().len(), disc.len(), "image size unchanged");

    let mov = patcher
        .read_entry(seru_overlay::PICKER_MENU_PROT_INDEX)
        .expect("menu overlay");
    let base = seru_overlay::SLOT_A_BASE;
    let mword = |va: u32| u32::from_le_bytes(mov[(va - base) as usize..][..4].try_into().unwrap());
    let detour_target = |va: u32| (mword(va) & 0x03ff_ffff) << 2;

    // Reorder: clamp 3->4, box height, row-2 string swapped to @Trade.
    assert_eq!(
        mword(seru_overlay::CLAMP_VA),
        seru_overlay::CLAMP_NEW,
        "clamp -> 4"
    );
    let bh = (seru_overlay::BOX_H_VA - base) as usize;
    assert_eq!(
        u16::from_le_bytes(mov[bh..bh + 2].try_into().unwrap()),
        seru_overlay::BOX_H_NEW,
        "box height -> 4 rows"
    );
    assert_eq!(
        [
            mword(seru_overlay::ROW2_STR_LOAD_VA),
            mword(seru_overlay::ROW2_STR_LOAD_VA + 4)
        ],
        seru_overlay::row2_str_load_new(),
        "row-2 text load swapped to @Trade"
    );

    // The three detours into overlay 0899 jump to their gap routines.
    assert_eq!(
        detour_target(seru_overlay::ROW4_DETOUR_VA),
        seru_overlay::ROW4_STUB_VA & 0x0fff_ffff,
        "renderer detour -> row-4 stub"
    );
    assert_eq!(
        detour_target(seru_overlay::TRADE_DISPATCH_VA),
        seru_overlay::TRADE_DISPATCH_STUB_VA & 0x0fff_ffff,
        "dispatch detour -> dispatch stub"
    );
    assert_eq!(
        detour_target(seru_overlay::ENTRY_VA),
        seru_overlay::ENTRY_STUB_VA & 0x0fff_ffff,
        "FUN_801dafd4 entry detour -> entry stub"
    );

    let scus = patcher.read_named_file("SCUS_942.54").expect("SCUS");
    let gap = |va: u32, bytes: &[u8]| {
        let off = legaia_asset::item_names::file_offset_for_va(&scus, va).unwrap();
        assert_eq!(&scus[off..off + bytes.len()], bytes, "gap blob @ {va:#x}");
    };
    // Each gap routine + string lands at its VA.
    let row4 = seru_overlay::words_to_bytes(&seru_overlay::assemble_row4_draw_stub_str(
        seru_overlay::QUIT_STR_VA,
    ));
    let entry = seru_overlay::words_to_bytes(&seru_overlay::assemble_trade_entry_stub());
    let disp = seru_overlay::words_to_bytes(&seru_overlay::assemble_trade_dispatch_stub());
    let handler = seru_overlay::words_to_bytes(&seru_overlay::assemble_trade_handler());
    gap(seru_overlay::ROW4_STUB_VA, &row4);
    gap(seru_overlay::TRADE_STR_VA, seru_overlay::TRADE_STR);
    gap(seru_overlay::ENTRY_STUB_VA, &entry);
    gap(seru_overlay::TRADE_DISPATCH_STUB_VA, &disp);
    gap(seru_overlay::TRADE_HANDLER_VA, &handler);
    gap(seru_overlay::TITLE_STR_VA, seru_overlay::TITLE_STR);

    // Lower-gap routines don't overlap (entry < dispatch < handler), and the grown
    // handler body stays below the flee-EXP routine window at TRADE_HANDLER_END.
    assert!(
        seru_overlay::ENTRY_STUB_VA + entry.len() as u32 <= seru_overlay::TRADE_DISPATCH_STUB_VA
    );
    assert!(
        seru_overlay::TRADE_DISPATCH_STUB_VA + disp.len() as u32 <= seru_overlay::TRADE_HANDLER_VA
    );
    assert!(
        seru_overlay::TRADE_HANDLER_VA + handler.len() as u32 <= seru_overlay::TRADE_HANDLER_END,
        "trade handler overruns the flee-EXP window"
    );
    // Upper row-4 gap tail packs cleanly: row-4 stub < @Trade < title < flag, all
    // below the config blob at 0x8007AF00.
    assert!(seru_overlay::TRADE_STR_VA >= seru_overlay::ROW4_STUB_VA + row4.len() as u32);
    let trade_str_len = seru_overlay::TRADE_STR.len() as u32;
    let title_str_len = seru_overlay::TITLE_STR.len() as u32;
    assert!(seru_overlay::TITLE_STR_VA >= seru_overlay::TRADE_STR_VA + trade_str_len);
    assert!(seru_overlay::TRADE_ACTIVE_VA >= seru_overlay::TITLE_STR_VA + title_str_len);
    // The private flag cell resolves to real (all-zero) gap space below the config blob.
    let flag_off =
        legaia_asset::item_names::file_offset_for_va(&scus, seru_overlay::TRADE_ACTIVE_VA).unwrap();
    assert_eq!(
        &scus[flag_off..flag_off + 4],
        &[0u8; 4],
        "flag cell is dead space"
    );

    // The dispatch stub raises the private TRADE_ACTIVE flag (lui at,hi(flag) + sw),
    // and the entry stub / handler reference the same flag cell.
    let flag_lui = 0x3c01_0000 | (seru_overlay::TRADE_ACTIVE_VA.wrapping_add(0x8000) >> 16);
    let disp_words: Vec<u32> = disp
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
        .collect();
    let entry_words: Vec<u32> = entry
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
        .collect();
    assert!(
        disp_words.contains(&flag_lui),
        "dispatch stub raises the TRADE_ACTIVE flag"
    );
    assert!(
        entry_words.contains(&flag_lui),
        "entry stub gates on the TRADE_ACTIVE flag"
    );

    // Determinism.
    let mut p2 = DiscPatcher::open(disc).expect("reopen");
    apply::inject_trade_full(&mut p2).expect("re-inject");
    assert_eq!(p2.image(), patcher.image(), "trade flow deterministic");
}
