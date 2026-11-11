//! Cross-validate the curated `arts.toml` against the executable.
//!
//! The SCUS arts-name table (`DAT_80075EC4`, decoded by
//! `legaia_art::arts_table`) is the **ground truth** for each Tactical Art's
//! name, AP cost and directional command sequence; the resident battle-overlay
//! Super / Miracle Art trigger tables (`legaia_art::SUPER_ARTS` /
//! `MIRACLE_ARTS`, capture-validated byte-exact) are the ground truth for the
//! combination arts. This test pins every curated row against them:
//!
//! - **Regular / Hyper / Miracle arts** join the SCUS row at
//!   `index = action_constant - 0x1B` of the same character (not by name, so a
//!   curated disambiguating suffix like `"Tornado Flame (Hyper)"` cannot make a
//!   row silently skip). AP, directions and name must agree, and every SCUS row
//!   must have a curated row.
//! - **Super Arts** join the trigger table by finisher constant. The curated
//!   directions must be the physical input the retail tokenizer turns into the
//!   trigger's `find` pattern ([`legaia_art::derive_super_input`]), and the AP
//!   must be the sum of the chained arts' SCUS AP (a Super consumes no AP of its
//!   own - its chain pays).
//! - **Miracle Arts** must also carry the trigger table's command string.
//!
//! A divergence is a walkthrough error to correct in `arts.toml`, not to
//! allow-list here.
//!
//! Skips and passes when `extracted/SCUS_942.54` isn't on disk - same gating
//! pattern as the other disc-dependent tests, so CI needs no Sony bytes.

use legaia_art::{ActionConstant, ArtsOracle, Command, MIRACLE_ARTS, SUPER_ARTS};
use legaia_gamedata::{ArtKind, Character, Database};
use std::path::PathBuf;

fn scus_path() -> Option<PathBuf> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest.parent()?.parent()?;
    let p = workspace.join("extracted").join("SCUS_942.54");
    p.is_file().then_some(p)
}

/// Action constant of the art at SCUS arts-name index 0.
const ART_CONSTANT_BIAS: u8 = 0x1B;
/// The Miracle Art and the three Hyper Art slots are walked by the special
/// arm of the queue builder; the Super Art tokenizer catalog is the rest.
const FIRST_NORMAL_ORDINAL: usize = 4;

fn to_art(c: Character) -> legaia_art::Character {
    match c {
        Character::Vahn => legaia_art::Character::Vahn,
        Character::Noa => legaia_art::Character::Noa,
        Character::Gala => legaia_art::Character::Gala,
    }
}

fn dirs(cmds: &[Command]) -> Vec<u8> {
    cmds.iter().map(|c| c.as_byte()).collect()
}

/// The SCUS display name of a Miracle Art carries the character's name as a
/// runtime substitution (`"'s Ark"`, `""`); a Hyper row may be curated with a
/// `" (Hyper)"` suffix that disambiguates it from the Miracle-chain constant
/// of the same name.
fn names_agree(curated: &str, scus: &str, kind: ArtKind) -> bool {
    let curated = curated.trim_end_matches(" (Hyper)");
    if curated.eq_ignore_ascii_case(scus) {
        return true;
    }
    kind == ArtKind::Miracle && (scus.is_empty() || curated.ends_with(scus))
}

