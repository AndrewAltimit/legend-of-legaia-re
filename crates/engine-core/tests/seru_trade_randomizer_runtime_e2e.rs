//! Disc-gated runtime oracle for the **seru-trade** feature - the engine-side
//! counterpart to the patcher crate's `seru_trade_real` config round-trip.
//!
//! The patcher test proves the config blob (enabled flag + master seed) is
//! *written* to the disc faithfully. What it can't prove is that a runtime
//! *reads it and lets the player swap a seru*. This test closes that: it patches
//! the seru-trade config onto a scratch copy of the real disc (the surgical
//! `--seru-trade` edit), re-decodes the config straight from the patched SCUS
//! bytes, installs it into a clean-room [`World`] holding a party with known
//! seru, opens a trade session, confirms a trade, and asserts the runtime
//! rewrites the owner's spell list to the offered seru - and that the offers
//! reseed across a two-in-game-hour boundary.
//!
//! A baseline pass over the *unpatched* SCUS first confirms the engine reports
//! trading disabled, so the patched assertions can't pass vacuously.
//!
//! Skips without `LEGAIA_DISC_BIN` (CLAUDE.md convention).

use legaia_asset::seru_trade::{self, DEFAULT_MAX_OFFERS, SECONDS_PER_RESEED};
use legaia_engine_core::seru_trade::TradeResult;
use legaia_engine_core::world::World;
use legaia_patcher::apply;
use legaia_patcher::disc::DiscPatcher;
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

/// The vendor phase offset the test opens its trade session at.
const VENDOR_OFFSET: u8 = 7;

/// A party built so its lead owns exactly the seru the test vendor's phased
/// schedule slot wants at play-time 0 (the bucket model trades a *type* the
/// party holds, so the fixture has to hold it), plus unrelated seru on the
/// other members.
fn party_for(seed: u64) -> Party {
    let offer = seru_trade::bucket_offer(seed, VENDOR_OFFSET as u32, &seru_trade::default_pool());
    let other = if offer.want_id == 0x90 { 0x91 } else { 0x90 };
    Party {
        members: vec![
            ch_with_spells(&[offer.want_id]),
            ch_with_spells(&[other]),
            ch_with_spells(&[0x05]), // not a tradeable seru
        ],
    }
}

#[test]
fn seru_trade_runtime_swaps_and_reseeds() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };

    let seed = 0xBADC0DEu64;

    // --- Baseline: unpatched disc reports trading disabled. ---
    let base = DiscPatcher::open(disc.clone()).expect("open disc");
    let vanilla_scus = base
        .read_named_file("SCUS_942.54")
        .expect("SCUS present on disc");
    let mut w0 = World {
        roster: party_for(seed),
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
    let mut patcher = DiscPatcher::open(disc).expect("open disc");
    apply::enable_seru_trades(&mut patcher, seed, DEFAULT_MAX_OFFERS).expect("enable seru trade");
    let patched_scus = patcher
        .read_named_file("SCUS_942.54")
        .expect("SCUS present after patch");

    let mut w = World {
        roster: party_for(seed),
        play_time_seconds: 0,
        ..World::default()
    };
    assert!(
        w.install_seru_trade_config(&patched_scus),
        "patched disc enables seru trading"
    );
    assert!(w.seru_trade_enabled());

    // Open at a vendor (phase offset 7). The session carries that vendor's
    // standing offer (the basis of the "No X available to trade for Y" empty
    // message) and, since the fixture lead owns the wanted seru, one
    // selectable line.
    let vendor_id = VENDOR_OFFSET;
    let session = w.open_seru_trade(vendor_id).expect("trade session opens");
    let expected_offer =
        seru_trade::bucket_offer(seed, VENDOR_OFFSET as u32, &seru_trade::default_pool());
    assert_eq!(
        session.offer, expected_offer,
        "session offer matches the kernel's phased slot for the disc seed"
    );
    assert!(!session.is_empty(), "lead owns the want -> a line exists");
    for o in &session.offers {
        assert_eq!(o.given_id, expected_offer.want_id);
        assert_eq!(o.received_id, expected_offer.give_id);
        assert_ne!(o.received_id, o.given_id);
        assert!((0x81..=0x95).contains(&o.received_id));
        assert_eq!(o.received_level, expected_offer.give_level);
        assert!((1..=9).contains(&o.received_level), "curved level range");
    }

    // Determinism: reopening the same vendor/time/party yields the same offers.
    let again = w.open_seru_trade(vendor_id).unwrap();
    assert_eq!(again.offers, session.offers, "offers are deterministic");

    // --- Confirm + apply the first line; the runtime rewrites the owner. ---
    let trade = session.offers[0];
    let owner = trade.owner_slot as usize;
    let before = w.roster.members[owner].spell_list();
    assert!(
        before.ids[..before.count as usize].contains(&trade.given_id),
        "owner really holds the seru being given"
    );

    let result = w.apply_seru_trade(&trade);
    assert_eq!(
        result,
        TradeResult::Swapped {
            owner_slot: trade.owner_slot,
            given: trade.given_id,
            received: trade.received_id,
        }
    );

    let after = w.roster.members[owner].spell_list();
    let pos = after.ids[..after.count as usize]
        .iter()
        .position(|&id| id == trade.received_id)
        .expect("owner now holds the received seru");
    assert_eq!(
        after.levels[pos], trade.received_level,
        "received seru arrives at the offered level (retail parity)"
    );
    // The given seru is gone (unless the owner held a second copy, which our
    // fixtures don't).
    assert!(
        !after.ids[..after.count as usize].contains(&trade.given_id),
        "the traded-away seru is removed from the owner"
    );

    // After the swap the owner holds the give-back, so the line list empties -
    // but the standing offer stays visible for the empty-state message.
    let emptied = w.open_seru_trade(vendor_id).unwrap();
    assert!(emptied.is_empty(), "traded owner no longer qualifies");
    assert_eq!(
        emptied.offer, expected_offer,
        "offer still named while empty"
    );

    // --- Reseed: advancing past a bucket boundary changes the standing offer. ---
    let mut reseeded = false;
    for bucket in 1..16u32 {
        w.play_time_seconds = bucket * SECONDS_PER_RESEED;
        let later = w.open_seru_trade(vendor_id).unwrap();
        if later.offer != expected_offer {
            reseeded = true;
            break;
        }
    }
    assert!(
        reseeded,
        "the vendor's standing offer should reseed across a bucket boundary"
    );
}
