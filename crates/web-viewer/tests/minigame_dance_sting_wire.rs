//! Disc-gated: the browser dance page's good-step **hit sting** resolves the
//! `(program, tone, note)` triple through the ported `FUN_801d3d78` kernel
//! (`legaia_engine_core::dance::dance_hit_sting_voices`) rather than
//! recomputing it.
//!
//! Two things the wire buys, both checked here against the visitor's own disc:
//!
//! 1. The bank index is the retail `program` argument. `tones` is program-
//!    slot-indexed, so a page that reaches `tones[1]` by hand is only right by
//!    coincidence; asking the kernel makes it right by construction.
//! 2. The `r` space is the retail one. `FUN_801d1af4` reaches the sting from
//!    four sites - the tier-2 award's `rand() % 3` and, from each of the three
//!    groovy-move tiers, the literal `STING_TIER_VARIANT`. The page used to
//!    reject anything above `2`, which dropped the tier sting entirely.
//!
//! Skips and passes without `LEGAIA_DISC_BIN`.

use std::path::PathBuf;

use legaia_engine_core::dance::{
    STING_PROGRAM, STING_RANDOM_VARIANTS, STING_TIER_VARIANT, dance_hit_sting_voices,
};
use legaia_web_viewer::minigames::LegaiaMinigames;

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

fn loaded() -> Option<LegaiaMinigames> {
    let prot = prot_dat()?;
    let bytes = std::fs::read(&prot).expect("read PROT.DAT");
    let mut mg = LegaiaMinigames::new();
    mg.load_disc(bytes).expect("load_disc");
    Some(mg)
}

/// Every `r` retail keys - the three random picks and the tier variant -
/// decodes both of its layers off the disc bank.
#[test]
fn the_page_decodes_every_sting_retail_keys() {
    let Some(mg) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN or extracted/PROT.DAT missing");
        return;
    };

    let retail_r: Vec<u16> = (0..STING_RANDOM_VARIANTS)
        .chain(std::iter::once(STING_TIER_VARIANT))
        .collect();
    assert!(
        retail_r.contains(&STING_TIER_VARIANT),
        "the tier sting is outside the random space, so it has to be listed"
    );

    for &r in &retail_r {
        for layer in 0u8..2 {
            let pcm = mg.dance_sting_pcm(r as u8, layer);
            let rate = mg.dance_sting_rate(r as u8, layer);
            assert!(!pcm.is_empty(), "sting r={r} layer={layer} decoded empty");
            assert!(rate > 0, "sting r={r} layer={layer} has no rate");
            // The SPU pitch fold is bounded the same way every other decode on
            // this page is.
            assert!(
                (4000..=96_000).contains(&rate),
                "sting r={r} layer={layer} rate {rate} out of the clamp"
            );
        }
    }
}

/// The kernel is what names the tone, and the two layers are distinct tones in
/// the same program keyed at one note - which on this bank mostly means the
/// *same* VAG at two centre notes, i.e. a detuned pair. So the discriminator is
/// the playback rate, not the sample.
#[test]
fn the_kernel_triple_is_what_selects_the_sample() {
    let Some(mg) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN or extracted/PROT.DAT missing");
        return;
    };

    let retail_r: Vec<u16> = (0..STING_RANDOM_VARIANTS)
        .chain(std::iter::once(STING_TIER_VARIANT))
        .collect();

    for &r in &retail_r {
        let voices = dance_hit_sting_voices(r);
        assert_eq!(voices[0].program, STING_PROGRAM);
        assert_eq!(voices[1].program, STING_PROGRAM);
        assert_eq!(voices[1].tone, voices[0].tone + 1, "2r / 2r+1");
        assert_eq!(voices[0].note, voices[1].note, "one note, two tones");

        // Two tones, one note: the pair separates by centre note, so the two
        // layers land at different rates even where they share a sample.
        assert_ne!(
            mg.dance_sting_rate(r as u8, 0),
            mg.dance_sting_rate(r as u8, 1),
            "the two voices of r={r} are keyed at one pitch"
        );
    }

    // Distinct `r` means a distinct sting, so the random space is three
    // audibly different ones and not one sample re-keyed.
    let picks: Vec<Vec<i16>> = (0..STING_RANDOM_VARIANTS)
        .map(|r| mg.dance_sting_pcm(r as u8, 0))
        .collect();
    for i in 1..picks.len() {
        assert_ne!(picks[0], picks[i], "sting 0 and sting {i} are the same PCM");
    }

    // The note rises one semitone per variant, so the tier sting sits a fourth
    // above the random ones - that offset is the whole reason it reads as a
    // reward and not a repeat.
    assert_eq!(
        dance_hit_sting_voices(STING_TIER_VARIANT)[0].note - dance_hit_sting_voices(0)[0].note,
        STING_TIER_VARIANT as i16
    );
    assert_ne!(
        mg.dance_sting_pcm(STING_TIER_VARIANT as u8, 0),
        picks[0],
        "the tier sting is its own sample, not random pick 0"
    );
}

/// Out-of-bank indices stay `None` rather than panicking or aliasing another
/// tone: the bound is now the bank's, not a hand-written `r > 2`.
#[test]
fn an_index_the_bank_does_not_hold_decodes_empty() {
    let Some(mg) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN or extracted/PROT.DAT missing");
        return;
    };
    assert!(
        mg.dance_sting_pcm(0, 2).is_empty(),
        "layer 2 does not exist"
    );
    assert_eq!(mg.dance_sting_rate(0, 2), 0);
    assert!(
        mg.dance_sting_pcm(200, 0).is_empty(),
        "tone 400 is not a tone"
    );
    assert_eq!(mg.dance_sting_rate(200, 0), 0);
}
