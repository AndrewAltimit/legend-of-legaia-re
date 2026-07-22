//! Disc-gated end-to-end oracle for the Tactical-Arts button-combo randomizer
//! **at runtime** - the engine member of the randomizer oracle set (chest,
//! monster-drop, encounter, steal, shop, …).
//!
//! The randomizer's own disc-gated test (`crates/patcher/tests/arts_patch_real`)
//! proves a patched combo is *written* faithfully: each art's `+8`
//! command-glyph pointer in the static `SCUS_942.54` arts-name table is
//! reassigned, input counts + uniqueness + the Miracle Arts are preserved, and
//! the touched SCUS sector stays EDC/ECC-valid. What it does **not** prove is
//! that a runtime actually *recognises the new combo and fires the art* (and no
//! longer fires it on the old combo) - "is it truly randomizing, or is the old
//! combo still the trigger?".
//!
//! A savestate can't answer that cleanly (the same cache trap the other oracles
//! document): the arts table is static rodata resident in RAM the moment the
//! executable loads, so a state captured on a patched disc still matches the
//! *original* combo from the cached RAM copy. The patched combo is only the
//! trigger after a fresh executable load re-reads the table off the disc.
//!
//! The clean-room engine sidesteps that cache: it decodes the combo straight
//! from the patched `SCUS_942.54` bytes and runs the real combo-recognition
//! kernel - [`battle_arts::chain_matches_record`], the tail-match a directional
//! chain triggers an art with in retail. So this test, on a scratch copy of the
//! real disc:
//!   1. shuffles the arts combos and re-decodes the patched table off the
//!      patched image (the bytes a fresh executable load would stream),
//!   2. picks an art whose combo changed,
//!   3. asserts the engine matcher fires that art on the **new** combo, and
//!   4. asserts it no longer fires on the **old** combo.
//!
//! A baseline over the *unpatched* combo first confirms the matcher fires the
//! art at all, so the patched assertion can't pass vacuously. Skips without
//! `LEGAIA_DISC_BIN` (CLAUDE.md convention).

use legaia_art::queue::{ActionConstant, Command};
use legaia_art::record::{ArtRecord, EnemyEffect};
use legaia_engine_core::battle_arts::chain_matches_record;
use legaia_patcher::apply::{self, ArtSite};
use legaia_patcher::arts::ArtsMode;
use legaia_patcher::disc::DiscPatcher;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// A minimal art record carrying just the command string the matcher reads.
fn rec_with(commands: Vec<Command>) -> ArtRecord {
    ArtRecord {
        action: ActionConstant::Art1B,
        commands,
        anim_index: 0,
        anim_extra: vec![],
        name: None,
        power: vec![],
        dmg_timing: vec![],
        effect_cues: Default::default(),
        hit_cues: vec![],
        identifier: 0,
        anim_speed: 0,
        enemy_effect: EnemyEffect::default(),
        repeat_frames: Default::default(),
        background: 0,
        runtime_address: None,
    }
}

fn combo_bytes(a: &ArtSite) -> Vec<u8> {
    a.commands.iter().map(|c| c.as_byte()).collect()
}

#[test]
fn engine_matcher_fires_on_the_patched_combo_not_the_original() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };

    // Original combos.
    let base = DiscPatcher::open(original.clone()).expect("open");
    let before = apply::current_arts(&base).expect("read arts");

    // Baseline (non-vacuous): the engine matcher fires each art on its own
    // original combo.
    for a in before.iter().filter(|a| !a.is_miracle) {
        let rec = rec_with(a.commands.clone());
        assert!(
            chain_matches_record(&combo_bytes(a), &rec),
            "baseline: original combo must fire {:?} art {}",
            a.character,
            a.index
        );
    }

    // Shuffle the combos on a scratch copy and re-decode off the patched image.
    let mut patcher = DiscPatcher::open(original).expect("open");
    let (_plan, report) = apply::randomize_arts(&mut patcher, 0xA275_C0DE_1234, ArtsMode::Shuffle)
        .expect("randomize");
    assert!(report.combos_changed > 0);
    let after = apply::current_arts(&patcher).expect("read patched arts");

    // Find an art whose combo actually changed and drive the matcher.
    let mut checked = 0usize;
    for (b, a) in before.iter().zip(&after) {
        assert_eq!((b.character, b.index), (a.character, a.index));
        if b.is_miracle {
            continue;
        }
        let old = combo_bytes(b);
        let new = combo_bytes(a);
        if old == new {
            continue; // unchanged (singleton length class) - nothing to assert
        }
        // Same length (length-preserving randomizer) + different => neither is a
        // tail of the other.
        assert_eq!(old.len(), new.len(), "input count preserved");
        let rec = rec_with(a.commands.clone());
        // The NEW combo fires the art...
        assert!(
            chain_matches_record(&new, &rec),
            "patched combo must fire {:?} art {}",
            a.character,
            a.index
        );
        // ...and the OLD combo no longer does.
        assert!(
            !chain_matches_record(&old, &rec),
            "old combo must no longer fire {:?} art {} after randomization",
            a.character,
            a.index
        );
        checked += 1;
    }
    assert!(checked > 0, "at least one changed art must be exercised");
    eprintln!("arts runtime oracle: {checked} changed arts fire on the new combo, not the old");
}
