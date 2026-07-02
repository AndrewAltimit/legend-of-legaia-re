//! Disc-gated reproducibility for the Baka Fighter per-opponent table.
//!
//! Re-extract the Baka Fighter overlay (PROT 0976) from the user's `PROT.DAT`,
//! decode the opponent table, and assert the structural invariants that pin it
//! (no Sony bytes asserted - the gold values + AI patterns stay on the disc):
//!
//! * exactly [`OPPONENT_COUNT`] records, each with a valid non-empty `1/2/3` AI
//!   move pattern;
//! * the table is bounded: opponent 17 (one past the end) does NOT decode to a
//!   valid pattern;
//! * the gold rewards are sane (the ladder opponents pay a positive prize).
//!
//! Skips + passes when `LEGAIA_DISC_BIN` / `extracted/PROT.DAT` are absent.

use std::path::PathBuf;

use legaia_asset::baka_opponents::{self as baka, OPPONENT_COUNT};
use legaia_asset::static_overlay;
use legaia_prot::archive::Archive;

fn prot_dat() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for p in ["extracted/PROT.DAT", "../../extracted/PROT.DAT"] {
        let f = PathBuf::from(p);
        if f.is_file() {
            return Some(f);
        }
    }
    None
}

fn baka_overlay() -> Option<Vec<u8>> {
    let prot = prot_dat()?;
    let mut archive = Archive::open(&prot).expect("open PROT.DAT");
    let rec = static_overlay::overlay_map()
        .by_prot_index(baka::BAKA_OVERLAY_PROT_INDEX as u32)
        .expect("baka overlay in static map");
    let entry = archive
        .entries
        .iter()
        .find(|e| e.index == rec.prot_index)
        .cloned()
        .expect("PROT entry present");
    let mut raw = Vec::new();
    archive.read_entry(&entry, &mut raw).expect("read entry");
    Some(static_overlay::as_loaded(&raw, rec).expect("as-loaded form"))
}

#[test]
fn opponent_table_reproduces_and_is_bounded() {
    let Some(overlay) = baka_overlay() else {
        eprintln!("[skip] LEGAIA_DISC_BIN or extracted/PROT.DAT missing");
        return;
    };

    let opponents = baka::parse(&overlay).expect("opponent table parses");
    assert_eq!(opponents.len(), OPPONENT_COUNT);

    let mut paying = 0usize;
    for o in &opponents {
        assert!(
            baka::is_valid_pattern(&o.ai_pattern),
            "opponent {} has a valid 1/2/3 AI pattern",
            o.index
        );
        // attack_at maps each pattern symbol into the 0..=2 attack-type space.
        for c in 0..o.ai_pattern.len() {
            assert!(o.attack_at(c).unwrap() <= 2);
        }
        // sane gold range (no garbage u32) - the ladder opponents pay.
        assert!(o.gold_reward < 1_000_000, "opponent {} gold sane", o.index);
        if o.gold_reward > 0 {
            paying += 1;
        }
    }
    assert!(paying >= 10, "most opponents pay a gold prize");

    // Bound: opponent 17 (one past the table) is not a valid record.
    let over = baka::parse_at(
        &overlay,
        baka::OPPONENT_TABLE_FILE_OFFSET,
        OPPONENT_COUNT + 1,
    )
    .expect("over-read parses");
    assert!(
        !baka::is_valid_pattern(&over[OPPONENT_COUNT].ai_pattern),
        "opponent {OPPONENT_COUNT} is past the table (no valid pattern)"
    );
}

#[test]
fn stat_fields_and_action_tables_decode_sane() {
    let Some(overlay) = baka_overlay() else {
        eprintln!("[skip] LEGAIA_DISC_BIN or extracted/PROT.DAT missing");
        return;
    };

    let opponents = baka::parse(&overlay).expect("opponent table parses");
    let mut nonzero_stats = 0usize;
    for o in &opponents {
        // Percent-scale stat fields: loose sanity bounds, no Sony values
        // pinned. Tiers can be negative (an HP-band stat penalty).
        assert!(
            (0..=1000).contains(&o.damage_mod),
            "fighter {} damage_mod in range ({})",
            o.index,
            o.damage_mod
        );
        assert!(
            (0..=100).contains(&o.crit_chance),
            "fighter {} crit chance is a percent ({})",
            o.index,
            o.crit_chance
        );
        for t in 0..3 {
            assert!(
                (-100..=200).contains(&o.atk_tiers[t]),
                "fighter {} atk tier {t} sane ({})",
                o.index,
                o.atk_tiers[t]
            );
            assert!(
                (-100..=200).contains(&o.def_tiers[t]),
                "fighter {} def tier {t} sane ({})",
                o.index,
                o.def_tiers[t]
            );
        }
        if o.atk_tiers.iter().any(|&v| v != 0) || o.def_tiers.iter().any(|&v| v != 0) {
            nonzero_stats += 1;
        }
    }
    assert!(
        nonzero_stats >= 10,
        "most fighters carry HP-keyed stat tiers"
    );

    // Action tables through PTR_DAT_801db8b8: 17 sets; the three attack
    // records carry positive base power, and the special record's payoff is
    // its keyframe count (its power is the zero the damage kernel reads).
    let actions = baka::parse_actions(&overlay).expect("action tables parse");
    assert_eq!(actions.len(), OPPONENT_COUNT);
    let mut powered = 0usize;
    let mut charged = 0usize;
    for a in &actions {
        for t in 1..=4u8 {
            let p = a.attack_power(t).unwrap();
            assert!(
                (-10_000..=10_000).contains(&p),
                "fighter {} attack {t} power sane ({p})",
                a.index
            );
        }
        if (1..=3u8).all(|t| a.attack_power(t).unwrap() > 0) {
            powered += 1;
        }
        // The special's keyframe count gates the full-hit round win.
        let kf = a.keyframes[baka::ACTION_SPECIAL];
        assert!(
            (0..=256).contains(&kf),
            "fighter {} special keyframes ({kf})",
            a.index
        );
        if kf >= 1 {
            charged += 1;
        }
    }
    assert!(powered >= 10, "most fighters have positive attack powers");
    assert!(charged >= 10, "most fighters have a charged special");
}
