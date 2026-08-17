//! Disc-gated oracle for the Super Art damage-power editor
//! (`legaia_patcher::super_art_power` + `apply::set_super_art_power`).
//!
//! The load-bearing claim under test is the **addressing**: a Super Art has no
//! input combo and no row in the SCUS arts-name table, so it is reached through
//! its finisher action constant in the character's own `record0` art block
//! (`record_off = base + (finisher - GRID_BIAS) * 0xD0`). The oracle pins that
//! all fifteen resolve on the real disc, that each landed record's `+0x10` name
//! is the Super Art's own name, and that an edit is surgical, EDC/ECC-clean,
//! idempotent and byte-deterministic. Skips + passes without `LEGAIA_DISC_BIN`.

use legaia_art::queue::{Character, Command};
use legaia_iso::raw::{SECTOR_SIZE, USER_DATA_SIZE};
use legaia_patcher::apply;
use legaia_patcher::arts_power::labeled_art_powers;
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::super_art_power::{self, find_super_art, player_entry_index, super_art_powers};

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

fn scus(patcher: &DiscPatcher) -> Vec<u8> {
    legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54").expect("SCUS")
}

/// Every located Super Art of `ch`, keyed by finisher.
fn powers(
    patcher: &DiscPatcher,
    ch: Character,
) -> std::collections::BTreeMap<u8, super_art_power::SuperArtPower> {
    let entry = patcher.read_entry(player_entry_index(ch)).unwrap();
    super_art_powers(&scus(patcher), &entry, ch)
        .unwrap_or_default()
        .into_iter()
        .map(|r| (r.finisher, r))
        .collect()
}

/// One named Super Art's table row.
fn art(name: &str) -> &'static legaia_art::SuperArt {
    let hits = find_super_art(name, None);
    assert_eq!(hits.len(), 1, "{name} resolves uniquely");
    hits[0]
}

#[test]
fn all_fifteen_super_arts_resolve_and_carry_their_own_name() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(disc).expect("open");

    let mut total = 0usize;
    for ch in Character::all() {
        let expected = super_art_power::super_arts_for(ch);
        if expected.is_empty() {
            continue; // Terra has no Tactical Arts
        }
        let found = powers(&patcher, ch);
        assert_eq!(
            found.len(),
            expected.len(),
            "{ch:?}: every Super Art must resolve in record0"
        );
        for s in expected {
            let row = found
                .get(&s.finisher)
                .unwrap_or_else(|| panic!("{ch:?} {} missing", s.name));
            assert_eq!(row.name, s.name);
            // The address is derived from the finisher, and the record's own
            // name field is what validates it - so a match here is the pin.
            assert_eq!(
                row.record_off % legaia_patcher::arts_power::ART_RECORD_STRIDE,
                found.values().next().unwrap().record_off
                    % legaia_patcher::arts_power::ART_RECORD_STRIDE,
                "{} sits on the shared 0xD0 grid",
                s.name
            );
            total += 1;
        }
    }
    assert_eq!(total, 15, "5 Super Arts each for Vahn / Noa / Gala");
}

#[test]
fn baseline_power_bytes_are_the_retail_values() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(disc).expect("open");

    // Single-hit finishers, one per character, plus a four-hit and a three-hit
    // one - so the hit-count spread is pinned as well as the values.
    let vahn = powers(&patcher, Character::Vahn);
    assert_eq!(vahn[&0x2B].name, "Tri-Somersault");
    assert_eq!(vahn[&0x2B].power, vec![0x16]);
    assert_eq!(vahn[&0x2C].power, vec![0x1F]); // Maximum Blow

    let noa = powers(&patcher, Character::Noa);
    assert_eq!(noa[&0x30].name, "Super Tempest");
    assert_eq!(noa[&0x30].power, vec![0x1B, 0x1B, 0x16, 0x16]);

    let gala = powers(&patcher, Character::Gala);
    assert_eq!(gala[&0x2E].name, "Heaven's Drop");
    assert_eq!(gala[&0x2E].power, vec![0x16, 0x16, 0x16, 0x16]);
    assert_eq!(gala[&0x2F].name, "Neo Static Raising");
    assert_eq!(gala[&0x2F].power, vec![0x1B, 0x1B, 0x16]);

    // Non-vacuous baseline: the values the edit test writes are NOT what the
    // clean disc already holds, so a green edit assertion cannot be a no-op.
    assert_ne!(vahn[&0x2B].power, vec![0x1A]);
    assert_ne!(gala[&0x2F].power, vec![0x0C, 0x0C, 0x0C]);
}

