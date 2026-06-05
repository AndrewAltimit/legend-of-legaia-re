//! Disc-gated end-to-end test for the Tactical-Arts button-combo randomizer:
//! rewrite each art's directional **glyph bytes in place** in the static
//! `SCUS_942.54` arts table on a scratch copy of the disc (the bytes both the
//! menu display and the in-battle matcher read), then re-decode the patched
//! combos straight off the patched image and confirm the edit is faithful —
//! every art keeps its input count, each character's combos stay unique, the
//! Miracle Arts are untouched, a shuffle preserves the global per-length set of
//! distinct combos, the touched `SCUS_942.54` sectors stay EDC/ECC-valid, the
//! image size is unchanged, and a fixed seed is byte-deterministic. Skips +
//! passes without `LEGAIA_DISC_BIN`.

use legaia_iso::iso9660::find_file_in_image;
use legaia_iso::raw::{SECTOR_SIZE, USER_DATA_SIZE};
use legaia_rando::apply::{self, ArtSite};
use legaia_rando::arts::ArtsMode;
use legaia_rando::disc::DiscPatcher;
use std::collections::{BTreeMap, BTreeSet};

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

fn combo_bytes(a: &ArtSite) -> Vec<u8> {
    a.commands.iter().map(|c| c.as_byte()).collect()
}

/// Every regular art keyed by `(character, index)` -> combo bytes; plus the
/// Miracle rows kept separately so we can assert they never move.
fn read(patcher: &DiscPatcher) -> Vec<ArtSite> {
    apply::current_arts(patcher).expect("read arts table")
}

fn assert_invariants(before: &[ArtSite], after: &[ArtSite], mode: ArtsMode) {
    assert_eq!(before.len(), after.len(), "art count unchanged");

    // 1. Miracle arts (idx 0) are byte-for-byte untouched.
    for (b, a) in before.iter().zip(after) {
        assert_eq!((b.character, b.index), (a.character, a.index), "row order");
        if b.is_miracle {
            assert_eq!(
                combo_bytes(b),
                combo_bytes(a),
                "Miracle art must not change"
            );
        }
        // 2. Every art keeps its input count.
        assert_eq!(
            b.commands.len(),
            a.commands.len(),
            "{:?} art {} changed input count",
            b.character,
            b.index
        );
    }

    // Group regular arts per character.
    for ch in legaia_art::queue::Character::all() {
        let before_ch: Vec<&ArtSite> = before
            .iter()
            .filter(|a| a.character == ch && !a.is_miracle)
            .collect();
        let after_ch: Vec<&ArtSite> = after
            .iter()
            .filter(|a| a.character == ch && !a.is_miracle)
            .collect();
        assert_eq!(before_ch.len(), 14, "14 regular arts per character");

        // 3. Combos are unique within the character.
        let uniq: BTreeSet<Vec<u8>> = after_ch.iter().map(|a| combo_bytes(a)).collect();
        assert_eq!(
            uniq.len(),
            after_ch.len(),
            "{ch:?} combos must be unique within the character"
        );

        let _ = (&before_ch, &after_ch);
    }

    // The combo bytes are edited in place (not a pointer move), so the
    // matcher's copy changes too. Shuffle permutes the *distinct* combo
    // strings' contents within each length class, so the per-length SET of
    // distinct combos in use is preserved (a permutation; per-art multiplicity
    // can shift because some strings are shared across characters).
    if mode == ArtsMode::Shuffle {
        let per_len_set = |arts: &[ArtSite]| {
            let mut m: BTreeMap<usize, BTreeSet<Vec<u8>>> = BTreeMap::new();
            for a in arts.iter().filter(|a| !a.is_miracle) {
                m.entry(a.commands.len())
                    .or_default()
                    .insert(combo_bytes(a));
            }
            m
        };
        assert_eq!(
            per_len_set(before),
            per_len_set(after),
            "shuffle must preserve the global per-length set of distinct combos"
        );
    }

    // At least one combo actually changed somewhere.
    let changed = before
        .iter()
        .zip(after)
        .filter(|(b, a)| combo_bytes(b) != combo_bytes(a))
        .count();
    assert!(changed > 0, "the randomizer must change at least one combo");
}

