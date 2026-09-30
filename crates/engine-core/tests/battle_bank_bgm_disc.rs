//! Disc-gated: the battle bundle `0x36F + N` the intro loads for battle
//! sound set `N` carries a byte-identical copy of the `music_01` track
//! [`battle_bank_bgm_id`] names - the join the engine's battle swap plays.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset (CI has no Sony disc).

use legaia_engine_core::music_labels::{
    BATTLE_BANK_BASE_ENTRY, BATTLE_BANK_BGM_IDS, battle_bank_bgm_id,
};
use legaia_engine_core::scene::SceneHost;

/// The first `len` bytes of an entry's SEQ chunk (`pQES` onward).
fn seq_head(bytes: &[u8], len: usize) -> Option<Vec<u8>> {
    let at = bytes.windows(4).position(|w| w == b"pQES")?;
    bytes.get(at..at + len).map(<[u8]>::to_vec)
}

#[test]
fn every_battle_bundle_carries_its_music_bank_track() {
    let Some(host) = std::env::var("LEGAIA_DISC_BIN")
        .ok()
        .and_then(|d| SceneHost::open_disc(&d).ok())
    else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    for (n, &id) in BATTLE_BANK_BGM_IDS.iter().enumerate() {
        let bundle = host
            .index
            .entry_bytes(BATTLE_BANK_BASE_ENTRY + n as u32)
            .expect("battle bundle");
        let track = host
            .music_bank_entry_bytes(id)
            .expect("read music bank")
            .expect("global id resolves");
        let a = seq_head(&bundle, 256).expect("battle bundle has a SEQ");
        let b = seq_head(&track, 256).expect("music bank entry has a SEQ");
        assert_eq!(a, b, "battle bundle {n} vs global track {id}");
    }
    eprintln!("[ran] {} battle bundles joined", BATTLE_BANK_BGM_IDS.len());
    assert_eq!(battle_bank_bgm_id(-1), None, "sound set -1 loads no track");
}