#[test]
fn edit_is_surgical_edc_valid_and_reparses() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(disc).expect("open");
    let before_supers = powers(&patcher, Character::Gala);
    let scus_img = scus(&patcher);
    let index = player_entry_index(Character::Gala);
    let before_regular = labeled_art_powers(
        &scus_img,
        &patcher.read_entry(index).unwrap(),
        Character::Gala,
    )
    .unwrap();

    let report = apply::set_super_art_power(&mut patcher, &[(art("Neo Static Raising"), 0x0C)])
        .expect("apply");
    assert_eq!(report.edits.len(), 1, "exactly one record edited");
    assert_eq!(report.edits[0].character, Character::Gala);
    assert_eq!(report.edits[0].name, "Neo Static Raising");
    assert_eq!(report.edits[0].old_power, vec![0x1B, 0x1B, 0x16]);
    // Hit count preserved: a three-hit finisher stays three hits.
    assert_eq!(report.edits[0].new_power, vec![0x0C, 0x0C, 0x0C]);

    // Re-decode off the patched image: the target moved, every sibling Super
    // Art and every regular art kept its bytes.
    let after_supers = powers(&patcher, Character::Gala);
    assert_eq!(before_supers.len(), after_supers.len());
    for (fin, before) in &before_supers {
        let after = &after_supers[fin];
        assert_eq!(before.name, after.name);
        if *fin == 0x2F {
            assert_eq!(after.power, vec![0x0C, 0x0C, 0x0C]);
        } else {
            assert_eq!(before.power, after.power, "{} unchanged", after.name);
        }
    }
    let after_regular = labeled_art_powers(
        &scus_img,
        &patcher.read_entry(index).unwrap(),
        Character::Gala,
    )
    .unwrap();
    assert_eq!(before_regular.len(), after_regular.len());
    for (b, a) in before_regular.iter().zip(after_regular.iter()) {
        assert_eq!(b.combo, a.combo);
        assert_eq!(
            b.power,
            a.power,
            "regular art {:?} must not move when a Super Art is edited",
            a.combo_str()
        );
    }
    // The other characters' files were never opened for writing.
    let vahn = powers(&patcher, Character::Vahn);
    assert_eq!(vahn[&0x2B].power, vec![0x16]);

    // Image size unchanged; every touched player-file sector stays EDC/ECC-valid.
    assert_eq!(patcher.image().len() % SECTOR_SIZE, 0);
    let lba = patcher.entry_disc_lba(index).unwrap() as usize;
    let footprint = patcher.entry_footprint(index).unwrap() as usize;
    let sectors = footprint.div_ceil(USER_DATA_SIZE);
    let img = patcher.image();
    for s in 0..sectors {
        let sb = (lba + s) * SECTOR_SIZE;
        assert!(
            legaia_iso::write::mode2_form1_sector_is_valid(&img[sb..sb + SECTOR_SIZE]),
            "player-file sector {s} must stay EDC/ECC-valid after the Super Art power edit"
        );
    }
}