#[test]
fn arts_toml_matches_scus_arts_table_or_skips() {
    let Some(path) = scus_path() else {
        eprintln!("extracted/SCUS_942.54 not present - skipping");
        return;
    };
    let bytes = std::fs::read(&path).expect("read SCUS");
    let oracle = ArtsOracle::from_scus(&bytes).expect("build arts oracle");
    let gd = Database::load();

    let mut joined = std::collections::BTreeSet::new();
    for art in gd.arts().iter().filter(|a| a.kind != ArtKind::Super) {
        let ac = art
            .action_constant
            .unwrap_or_else(|| panic!("{} has no action constant", art.name));
        let index = ac - ART_CONSTANT_BIAS;
        let entry = oracle
            .by_character_index(to_art(art.character), index)
            .unwrap_or_else(|| {
                panic!(
                    "{:?} {} (action {ac:#04x}) has no SCUS arts-name row {index}",
                    art.character, art.name
                )
            });
        assert!(
            names_agree(&art.name, &entry.name, art.kind),
            "{:?} action {ac:#04x}: curated name {:?} vs SCUS {:?}",
            art.character,
            art.name,
            entry.name
        );
        assert_eq!(
            u32::from(entry.ap),
            art.ap,
            "AP mismatch for {}: SCUS {} vs gamedata {}",
            art.name,
            entry.ap,
            art.ap
        );
        assert_eq!(
            art.directions,
            dirs(&entry.commands),
            "directions mismatch for {:?} {}",
            art.character,
            art.name
        );
        assert_eq!(
            entry.is_miracle,
            art.kind == ArtKind::Miracle,
            "{} kind {:?} vs SCUS miracle flag {}",
            art.name,
            art.kind,
            entry.is_miracle
        );
        assert!(
            joined.insert((art.character as u8, index)),
            "two curated arts claim {:?} row {index}",
            art.character
        );
    }

    // Completeness: every SCUS row has its curated art.
    for (c, ch) in [
        (Character::Vahn, legaia_art::Character::Vahn),
        (Character::Noa, legaia_art::Character::Noa),
        (Character::Gala, legaia_art::Character::Gala),
    ] {
        for e in oracle.entries().iter().filter(|e| e.character == ch) {
            assert!(
                joined.contains(&(c as u8, e.index)),
                "SCUS {:?} art row {} ({:?}) has no curated arts.toml row",
                c,
                e.index,
                e.name
            );
        }
    }
    assert!(joined.len() >= 40, "only joined {} arts", joined.len());
}

#[test]
fn super_and_miracle_arts_match_trigger_tables_or_skips() {
    let Some(path) = scus_path() else {
        eprintln!("extracted/SCUS_942.54 not present - skipping");
        return;
    };
    let bytes = std::fs::read(&path).expect("read SCUS");
    let records = legaia_art::raw_records_from_scus(&bytes).expect("arts-name records");
    let gd = Database::load();

    for c in [Character::Vahn, Character::Noa, Character::Gala] {
        let ch = to_art(c);
        let mut mine: Vec<_> = records.iter().filter(|r| r.character == ch).collect();
        mine.sort_by_key(|r| r.index);
        let catalog: Vec<(ActionConstant, &[Command])> = mine
            .iter()
            .enumerate()
            .filter(|(ordinal, _)| *ordinal >= FIRST_NORMAL_ORDINAL)
            .filter_map(|(_, r)| {
                let a = ActionConstant::from_byte(r.index + ART_CONSTANT_BIAS)?;
                Some((a, r.commands.as_slice()))
            })
            .collect();
        let ap_of = |constant: u8| -> u32 {
            let r = mine
                .iter()
                .find(|r| r.index + ART_CONSTANT_BIAS == constant)
                .unwrap_or_else(|| panic!("{c:?} chain art {constant:#04x} not in SCUS"));
            u32::from(r.ap)
        };

        let curated: Vec<_> = gd
            .arts_for(c)
            .filter(|a| a.kind == ArtKind::Super)
            .collect();
        let triggers: Vec<_> = SUPER_ARTS.iter().filter(|s| s.character == ch).collect();
        assert_eq!(curated.len(), triggers.len(), "{c:?} Super Art count");
        for s in triggers {
            let art = curated
                .iter()
                .find(|a| a.action_constant == Some(s.finisher))
                .unwrap_or_else(|| panic!("no curated Super with finisher {:#04x}", s.finisher));
            assert_eq!(art.name, s.name, "{c:?} Super {:#04x} name", s.finisher);
            let input = legaia_art::derive_super_input(&catalog, s.find)
                .unwrap_or_else(|| panic!("{} derives no unique input", s.name));
            assert_eq!(
                art.directions,
                dirs(&input),
                "{c:?} {}: curated input vs the input the retail tokenizer needs",
                art.name
            );
            let ap: u32 = s.art_sequence().into_iter().map(ap_of).sum();
            assert_eq!(art.ap, ap, "{c:?} {}: AP vs its chain's SCUS AP", art.name);
        }

        let m = MIRACLE_ARTS
            .iter()
            .find(|m| m.character == ch)
            .expect("miracle trigger");
        let art = gd
            .arts_for(c)
            .find(|a| a.kind == ArtKind::Miracle)
            .expect("curated miracle");
        assert_eq!(art.name, m.name, "{c:?} Miracle Art name");
        assert_eq!(art.directions, dirs(m.commands), "{} directions", art.name);
    }
}
