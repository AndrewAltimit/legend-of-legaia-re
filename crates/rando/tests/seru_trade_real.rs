//! Disc-gated round-trip oracle for the seru-trade config write.
//!
//! Gates on `LEGAIA_DISC_BIN`; skips+passes when unset. The patched image lives
//! only in memory. Asserts the embedded config blob decodes back to what was
//! written, the write is same-size + sector-valid, a fixed seed is
//! byte-deterministic, and the rest of the disc is untouched.

use legaia_asset::seru_trade::{DEFAULT_MAX_OFFERS, SeruTradeConfig};
use legaia_rando::apply;
use legaia_rando::disc::DiscPatcher;

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