#[test]
fn a_batch_spanning_characters_touches_exactly_its_targets() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(disc).expect("open");
    let report = apply::set_super_art_power(
        &mut patcher,
        &[
            (art("Tri-Somersault"), 0x1A),
            (art("Dragon Fangs"), 0x1F),
            (art("Back Punch x3"), 0x0C),
        ],
    )
    .expect("apply");
    assert_eq!(report.edits.len(), 3);
    assert_eq!(powers(&patcher, Character::Vahn)[&0x2B].power, vec![0x1A]);
    assert_eq!(
        powers(&patcher, Character::Noa)[&0x32].power,
        vec![0x1F, 0x1F, 0x1F, 0x1F]
    );
    assert_eq!(powers(&patcher, Character::Gala)[&0x2B].power, vec![0x0C]);
    // Siblings in the same files stayed put.
    assert_eq!(powers(&patcher, Character::Vahn)[&0x2C].power, vec![0x1F]);
    assert_eq!(
        powers(&patcher, Character::Gala)[&0x2F].power,
        vec![0x1B, 0x1B, 0x16]
    );
}

#[test]
fn reapply_is_a_noop() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(disc).expect("open");
    let r1 =
        apply::set_super_art_power(&mut patcher, &[(art("Fire Tackle"), 0x10)]).expect("apply");
    assert_eq!(r1.edits.len(), 1);
    let r2 =
        apply::set_super_art_power(&mut patcher, &[(art("Fire Tackle"), 0x10)]).expect("apply");
    assert!(r2.edits.is_empty(), "re-applying the same power is a no-op");
}

#[test]
fn edit_is_byte_deterministic() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut a = DiscPatcher::open(disc.clone()).expect("open");
    let mut b = DiscPatcher::open(disc).expect("open");
    let batch = [(art("Super Ironhead"), 0x0C), (art("Love You"), 0x1F)];
    apply::set_super_art_power(&mut a, &batch).unwrap();
    apply::set_super_art_power(&mut b, &batch).unwrap();
    assert!(
        a.image() == b.image(),
        "the same Super Art power edit must reproduce the image byte-for-byte"
    );
}

#[test]
fn the_combo_keyed_editors_still_cannot_reach_a_super_art() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(disc).expect("open");
    let scus_img = scus(&patcher);

    // The reason this feature exists: no Super Art has a combo, so the SCUS
    // arts-name table (what `--arts-power` and `--arts-ap-*` key on) holds no
    // row for one. Assert the table's row set really is the 45 regular arts.
    let rows = legaia_art::arts_table::raw_records_from_scus(&scus_img).unwrap();
    assert_eq!(
        rows.len(),
        45,
        "the arts-name table is 15 arts per character"
    );
    let names: Vec<String> = legaia_art::arts_table::parse_from_scus(&scus_img)
        .unwrap()
        .into_iter()
        .map(|e| e.name)
        .collect();
    for s in legaia_art::SUPER_ARTS {
        assert!(
            !names.iter().any(|n| n == s.name),
            "{} must not appear in the arts-name table",
            s.name
        );
    }

    // And each Super Art's record really does carry an empty combo field: the
    // stub run at `+0` is a single byte, shorter than any real art's input.
    let entry = patcher
        .read_entry(player_entry_index(Character::Vahn))
        .unwrap();
    let dec = legaia_patcher::arts::player_record0_decoded(&entry).unwrap();
    let located = super_art_power::super_art_powers_in(&scus_img, &dec, Character::Vahn);
    assert_eq!(located.len(), 5);
    let super_offs: Vec<usize> = located.iter().map(|r| r.record_off).collect();
    for row in &located {
        let run = dec[row.record_off..]
            .iter()
            .take_while(|&&b| (1..=4).contains(&b))
            .count();
        assert!(
            run <= 1,
            "{} carries no real input combo (run {run})",
            row.name
        );
    }
    // The converse, which is what actually makes the combo editors blind to a
    // Super Art: no arts-table combo resolves onto a Super Art's record.
    for r in rows.iter().filter(|r| r.character == Character::Vahn) {
        if r.commands.is_empty() {
            continue;
        }
        let cmds: Vec<Command> = r.commands.clone();
        for off in legaia_patcher::arts_power::find_records_by_combo(&dec, &cmds) {
            assert!(
                !super_offs.contains(&off),
                "combo-keyed lookup must never land on a Super Art record"
            );
        }
    }
}
