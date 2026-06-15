//! Disc-gated runtime oracle for the **seru-trade** feature — the engine-side
//! counterpart to the rando crate's `seru_trade_real` config round-trip.
//!
//! The rando test proves the config blob (enabled flag + master seed) is
//! *written* to the disc faithfully. What it can't prove is that a runtime
//! *reads it and lets the player swap a seru*. This test closes that: it patches
//! the seru-trade config onto a scratch copy of the real disc (the surgical
//! `--seru-trade` edit), re-decodes the config straight from the patched SCUS
//! bytes, installs it into a clean-room [`World`] holding a party with known
//! seru, opens a trade session, confirms a trade, and asserts the runtime
//! rewrites the owner's spell list to the offered seru — and that the offers
//! reseed across a two-in-game-hour boundary.
//!
//! A baseline pass over the *unpatched* SCUS first confirms the engine reports
//! trading disabled, so the patched assertions can't pass vacuously.
//!
//! Skips without `LEGAIA_DISC_BIN` (CLAUDE.md convention).

use legaia_asset::seru_trade::{DEFAULT_MAX_OFFERS, SECONDS_PER_RESEED};
use legaia_engine_core::seru_trade::TradeResult;
use legaia_engine_core::world::World;
use legaia_rando::apply;
use legaia_rando::disc::DiscPatcher;
use legaia_save::{CharacterRecord, Party, SpellList};

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

fn ch_with_spells(ids: &[u8]) -> CharacterRecord {
    let mut r = CharacterRecord::zeroed();
    let mut list = SpellList::default();
    for (i, &id) in ids.iter().enumerate() {
        list.ids[i] = id;
        list.levels[i] = 1;
    }
    list.count = ids.len() as u8;
    r.set_spell_list(list);
    r
}

/// A party whose members own a spread of player seru (0x81..=0x95).
fn party() -> Party {
    Party {
        members: vec![
            ch_with_spells(&[0x81, 0x82, 0x83]),
            ch_with_spells(&[0x90, 0x91]),
            ch_with_spells(&[0x88]),
        ],
    }
}

#[test]
fn seru_trade_runtime_swaps_and_reseeds() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };

    // --- Baseline: unpatched disc reports trading disabled. ---
    let base = DiscPatcher::open(disc.clone()).expect("open disc");
    let vanilla_scus = base
        .read_named_file("SCUS_942.54")
        .expect("SCUS present on disc");
    let mut w0 = World {
        roster: party(),
        ..World::default()
    };
    assert!(
        !w0.install_seru_trade_config(&vanilla_scus),
        "unpatched disc must not enable seru trading"
    );
    assert!(
        w0.open_seru_trade(0).is_none(),
        "no trades without a config"
    );

    // --- Patch the config onto a scratch copy, re-decode off the patched SCUS. ---
    let seed = 0xBADC0DEu64;
    let mut patcher = DiscPatcher::open(disc).expect("open disc");
    apply::enable_seru_trades(&mut patcher, seed, DEFAULT_MAX_OFFERS).expect("enable seru trade");
    let patched_scus = patcher
        .read_named_file("SCUS_942.54")
        .expect("SCUS present after patch");

    let mut w = World {
        roster: party(),
        play_time_seconds: 0,
        ..World::default()
    };
    assert!(
        w.install_seru_trade_config(&patched_scus),
        "patched disc enables seru trading"
    );
    assert!(w.seru_trade_enabled());

    // Open at a vendor; the party owns seru, so the vendor has trades.
    let vendor_id = 7;
    let session = w.open_seru_trade(vendor_id).expect("trade session opens");
    assert!(!session.is_empty(), "party owns seru -> offers exist");
    assert!(session.offers.len() <= DEFAULT_MAX_OFFERS as usize);
    for o in &session.offers {
        assert_ne!(o.receive_seru_id, o.give.seru_id);
        assert!((0x81..=0x95).contains(&o.receive_seru_id));
    }

    // Determinism: reopening the same vendor/time/party yields the same offers.
    let again = w.open_seru_trade(vendor_id).unwrap();
    assert_eq!(again.offers, session.offers, "offers are deterministic");

    // --- Confirm + apply the first offer; the runtime rewrites the owner. ---
    let offer = session.offers[0];
    let owner = offer.give.owner_slot as usize;
    let before = w.roster.members[owner].spell_list();
    assert!(
        before.ids[..before.count as usize].contains(&offer.give.seru_id),
        "owner really holds the seru being given"
    );

    let result = w.apply_seru_trade(&offer);
    assert_eq!(
        result,
        TradeResult::Swapped {
            owner_slot: offer.give.owner_slot,
            given: offer.give.seru_id,
            received: offer.receive_seru_id,
        }
    );

    let after = w.roster.members[owner].spell_list();
    assert!(
        after.ids[..after.count as usize].contains(&offer.receive_seru_id),
        "owner now holds the received seru"
    );
    // The given seru is gone (unless the owner held a second copy, which our
    // fixtures don't).
    assert!(
        !after.ids[..after.count as usize].contains(&offer.give.seru_id),
        "the traded-away seru is removed from the owner"
    );

    // --- Reseed: advancing past a two-hour boundary changes the offers. ---
    let bucket0 = w.open_seru_trade(vendor_id).unwrap().offers;
    let mut reseeded = false;
    for bucket in 1..16u32 {
        w.play_time_seconds = bucket * SECONDS_PER_RESEED;
        let later = w.open_seru_trade(vendor_id).unwrap();
        if later.offers != bucket0 {
            reseeded = true;
            break;
        }
    }
    assert!(
        reseeded,
        "vendor offers should reseed across a two-in-game-hour boundary"
    );
}