fn run(mode: ArtsMode, seed: u64) {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };

    let base = DiscPatcher::open(original.clone()).expect("open");
    let before = read(&base);

    let mut patcher = DiscPatcher::open(original.clone()).expect("open");
    let (plan, report) = apply::randomize_arts(&mut patcher, seed, mode).expect("randomize");
    assert_eq!(report.arts, 42, "42 regular arts (3 Miracle excluded)");
    assert!(
        report.combos_changed > 0,
        "should change at least one combo"
    );

    // Re-decode the arts table off the PATCHED image.
    let after = read(&patcher);
    assert_invariants(&before, &after, mode);

    // The MATCHER guard (the bug this whole feature tripped over): every art's
    // display combo must also be present as a matcher art record in its
    // character's player-data file (`record0`), so the trigger matches the menu.
    // And at least one matcher record must actually differ from vanilla.
    for ch in legaia_art::queue::Character::all() {
        let index = legaia_rando::arts::player_entry_index(ch);
        let vanilla_entry = base.read_entry(index).expect("read vanilla player file");
        let patched_entry = patcher.read_entry(index).expect("read patched player file");
        let vanilla_rec0 = legaia_rando::arts::player_record0_decoded(&vanilla_entry)
            .expect("decode vanilla record0");
        let patched_rec0 = legaia_rando::arts::player_record0_decoded(&patched_entry)
            .expect("decode patched record0");
        assert_ne!(
            vanilla_rec0, patched_rec0,
            "{ch:?} player-file matcher records must change"
        );
        for a in after.iter().filter(|a| a.character == ch && !a.is_miracle) {
            assert!(
                legaia_rando::arts::record0_has_combo(&patched_rec0, &a.commands),
                "{ch:?} art {}: display combo not present as a matcher record (display/trigger desync)",
                a.index
            );
        }
    }

    // Image size unchanged (all edits are same-size glyph-byte writes).
    assert_eq!(
        patcher.image().len(),
        original.len(),
        "image size unchanged"
    );

    // The patched SCUS_942.54 sector holding a touched combo glyph stays valid.
    let img = patcher.image();
    let (scus_lba, _) = find_file_in_image(img, "SCUS_942.54").unwrap();
    let touched = plan
        .iter()
        .find(|e| e.new_directions != e.old_directions)
        .and_then(|e| e.direction_slots.first().copied())
        .expect("at least one glyph edited");
    let sb = (scus_lba as usize + touched / USER_DATA_SIZE) * SECTOR_SIZE;
    assert!(
        legaia_iso::write::mode2_form1_sector_is_valid(&img[sb..sb + SECTOR_SIZE]),
        "patched arts-table sector must be EDC/ECC-valid"
    );

    // Determinism: same seed -> byte-identical patched image.
    let mut patcher2 = DiscPatcher::open(original).expect("open");
    let (_p2, report2) = apply::randomize_arts(&mut patcher2, seed, mode).expect("randomize");
    assert_eq!(report2.combos_changed, report.combos_changed);
    assert!(
        patcher2.image() == patcher.image(),
        "same seed must reproduce the patched image"
    );

    eprintln!(
        "arts {mode:?} seed {seed:#x}: {} of {} regular arts re-combo'd; lengths + uniqueness + Miracles preserved",
        report.combos_changed,
        plan.len()
    );
}

#[test]
fn shuffle_arts_round_trips_on_disc() {
    run(ArtsMode::Shuffle, 0x5EA1_F00D_0A27_5C0B);
}

#[test]
fn random_arts_round_trips_on_disc() {
    run(ArtsMode::Random, 0x0A27_C0FF_EE12_3456);
}
