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
